# Phase 3 — Split the worktree into two coherent commits

> **Recommended Codex model: GPT 5.5 medium**
>
> The git plumbing is mechanical, but the structural call —
> *which hunks of `crates/modde-cli/src/main.rs` go in Commit 1
> versus Commit 2* — needs reading the surrounding code to decide.
> The Exec subcommand (Commit 1, "test harness + exec alias") and
> the Skill subcommand (Commit 2, "install pipeline rework + skill
> surface") sit adjacent in the same enum, so a `low` model with a
> naïve `git add main.rs` lumps them. A `medium` model has the
> headroom for `git add -p` + the read of why each hunk belongs
> where. No design content, no architecture.
>
> Routed as **leaf × moderate** in the `gpt-plan-routing` matrix.

## Working tree

`/data/nvme0/can/Projects/rs-modde`

**Depends on Phase 1 and Phase 2** — both must have landed (in the
index, not necessarily committed) before this phase can finalise
commit messages and content. Phase 1 mutates Cargo.toml + Cargo.lock,
which need to land in Commit 1 (infrastructure / tooling). Phase 2
mutates TODO.md + REMAINING_WORK.md, which need to land in Commit 1
too.

## Goal

Two commits on `trunk` ahead of `origin/trunk`:

1. **Commit 1** — Wire coverage, CLI test harness, and `modde exec`
   alias.
2. **Commit 2** — Rework Wabbajack install pipeline for
   memory-bounded large lists; introduce `modde skill`, manual /
   mediafire sources, and native decompression.

Each commit's `git show --stat` reads as a coherent unit. `git
bisect` between them lands in a buildable state. Neither commit
contains stray hunks from the other.

## Why this matters now

The conversation that produced this work had two distinct sessions
that got entangled in the index because of a stash-pop:

- **Session A** (already staged): coverage tooling + CLI integration
  harness + `modde exec` alias + `MODDE_NEXUS_BASE_URL` override +
  proptest/criterion deps + TODO/REMAINING_WORK docs.
- **Session B** (worktree only): INSTALL_PIPELINE_REWORK phases
  1–9, the `modde skill` subcommand, new wabbajack modules
  (acquire/diagnostics/impact/inline/staging), mediafire +
  manual + decompress + cache + link modules, agent skills
  (manual-archive-curation, modde-hm-integration,
  wabbajack-readiness).

Squashing the two sessions into one commit obscures the audit trail
needed by [docs/INSTALL_PIPELINE_REWORK.md](../../../docs/INSTALL_PIPELINE_REWORK.md):
when a future operator runs `git log -- docs/INSTALL_PIPELINE_REWORK.md`,
they should see the single commit that landed it. When they run
`git log -- crates/modde-cli/tests/common/mod.rs`, they should see
the single commit that landed the test harness.

## Out of scope

- Pushing the commits (Phase 5).
- Running CI commands locally (Phase 4).
- Rewriting either commit's *content* — only authoring messages and
  selecting which file goes where. If a code change is wrong, fix it
  forward in a follow-up commit; do not rewrite history.
- Tagging or releasing.
- Cherry-picking these to another branch.

## Plan

1. Sanity check the current index against the expected Commit 1
   scope. Run:
   ```
   git diff --cached --stat
   ```
   Expected file list (33 entries) is in the "Commit 1 manifest"
   section below. If anything differs, investigate before proceeding.

2. **Handle files that were modified twice (staged set + further
   worktree edits):** These need a hunk split.

   The affected files are:

   | File | Commit 1 owns | Commit 2 owns |
   |---|---|---|
   | `crates/modde-cli/src/main.rs` | `Exec` enum + `ExecAction` + Exec match arms + `Exec` notification policy | `Skill` enum + `SkillAction` + Skill match arms + Skill notification policy |
   | `crates/modde-cli/tests/cli_exec.rs` | Entire file (Commit 1's test) | Nothing — but rustfmt cleanups from Session B do land here; keep them in Commit 1 since they make Session A's content cleaner |
   | `crates/modde-cli/tests/cli_update_check.rs` | Entire file | Same — formatting cleanups in Commit 1 |
   | `crates/modde-cli/tests/snapshots/*.snap` | The baseline snapshots without `Skill` | **Updated snapshots with `Skill`** — but Commit 1's tests must remain green, so the snapshots must reflect Commit 1's main.rs state, *not* Commit 2's. See step 3. |
   | `Cargo.toml` (workspace) | `insta`, `assert_cmd`, `wiremock`, `criterion`, `proptest` workspace deps | `lz4_flex`, `sevenz-rust2`, `reflink-copy`, `zstd`, `bytes`, `lru`, `parking_lot` workspace deps |
   | `Cargo.lock` | Lockfile rows for Commit 1's deps | Lockfile rows for Commit 2's deps + memory-admission git-source line from Phase 1 |
   | `crates/modde-cli/Cargo.toml` | `insta`, `assert_cmd`, `wiremock` dev-deps | (likely none, but check) |
   | `crates/modde-core/Cargo.toml` | `criterion`, `proptest` dev-deps + `[[bench]] vfs_deploy` | `lz4_flex`, `reflink-copy` deps |
   | `crates/modde-sources/Cargo.toml` | (likely none from Session A) | All new deps + `rar` feature + `memory-admission` (post-Phase-1) git pin + `wiremock` dev-dep |

3. **Snapshot subtlety:** Commit 1's `cli_help_snapshots` tests must
   pass against Commit 1's `main.rs` (which has Exec but not Skill).
   Strategy: regenerate the snapshots from Commit 1's main.rs state
   before authoring Commit 1, then *update* the snapshots in Commit 2
   when adding the Skill subcommand. The `.snap` files thus appear in
   both commits — that's fine and expected.

   To regenerate cleanly, the safest sequence is:
   - Stash Commit-2-only hunks of main.rs (use the per-hunk `git stash push -p`).
   - Re-run `cargo test -p modde-cli --test cli_help_snapshots`.
   - Move `.snap.new` → `.snap` for any pending updates.
   - Confirm green: `cargo test -p modde-cli --test cli_help_snapshots`.
   - Stage and commit the now-clean Commit 1.
   - Pop the stash.
   - Re-run the snapshot tests; insta writes new `.snap.new` files
     for Commit 2 — accept them.
   - Stage Commit 2 and commit.

4. **Commit 1 message** (use `git commit -m "$(cat <<'EOF' ... EOF)"`):
   ```
   Wire coverage tooling, CLI test harness, and modde exec alias

   - cargo-llvm-cov + just coverage* recipes; Forgejo Actions runs
     just coverage-ci with FAIL_UNDER=0 placeholder.
   - CLI integration harness: shared tests/common/mod.rs Fixture
     isolating MODDE_DATA_DIR + HOME + XDG.
   - insta snapshot tests for help output (top-level / install /
     nxm / profile / no-args-error / unknown-subcommand).
   - End-to-end tests via wiremock: nxm dispatch, nexus status
     (premium/free/401), install-mod failure paths, update-check
     short-circuit, scan dispatch, exec round-trip.
   - modde-sources Nexus refactor: MODDE_NEXUS_BASE_URL /
     MODDE_NEXUS_GRAPHQL_URL env-var overrides so tests can point
     the client at a wiremock server.
   - modde exec top-level CLI alias: thin shim over the existing
     modde tool *-executable handlers; same DB row, more
     discoverable surface.
   - Workspace dev-deps: insta, assert_cmd, wiremock, criterion,
     proptest. vfs_deploy bench in modde-core. Resolver proptest
     with 4 properties.
   - REMAINING_WORK.md + TODO.md handoff docs.

   Workspace test count: 1,475 → <N> passing. (Measure with
   `cargo test --workspace --tests --no-fail-fast` after staging
   Commit 1 but before authoring the message.)
   ```

5. **Commit 2 message:**
   ```
   Rework Wabbajack install pipeline for memory-bounded large lists

   See docs/INSTALL_PIPELINE_REWORK.md for the full design.

   Phase 1: per-archive batched apply (open each archive once).
   Phase 2: native Rust decompression (sevenz-rust2 + unrar-ng
     behind rar feature; no 7zz / unrar subprocess fallback).
   Phase 3: streaming I/O for large outputs (1 MiB copy buffer).
   Phase 4: InlineFile zip index (shared Arc<InlineSource>).
   Phase 5: hardlink/reflink Stock Game + deploy (modde_core::link).
   Phase 6: bounded LRU patch-source cache (ByteLruCache).
   Phase 7a/7c: zstd-recompressed staging; reflink-aware deploy.
   Phase 8: streaming hash verification on download.
   Phase 9: resumable apply via per-batch JSON sentinels under
     _state/archive-batches/.

   Deferred: Phase 7b (CAS chunk-dedup store), Phase 7d (zstd
   content trains).

   New surfaces:
   - modde skill subcommand: install / list / path for the
     bundled agent skill tree.
   - modde-sources::manual: manual-archive curation for sources
     Wabbajack can't auto-acquire.
   - modde-sources::mediafire: MediaFire download backend.
   - modde-sources::decompress: native archive readers.
   - modde-sources::cache: bounded byte-LRU.
   - modde-sources::wabbajack::{acquire,diagnostics,impact,
     inline,staging}: assess/acquire/curate flow.
   - modde-core::link: link_or_copy with hardlink → reflink →
     copy fallback.

   memory-admission migrated to a git dep (codeberg.org pin) so
   CI and outside contributors can build.

   CI migrated from Woodpecker to Forgejo Actions
   (.forgejo/workflows/{ci,pages,release}.yml supersedes
   .woodpecker/{check,release,site}.yml).

   Workspace test count: <N from Commit 1 message> → 1,563
   passing.
   ```

6. **Stage Commit 2's content** explicitly — do *not* use
   `git add -A` since the agent skills (`.agents/skills/*/SKILL.md`)
   include both modified and new files that all belong in Commit 2.
   See "Commit 2 manifest" below.

7. **Hook compliance:** if either commit has pre-commit / pre-push
   hooks (check `.git/hooks/`), let them run. The user's CLAUDE.md
   forbids `--no-verify`. If a hook fails, fix forward in a new
   commit; never amend or rebase the just-authored commits.

8. **Verify the split** with:
   ```
   git log --oneline -2
   git show --stat HEAD
   git show --stat HEAD~1
   git diff origin/trunk..HEAD --stat | tail -1
   ```
   Confirm the two commits cover all 92 expected files. Confirm
   neither commit has a stray file from the other.

## Commit 1 manifest (Session A — coverage + harness + exec alias)

```
.woodpecker/check.yml                                   # staged-modified; deleted in Session B (see Commit 2)
Cargo.lock                                              # partial — Session A deps only
Cargo.toml                                              # partial — Session A workspace deps only
REMAINING_WORK.md                                       # post-Phase-2 reconciled version
TODO.md                                                 # post-Phase-2 reconciled version
crates/modde-cli/Cargo.toml                             # partial — Session A dev-deps
crates/modde-cli/src/commands/update.rs                 # entire diff
crates/modde-cli/src/main.rs                            # Exec enum + ExecAction + match arms only
crates/modde-cli/tests/cli_exec.rs
crates/modde-cli/tests/cli_help_snapshots.rs
crates/modde-cli/tests/cli_install_mod.rs
crates/modde-cli/tests/cli_nexus_status.rs
crates/modde-cli/tests/cli_nxm_dispatch.rs
crates/modde-cli/tests/cli_scan_dispatch.rs
crates/modde-cli/tests/cli_update_check.rs
crates/modde-cli/tests/common/mod.rs
crates/modde-cli/tests/snapshots/cli_help_snapshots__snapshot_install_help.snap
crates/modde-cli/tests/snapshots/cli_help_snapshots__snapshot_no_args_error.snap
crates/modde-cli/tests/snapshots/cli_help_snapshots__snapshot_nxm_help.snap
crates/modde-cli/tests/snapshots/cli_help_snapshots__snapshot_profile_help.snap
crates/modde-cli/tests/snapshots/cli_help_snapshots__snapshot_top_level_help.snap
crates/modde-cli/tests/snapshots/cli_help_snapshots__snapshot_unknown_subcommand_error.snap
crates/modde-core/Cargo.toml                            # criterion + proptest dev-deps + [[bench]]
crates/modde-core/benches/vfs_deploy.rs
crates/modde-core/src/installer/analyze.rs
crates/modde-core/src/ipc.rs
crates/modde-core/tests/resolver_proptest.rs
crates/modde-sources/src/nexus/api.rs
crates/modde-sources/src/nexus/auth.rs
crates/modde-sources/src/nexus/cdn.rs                   # partial — base_url() helper call only
crates/modde-sources/src/nexus/graphql.rs
crates/modde-sources/src/nexus/mod.rs                   # base_url() / graphql_url() helpers
justfile
```

## Commit 2 manifest (Session B — install pipeline rework + skill + new modules)

Modified tracked files:
```
.agents/skills/add-game/SKILL.md
.agents/skills/add-ue4-game/SKILL.md
.agents/skills/modde-installer/SKILL.md
.agents/skills/optiscaler-quirks/SKILL.md
Cargo.lock                                              # Session B deps + memory-admission git source
Cargo.toml                                              # Session B workspace deps
crates/modde-cli/Cargo.toml                             # any Session B dev-deps
crates/modde-cli/src/commands/install.rs
crates/modde-cli/src/commands/mod.rs                    # skill module registration
crates/modde-cli/src/commands/update.rs                 # Session B portion if any
crates/modde-cli/src/commands/wabbajack.rs              # +1,445 lines: assess / acquire / hm-snippet / etc.
crates/modde-cli/src/main.rs                            # Skill enum + SkillAction + match arms
crates/modde-cli/tests/cli_smoke.rs
crates/modde-cli/tests/snapshots/*.snap                 # regenerated with Skill in help
crates/modde-core/Cargo.toml                            # lz4_flex + reflink-copy
crates/modde-core/src/bethesda_archive.rs
crates/modde-core/src/hash/mod.rs
crates/modde-core/src/lib.rs                            # link module export
crates/modde-core/src/manifest/wabbajack.rs             # +282 lines: grouped-by-archive helper
crates/modde-core/src/stock/mod.rs
crates/modde-core/tests/manifest_validation_tests.rs
crates/modde-games/src/bethesda/saves.rs
crates/modde-games/src/optiscaler.rs
crates/modde-games/src/tools/optiscaler.rs
crates/modde-games/src/traits.rs
crates/modde-games/src/witcher3/mod.rs
crates/modde-sources/Cargo.toml                         # all new deps + rar feature
crates/modde-sources/src/common.rs
crates/modde-sources/src/direct/mod.rs
crates/modde-sources/src/gdrive/mod.rs
crates/modde-sources/src/lib.rs                         # new module exports
crates/modde-sources/src/nexus/cdn.rs                   # normalize_nexus_game_domain wiring
crates/modde-sources/src/traits.rs
crates/modde-sources/src/wabbajack/bsa_repack.rs
crates/modde-sources/src/wabbajack/catalog.rs
crates/modde-sources/src/wabbajack/cdn.rs
crates/modde-sources/src/wabbajack/installer.rs        # +2,496 lines
crates/modde-sources/src/wabbajack/mod.rs
crates/modde-sources/src/wabbajack/patcher.rs
crates/modde-sources/src/wabbajack/runner.rs
crates/modde-sources/src/wabbajack/validator.rs
crates/modde-sources/tests/download_source_tests.rs
crates/modde-sources/tests/gdrive_tests.rs
crates/modde-sources/tests/validator_integration.rs
crates/modde-sources/tests/wabbajack_comprehensive_tests.rs
crates/modde-ui/src/app.rs
crates/modde-ui/src/semantics.rs
crates/modde-ui/src/views/tabs.rs
crates/modde-ui/src/views/tools.rs
nix/hm-module.nix
```

Deletions (`.woodpecker/` retired in favour of `.forgejo/workflows/`):
```
.woodpecker/check.yml                                   # superseded by .forgejo/workflows/ci.yml
.woodpecker/release.yml                                 # superseded by .forgejo/workflows/release.yml
.woodpecker/site.yml                                    # superseded by .forgejo/workflows/pages.yml
```

New untracked files:
```
.forgejo/workflows/ci.yml
.forgejo/workflows/pages.yml
.forgejo/workflows/release.yml
.agents/skills/manual-archive-curation/SKILL.md
.agents/skills/modde-hm-integration/SKILL.md
.agents/skills/wabbajack-readiness/SKILL.md
crates/modde-cli/src/commands/skill.rs
crates/modde-cli/tests/cli_skill.rs
crates/modde-cli/tests/cli_wabbajack_acquire.rs
crates/modde-core/src/link.rs
crates/modde-sources/src/cache.rs
crates/modde-sources/src/decompress/mod.rs
crates/modde-sources/src/manual/mod.rs
crates/modde-sources/src/mediafire/mod.rs
crates/modde-sources/src/wabbajack/acquire.rs
crates/modde-sources/src/wabbajack/diagnostics.rs
crates/modde-sources/src/wabbajack/impact.rs
crates/modde-sources/src/wabbajack/inline.rs
crates/modde-sources/src/wabbajack/staging.rs
docs/INSTALL_PIPELINE_REWORK.md
docs/planning/ship-install-pipeline-rework/*.md          # this plan set
```

## Acceptance criteria

- [ ] `git log -2 --oneline` shows exactly two new commits with the
      planned subject lines.
- [ ] `git status --porcelain` is empty post-split (no leftover
      working-tree changes, no leftover untracked files).
- [ ] `git diff origin/trunk..HEAD --stat | tail -1` reports
      approximately `92 files changed, ~7,400 insertions(+), ~1,000 deletions(-)`.
- [ ] `git show HEAD~1 --stat | grep -c '^ '` reports ~33 files
      (Commit 1).
- [ ] `git show HEAD --stat | grep -c '^ '` reports ~59 files
      (Commit 2).
- [ ] `git show HEAD~1 --stat | grep -E 'wabbajack/installer\.rs'`
      returns nothing — installer.rs must not be in Commit 1.
- [ ] `git show HEAD~1 --stat | grep -E 'crates/modde-cli/tests/common/mod\.rs'`
      returns the file — common/mod.rs must be in Commit 1.
- [ ] Both commits compile in isolation: `git checkout HEAD~1 -- . && cargo check --workspace` then `git checkout HEAD -- . && cargo check --workspace`. (Run this only if confidence is low — it requires resetting the worktree twice.)

## Files likely touched

All 92 changed files end up in one of the two commits. No files are
*modified* by this phase itself — only staged and committed.

This phase also lands the plan documents themselves
(`docs/planning/ship-install-pipeline-rework/*.md`) in Commit 2.

## Pitfalls

- **`git add -p` for `crates/modde-cli/src/main.rs` is fiddly.** The
  `Exec` enum and `Skill` enum are adjacent — both sit in the
  `Commands` enum and both have a parallel `*Action` enum. `git
  add -p` will offer them as separate hunks; double-check before
  hitting `y`. Symptom of mis-split: Commit 1's `cargo build` fails
  because `Commands::Skill` references the non-existent
  `commands::skill::handle`. Recovery: `git restore --staged
  crates/modde-cli/src/main.rs` and redo.
- **Snapshot files in two commits.** As described in step 3, the
  same `.snap` file legitimately changes in both commits. Reviewers
  may flag this; the commit messages should make it clear that
  Commit 2's snapshot update is *because the help output now
  includes Skill*.
- **`Cargo.lock` partial-split is non-trivial.** Cargo regenerates
  the lockfile holistically. Pragmatic approach: stage the entire
  `Cargo.lock` in Commit 1 reflecting Commit 1's `Cargo.toml`, then
  re-run `cargo check` after staging Commit 2's `Cargo.toml` so the
  lockfile gets the Commit 2 deps; commit that diff with Commit 2.
  Symptom of doing it wrong: Commit 1 has lockfile rows for crates
  (e.g., `sevenz-rust2`) that aren't yet referenced anywhere, which
  `cargo` will silently tolerate but a reviewer will catch.
- **Hook surprises.** If the user has a pre-commit hook that runs
  `cargo fmt --check`, it'll run twice (once per commit). Recovery:
  let it run; Phase 4 verifies fmt is clean.
- **Phase 1's Cargo.toml change ends up in the wrong commit.** Per
  the manifest, the memory-admission git pin belongs in *Commit 2*
  (it's enabling Session B's installer.rs to compile in CI). Not in
  Commit 1.

## Reference

- Originating split: chat session 2026-05-17 readiness audit
  identified two distinct sessions tangled in the index.
- Plan: this directory
  ([docs/planning/ship-install-pipeline-rework/](.)).
- Companion phases: [01](./01-memory-admission-remote-dep.md) (must
  land in index first), [02](./02-reconcile-planning-docs.md)
  (must land in index first), [04](./04-ci-parity-validation.md)
  (runs against the post-split state).
- Author convention reference:
  [.../CLAUDE.md](../../../) commit-message section.
