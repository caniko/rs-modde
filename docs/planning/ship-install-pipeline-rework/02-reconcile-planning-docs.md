# Phase 2 — Reconcile planning docs with the working tree

> **Recommended Codex model: GPT 5.5 low**
>
> Pure mechanical reconciliation: every claim that needs verifying
> can be checked with `grep` or by reading a section header in an
> existing file. No design decisions, no architecture, no API
> tradeoffs. The "judgement" part is just "did this item land fully,
> partially, or not at all" — and the evidence is in the worktree
> next to the doc. A `low` model has the headroom for this and the
> cost is dominated by editing diffs, not reasoning.
>
> Routed as **leaf × trivial** in the `gpt-plan-routing` matrix.

## Working tree

`/data/nvme0/can/Projects/rs-modde`

Independent of Phase 1 — touches different files entirely. Can run
in parallel with Phase 1; both must land before Phase 3.

## Goal

[TODO.md](../../../TODO.md) and
[REMAINING_WORK.md](../../../REMAINING_WORK.md) faithfully describe
the **current** worktree, including the install-pipeline-rework
phases now landed in
[docs/INSTALL_PIPELINE_REWORK.md](../../../docs/INSTALL_PIPELINE_REWORK.md).
An outside reader (or future agent) using the planning docs as an
entry point gets a consistent picture: completed items are ticked,
in-progress items use `[~]`, deferred items use `[ ]` with a brief
"deferred because…" hook.

## Why this matters now

The two planning docs are about to be **committed** as part of
Commit 1 (Phase 3 splits the history). If they ship stale, every
agent that picks them up in a future session will treat already-done
work as TODO. The drift is:

- TODO.md §1.4 lists "Durable resumable downloads" / "Resume across
  process restarts" as `[ ]`. INSTALL_PIPELINE_REWORK.md Phase 9
  marks the apply-side resumable layer as ✅ landed.
- TODO.md §1.4 lists "Integrity verification surfaced in CLI" as
  `[ ]`. INSTALL_PIPELINE_REWORK.md Phase 8 marks streaming
  verification as ✅ landed.
- TODO.md §3.2 lists "Clippy `-D warnings` gate" as `[ ]`.
  [.woodpecker/check.yml:7](../../../.woodpecker/check.yml#L7)
  already runs it and the worktree passes clean.
- REMAINING_WORK.md says "Workspace state at session end: **1,475
  tests passing**". Current worktree:
  `cargo test --workspace --tests --no-fail-fast` reports
  **1,563 passing**.
- Neither doc mentions `modde skill`, the
  [decompress/](../../../crates/modde-sources/src/decompress/)
  module, the new
  [link.rs](../../../crates/modde-core/src/link.rs) /
  [cache.rs](../../../crates/modde-sources/src/cache.rs) /
  [manual/](../../../crates/modde-sources/src/manual/) /
  [mediafire/](../../../crates/modde-sources/src/mediafire/)
  modules, nor the
  [wabbajack/{acquire,diagnostics,impact,inline,staging}.rs](../../../crates/modde-sources/src/wabbajack/)
  surfaces.

## Out of scope

- Authoring new feature designs or revising the prioritisation. Only
  reconcile the *state* of existing items against what's in the
  worktree.
- Writing a CHANGELOG. Mentioned as an optional bullet in TODO.md
  §3.4 — defer to a separate phase if the user wants it. Adding one
  here is fine; not adding one isn't a blocker.
- Editing [docs/INSTALL_PIPELINE_REWORK.md](../../../docs/INSTALL_PIPELINE_REWORK.md)
  itself. Its status legend (✅/🟡/⬜) is authoritative; TODO.md
  should reflect what INSTALL_PIPELINE_REWORK declares, not the
  other way round.
- Touching [docs/planning/user-defined-games/](../user-defined-games/)
  or any other unrelated doc.

## Plan

1. Build the verification corpus by re-reading the source of truth:
   ```
   grep -nE '^\*\*Status: (✅|🟡|⬜)' docs/INSTALL_PIPELINE_REWORK.md
   ```
   List every phase's claimed status. For "landed" entries, verify by
   spot-checking the named entry-point file (e.g.,
   `crates/modde-sources/src/wabbajack/inline.rs` exists for Phase 4).
2. Update [TODO.md](../../../TODO.md) item by item, in section order:
   - **§1.4** Resumable downloads: change three `[ ]` items to `[x]`
     with a parenthetical `(INSTALL_PIPELINE_REWORK Phase 9)`.
   - **§1.4** Integrity verification in CLI: change to `[x]` with
     `(Phase 8 streaming verify)`. If the CLI surface is partial,
     use `[~]` and name what's missing.
   - **§3.2** Clippy `-D warnings`: change to `[x]` with `(wired in
     .woodpecker/check.yml; passing on trunk)`.
   - **§2.6** Archive-extract bench: keep `[ ]`. Add a one-line note
     that the bench target should benchmark the new native
     [decompress/](../../../crates/modde-sources/src/decompress/)
     readers, not the old `7zz` subprocess.
   - **New ticked items** to insert (each with a one-line context):
     - §1.4 new entry: `[x] Native Rust decompression (zip / 7z / RAR / BSA / BA2) (Phase 2 of INSTALL_PIPELINE_REWORK)`
     - §1.4 new entry: `[x] Hardlink/reflink-aware deploy + Stock Game (Phase 5)`
     - §1.4 new entry: `[x] Bounded LRU patch-source cache (Phase 6)`
     - §1.4 new entry: `[x] zstd-recompressed staging (Phase 7a/7c)`
     - §1.4 new entry: `[ ] CAS chunk-dedup store (Phase 7b — deferred)`
     - §1.4 new entry: `[x] InlineFile zip index (Phase 4)`
     - §1.4 new entry: `[x] Per-archive batched apply (Phase 1)`
     - §1.4 new entry: `[x] Streaming I/O for large outputs (Phase 3)`
     - New top-level subsection or §1.1 entry: `[x] modde skill subcommand for installing agent skills`
     - §1.4 new entry: `[x] MediaFire and manual-archive download sources`
3. Update [REMAINING_WORK.md](../../../REMAINING_WORK.md):
   - **"What landed this session"** section: append a paragraph
     summarising the install-pipeline-rework wave with a link to
     [docs/INSTALL_PIPELINE_REWORK.md](../../../docs/INSTALL_PIPELINE_REWORK.md).
     Cover: per-archive batching, native decompression, streaming
     I/O, inline zip index, hardlink/reflink deploy, byte cache,
     zstd staging, verify-on-download, resumable apply.
   - **"Workspace state at session end"** line: replace `1,475 tests
     passing` with `1,563 tests passing` (verify by running
     `cargo test --workspace --tests --no-fail-fast`).
   - **§1.4 "Resumable downloads"** subsection (currently under
     "Needs design / discussion"): move to a new "What's done" entry
     near the top, with a one-line cross-ref to Phase 9. Or delete
     and replace with `(see [docs/INSTALL_PIPELINE_REWORK.md](../../../docs/INSTALL_PIPELINE_REWORK.md) Phase 9)`.
4. Re-read both docs end-to-end and verify no remaining contradictions
   with the worktree.

## Acceptance criteria

- [ ] `grep -nF '[ ] Durable resumable downloads' TODO.md` returns
      nothing.
- [ ] `grep -nF '[ ] Resume across process restarts' TODO.md`
      returns nothing.
- [ ] `grep -nF 'Clippy `-D warnings` gate' TODO.md` shows the line
      ticked `[x]`.
- [ ] `grep -nF '1,475 tests passing' REMAINING_WORK.md` returns
      nothing; `grep -nF '1,563 tests passing'` returns one line.
- [ ] At least three of: `modde skill`, `decompress`,
      `link_or_copy`, `ByteLruCache`, `InlineSource` appear in
      TODO.md as ticked-or-cited items.
- [ ] `cargo test --workspace --tests --no-fail-fast 2>&1 | grep -E '^test result:' | awk '{p+=$4} END {print p}'`
      matches the number cited in REMAINING_WORK.md.

## Files likely touched

- [TODO.md](../../../TODO.md) — ~15 line additions/edits across §1.4,
  §3.2, §2.6
- [REMAINING_WORK.md](../../../REMAINING_WORK.md) — new "What landed
  this session" paragraph, test-count update, §1.4 restructure

## Pitfalls

- **Over-ticking.** Marking an item `[x]` when only the
  installer-side landed but the CLI/UI surface is still missing.
  Symptom: future agent reads "done", goes to use it, discovers a
  half-built feature. Recovery: when in doubt, use `[~]` (in
  progress) with a sub-bullet naming what's missing — that's the
  existing pattern in §2.4.
- **Mid-section renumbering.** TODO.md sections are referenced by
  number in REMAINING_WORK.md ("§1.4", "§2.1"). Renumber and you
  break the cross-references. Recovery: don't renumber; only edit
  contents.
- **Repeating INSTALL_PIPELINE_REWORK content verbatim.** TODO.md
  should link, not duplicate. Symptom: 200 extra lines of TODO.md
  paraphrasing INSTALL_PIPELINE_REWORK. Recovery: keep each item
  to one line; link to the source for depth.

## Reference

- Source of truth:
  [docs/INSTALL_PIPELINE_REWORK.md](../../../docs/INSTALL_PIPELINE_REWORK.md)
- Existing planning conventions:
  [docs/planning/user-defined-games/](../user-defined-games/)
- Originating audit: chat session 2026-05-17 readiness review.
