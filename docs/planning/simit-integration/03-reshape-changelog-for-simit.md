# Phase 03 — Reshape `CHANGELOG.md` for simit's strict Keep-a-Changelog parser

> **Recommended Codex model: gpt-5.4-mini, effort medium**
>
> Three lines of Markdown to edit, one regex sanity check, one dry-run validation.
> Leaf node — no design, no cross-file impact. mini handles this cleanly; medium
> effort buys the care to not damage the existing 0.1.0 section while reshaping
> the Unreleased heading.

## Working tree

`/data/nvme0/can/Projects/rs-modde`. Independent of Phase 01. Mostly independent of
Phase 02 (only needs simit on `$PATH` for the dry-run validation step — until then, run
the dry-run via `cargo install --path /data/nvme0/can/Projects/simit` or skip the
validation step and verify by reading the simit parser source). Can run in parallel
with Phase 01.

## Goal

`CHANGELOG.md` contains a section header exactly matching `## [Unreleased]` (note the
brackets) so simit's `changelog::planned_update` can find its insertion anchor. The
existing `## Unreleased` heading is converted in place, preserving every entry beneath
it; the released-version sections (`## [0.1.0] - 2026-04-13`) are unchanged. A
`simit release --dry-run patch -m "test"` invocation prints a plausible plan and exits 0.

## Why this matters now

simit's release pipeline asserts the literal marker before doing any work:

```rust
// simit/src/render/changelog.rs:25-27
let marker = "## [Unreleased]";
if !text.contains(marker) {
    bail!("CHANGELOG.md must contain `## [Unreleased]`");
}
```

rs-modde's current CHANGELOG uses the unbracketed form ([CHANGELOG.md:8](../../CHANGELOG.md#L8)):

```
## Unreleased
```

Without this fix, the first `simit release` invocation in Phase 04 bails immediately —
which is recoverable but leaves a broken state if someone runs it manually before Phase
04 lands. Doing the rename in its own commit also makes the change easy to bisect: if
the changelog renderer regresses upstream, `git log -- CHANGELOG.md` points at this
single line change.

The bracket convention is the one Keep-a-Changelog 1.1.0 documents
(https://keepachangelog.com/en/1.1.0/) — rs-modde's CHANGELOG already declares
adherence on line 5, so this is fixing a pre-existing inconsistency, not introducing
a new convention.

## Out of scope

- Rewriting any released-version section.
- Adding new `### Added` / `### Changed` subheadings or content to the Unreleased
  section — that is release-content work, not tooling work.
- Wiring simit into xtask (Phase 04).
- Setting up CI checks that lint the CHANGELOG (Phase 05 considers but does not commit
  to this).

## Plan

1. **Read [simit/src/render/changelog.rs](../../../../simit/src/render/changelog.rs)
   end to end** before editing. Confirm:
   - The marker string is `## [Unreleased]` exactly (line ~25).
   - Whether simit also expects a version-link footer (Keep-a-Changelog defines
     comparison links at the bottom). If yes and rs-modde's CHANGELOG lacks them,
     decide: (a) accept that simit will not auto-generate them; (b) add the footer
     once and let simit maintain it going forward. The default decision is (a) —
     simpler, fewer moving parts. If the parser *requires* the footer, switch to (b).
2. **Edit [CHANGELOG.md](../../CHANGELOG.md)**: change the single line
   `## Unreleased` to `## [Unreleased]`. Make no other changes in this commit.
3. **Verify with a dry-run** (requires Phase 02 landed *or* simit installed via
   `cargo install`). From a clean working tree:
   ```
   nix develop --command simit release --dry-run patch -m "validation"
   ```
   Expect output along the lines of:
   ```
   simit release dry-run
   package modde-core: 0.1.0 -> 0.1.1
   ... (one line per workspace package)
   would run cargo test and cargo clippy
   would update CHANGELOG.md
   ...
   ```
   Exit code must be 0. If exit is non-zero with the "must contain `## [Unreleased]`"
   error, the edit in step 2 didn't land — fix and retry.
4. **Reset any cargo edits the dry-run may have left** (`simit release --dry-run` does
   not touch files per [release.rs:24-39](../../../../simit/src/commands/release.rs#L24-L39),
   but verify with `git status` and `git diff` to be sure).
5. **Commit**:
   ```
   git add CHANGELOG.md
   git commit -m 'docs(changelog): switch Unreleased heading to bracketed form'
   ```

## Acceptance criteria

- [ ] `CHANGELOG.md` contains the exact substring `## [Unreleased]` (verify with `grep -F '## [Unreleased]' CHANGELOG.md`).
- [ ] The bare `## Unreleased` heading no longer appears (`grep -cE '^## Unreleased$' CHANGELOG.md` returns 0).
- [ ] Every entry that previously appeared under `## Unreleased` still appears under `## [Unreleased]` in the same order (visual diff or `git show HEAD CHANGELOG.md | grep -c '^- '` matches pre-phase count).
- [ ] The released-version section header `## [0.1.0] - 2026-04-13` is unchanged.
- [ ] `nix develop --command simit release --dry-run patch -m "validation"` exits 0 and prints `would update CHANGELOG.md`.
- [ ] `git status` after the dry-run shows only the intended single-line CHANGELOG.md edit (no stray Cargo.toml or Cargo.lock changes).
- [ ] Commit message is `docs(changelog): switch Unreleased heading to bracketed form`.

## Files likely touched

- [CHANGELOG.md](../../CHANGELOG.md) — line 8 only.

## Pitfalls

- **Simit's parser may also require a blank line after the heading** or a specific subsection ordering. If the dry-run fails with a parser error other than the missing marker, read [render/changelog.rs](../../../../simit/src/render/changelog.rs) for the exact shape it expects and conform. Do not add extra structural Markdown that isn't already there unless the parser demands it.
- **Dry-run still requires `cargo metadata` to succeed**. If `cargo metadata` fails (e.g. detritus path-dep at [Cargo.toml:46-47](../../Cargo.toml#L46-L47) is unreachable), the dry-run will fail before reaching the changelog parser. Confirm `cargo metadata --no-deps --format-version 1 > /dev/null` exits 0 first.
- **Don't add content to `[Unreleased]` in this commit**. Mixing tooling work with release-content work makes the commit harder to bisect.
- **The Keep-a-Changelog link footer**: rs-modde's CHANGELOG does not have `[Unreleased]: https://...` comparison links at the bottom. simit (as of the version pinned in Phase 02) does not require them. If a future simit version starts to, that's a Phase-08-ish problem, not this one.

## Reference

- simit changelog parser: [simit/src/render/changelog.rs](../../../../simit/src/render/changelog.rs)
- simit release pipeline: [simit/src/commands/release.rs](../../../../simit/src/commands/release.rs)
- Keep-a-Changelog spec: https://keepachangelog.com/en/1.1.0/
- Current CHANGELOG: [CHANGELOG.md](../../CHANGELOG.md)
