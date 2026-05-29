# Phase 01 — Performance: de-quadratic-ize zip extraction and the filter view

> **Recommended Codex model: GPT 5.5 medium**
>
> Moderate complexity, behavior-critical: the zip rewrite must reproduce the
> 7z/rar path's matching and dedup semantics exactly, and the filter change has
> a real Unicode-vs-ASCII correctness fork. Well-specified with a strong test
> safety net, so `medium` holds the bar; a `low` tier risks a subtly different
> matching pass that silently drops a file. Leaf/sub-agent coding role — no
> cross-system orchestration.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. Same repo as the parent
project. Disjoint from Phases 02 and 09 — safe to run concurrently with them.

## Goal

Wabbajack zip extraction stops re-scanning the whole archive once per requested
entry (it builds a normalized-path lookup map and walks entries once, like the
7z path already does), and the mod-list filter view stops allocating a lowercased
`String` per mod per frame while a text filter is active — with **identical
observable results** in both cases.

## Why this matters now

Two findings from the performance audit that `rust-ultra` left as recommend-only
because they need care, not because they're not real:

- `crates/modde-sources/src/decompress/mod.rs:119` `extract_zip` calls
  `find_zip_entry` (`:611`) once **per request**, and `find_zip_entry` linearly
  scans every archive entry computing `to_lowercase()` per entry — `O(requests ×
  entries)` with heavy allocation. A wabbajack list extracting many files from
  one big zip pays this repeatedly. The 7z path (`extract_seven_z`, `:197`)
  already does it right: `requests_by_normalized_path(requests)` (`:269`) builds a
  map, then one pass over entries.
- `crates/modde-core/src/filter.rs:152` `m.mod_id.to_lowercase()` allocates a
  `String` for every mod, every `view()` frame, whenever a text filter is
  non-empty (i.e. while the user is typing). `O(mods)` allocations per keystroke.

(Performance fixes A1/A2/A3 — the quadratic archive lookups and env-read hoist —
already landed in commit `2509246`. This phase is only A4 + A5.)

## Out of scope

- Any change to `extract_seven_z`, `extract_rar`, or `extract_bethesda` (they're
  already correct — only study them).
- Changing which entries match or the order outputs are produced. This is a pure
  speed change.
- The `normalize_path`/`validate_archive_entry` `Cow` micro-opt (audit finding #8,
  low value — leave it).
- Re-touching the installer archive-lookup map (already done in `2509246`).

## Plan

1. **A4 — zip rescan.** Read `extract_seven_z` (`decompress/mod.rs:197`) and
   `extract_rar` carefully to learn the exact `requests_by_normalized_path` +
   single-pass matching contract: the map key is `normalize_path(name).to_lowercase()`,
   a key may map to multiple requests, `inner_path` requests read bytes and fan
   out via `satisfy_maybe_nested_requests_from_bytes`, `WriteFile` vs `Bytes` are
   handled per request, a `found` set tracks completion, and `ensure_all_found`
   runs at the end.
2. Rewrite `extract_zip` to mirror that: build `by_path =
   requests_by_normalized_path(requests)` once, iterate `0..archive.len()`,
   `archive.by_index(i)`, compute the entry's normalized-lowercased name, look it
   up in `by_path`, and satisfy all matched requests for that entry — preserving
   `validate_zip_entry`, `validate_declared_entry_size` (when `inner_path` is
   `None`), the nested-bytes path, `WriteFile`/`Bytes`, the `found` set, and the
   trailing `ensure_all_found`. Mind that `zip::ZipArchive::by_index` borrows
   mutably, same as `by_name` did.
3. If `find_zip_entry` is now unused, delete it. If anything else calls it, leave
   it and add `#[allow(dead_code)]` only as a last resort (prefer deleting).
4. **A5 — filter allocation.** Read `crates/modde-core/src/filter.rs:138`
   `apply_filters`. The current match is Unicode-correct (`to_lowercase`). Choose
   the **behavior-preserving** option:
   - Preferred: do not change matching semantics. If `EnabledMod` (or the row
     type) can cheaply carry a precomputed lowercased `mod_id`, compute it once at
     construction and compare against that. If that's too invasive, leave A5
     unchanged and record it as deferred in the commit body — do **not** silently
     swap to `eq_ignore_ascii_case`/byte comparison, which changes results for
     non-ASCII `mod_id`s.
   - Only if you can prove every `mod_id` in this codebase is ASCII (it is *not*
     guaranteed) may you use an allocation-free ASCII case-insensitive search, and
     then you must say so in the commit body.
5. Gate and commit. A4 and A5 are independent — they may be one commit
   (`perf: ...`) or two; if A5 ends up deferred, commit A4 alone.

## Acceptance criteria

- [ ] `extract_zip` builds the request map once and walks archive entries once;
      `find_zip_entry`'s per-request full scan is gone (`rg -n 'find_zip_entry'
      crates/modde-sources/src/decompress/mod.rs` shows it removed or unused-and-deleted).
- [ ] `cargo test -p modde-sources` passes with the **same** test count as before
      (no decompress test regresses; zip extraction still finds every requested
      entry, including nested `inner_path` and multi-request-per-entry cases).
- [ ] A5 either: precomputes a lowercased key with `to_lowercase()` (Unicode
      preserved) and the filter result is unchanged, OR is left unchanged with a
      one-line deferral note in the commit body. No ASCII downgrade without an
      explicit justification line.
- [ ] `cargo clippy -p modde-core -p modde-sources --all-targets -- -D warnings` clean.
- [ ] `cargo fmt --check` clean.

## Files likely touched

- `crates/modde-sources/src/decompress/mod.rs` (rewrite `extract_zip`, maybe drop
  `find_zip_entry`).
- `crates/modde-core/src/filter.rs` (A5; possibly the row/`EnabledMod` type if you
  precompute a key — but prefer not to widen scope).

## Pitfalls

- **Symptom:** a decompress test fails with "entry not found" after the rewrite.
  **Cause:** the map key normalization (`normalize_path(...).to_lowercase()`)
  doesn't match how requests were keyed, or `by_index` iteration skips an entry
  type. **Recovery:** diff your matching against `extract_seven_z` line by line;
  the two must key identically.
- **Symptom:** duplicate/overwritten output when one entry satisfies multiple
  requests. **Cause:** you stopped at the first matched request instead of
  satisfying all in `by_path[key]`. **Recovery:** mirror the 7z loop's
  `matched_requests.iter()` handling.
- **Symptom:** clippy `redundant_locals` or `doc_markdown` failure. **Cause:**
  pedantic lints. **Recovery:** backtick code-ish words; don't rebind `let x = x`.
- **A5 trap:** swapping `to_lowercase().contains()` for an ASCII byte search is a
  *behavior change* for non-ASCII mod ids. Treat it as out of scope unless you
  precompute a Unicode-lowercased key.

## Reference

- The correct pattern to mirror: `extract_seven_z` and `requests_by_normalized_path`
  in `crates/modde-sources/src/decompress/mod.rs`.
- Performance audit findings A4 (`decompress find_zip_entry`) and A5
  (`filter.rs apply_filters`) from the rust-ultra design-stage audit.
- Sibling phases: none block this; runs in Wave 0 with [02](./02-observability-library-output.md)
  and [09](./09-dependency-audit.md).
