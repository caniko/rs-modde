# Phase 04 — Replace the xtask release backend (cargo-release → simit)

> **Recommended Codex model: gpt-5.4, effort medium**
>
> Rust code change (subcommand reshape + process invocation) with a non-trivial
> CLI contract migration: rs-modde's xtask currently takes an absolute `Version`;
> simit takes a `BumpKind` enum. We have to break the xtask CLI on purpose and
> update its single in-repo caller (release.yml in Phase 05). Sub-agent — bounded
> to one crate + one workflow file, but the API break needs deliberate handling
> so callers don't silently keep an obsolete invocation.

## Working tree

`/data/nvme0/can/Projects/rs-modde`. Depends on **Phase 02 (simit on `$PATH`)** and
**Phase 03 (CHANGELOG reshape)** — both must be on `trunk` before this phase starts. Do
not begin before phases 02 and 03 are merged; the integration test in step 7 will fail
otherwise.

## Goal

`cargo xtask release <BUMP> [--dry-run]` (where `<BUMP>` is `patch | minor | major |
prerelease`) drives simit's `simit commit` or `simit release` subcommand under the
hood, producing bare-semver tags and updating CHANGELOG.md and Cargo.{toml,lock}. The
old `harbor_xtask::run_release` call is removed from rs-modde. `release.toml`
(cargo-release config) is deleted. The xtask `Cmd::Release` arm is rewritten as a
direct `std::process::Command` invocation of `simit`, not a re-export of a harbor
helper, because no harbor helper for simit exists yet (and adding one is out of
scope — that lives in rs-harbor).

After this phase, `cargo xtask release patch --dry-run` produces simit's dry-run output
verbatim. Nothing in rs-modde references cargo-release any more, *except* the
devShell entry (deferred to Phase 06) so that an unplanned rollback is one revert away.

## Why this matters now

`harbor_xtask::run_release` is the only entry point the release CI calls
([release.yml:47](../../.forgejo/workflows/release.yml#L47) currently uses
`cargo xtask copr vendor`, not the release subcommand — but `cargo xtask release` is the
documented developer entry point for cutting a release, see
[CONTRIBUTING.md](../../CONTRIBUTING.md)). Once Phase 02 and Phase 03 are in, every
release should flow through simit; leaving xtask wired to cargo-release would mean the
developer-facing CLI and the canonical tool disagree.

The prior decision in
[docs/planning/xtask-tooling-cli-uplift/DECISION.md](../xtask-tooling-cli-uplift/DECISION.md)
explicitly chose cargo-release for v0.1 and called the hybrid "rejected for now". This
phase records the reversal. The new decision: **simit is the release backend; xtask is
a thin wrapper that adds the rs-modde-specific concerns (RPM spec rewrite) that simit
doesn't own.**

## Out of scope

- Modifying `harbor_xtask` itself (out-of-tree; lives in rs-harbor). We simply stop
  calling its `run_release`. The `pub fn run_release` remains in rs-harbor for other
  consumers.
- Removing `cargo-release` from the devShell (Phase 06).
- Removing `release.yml`'s COPR / Codeberg / Attic steps. Those run *after* the tag is
  pushed; this phase only changes how the tag is created.
- Adding new xtask subcommands like `cargo xtask init-ci` that wrap `simit init-ci`.
  Phase 05 decides whether to do that.

## Plan

1. **Read the simit CLI contract** ([simit/src/cli.rs:33-103](../../../../simit/src/cli.rs#L33-L103))
   to confirm exact flag names: `--package`, `--workspace`, `--no-tag`, `--no-sign`,
   `--dry-run`, `<BUMP>`, `--pre`, plus `-m` for release. Note that `simit commit`
   trailing args are passed through to `git commit`, but `simit release` consumes `-m`
   as a parameter — the rs-modde xtask call site needs to pick one of the two simit
   subcommands.
2. **Decide which simit subcommand xtask drives**:
   - `simit commit` — version bump + git commit + tag only. No changelog edit, no test
     run.
   - `simit release` — version bump + cargo test + cargo clippy + CHANGELOG edit + git
     commit + tag.

   **Default to `simit release`** because the rs-modde release flow is meant to gate on
   tests and produce a changelog entry. Anyone who wants the bare commit-and-tag form
   can call `simit commit` directly from the devShell — xtask doesn't need to expose
   both. Record this in CONTRIBUTING.md (Phase 06).
3. **Rewrite the xtask CLI arm** in [crates/modde-xtask/src/main.rs:44-49 and 177-184](../../crates/modde-xtask/src/main.rs#L44-L49):
   - Change the `Cmd::Release` definition to take a `bump: BumpArg` enum (mirroring
     simit's `BumpKind`: `Patch | Minor | Major | Prerelease`), an `--message` (`-m`)
     required string, and the existing `--dry-run` flag.
   - Add an optional `--pre <ID>` flag to mirror simit.
   - Drop the `version: Version` positional — simit owns version arithmetic now. Do
     not try to translate a target version back into a bump kind; that's brittle and
     unnecessary.
4. **Replace the body** of the `Cmd::Release { bump, message, pre, dry_run }` match arm
   with a `std::process::Command::new("simit")` invocation:
   ```rust
   let mut cmd = std::process::Command::new("simit");
   cmd.current_dir(&cfg.workspace_root)
      .arg("release")
      .arg(match bump {
          BumpArg::Patch => "patch",
          BumpArg::Minor => "minor",
          BumpArg::Major => "major",
          BumpArg::Prerelease => "prerelease",
      })
      .args(["-m", &message]);
   if let Some(pre) = pre {
       cmd.args(["--pre", &pre]);
   }
   if dry_run {
       cmd.arg("--dry-run");
   }
   let status = cmd.status().context("running simit release")?;
   if !status.success() {
       anyhow::bail!("simit release failed: {status}");
   }
   Ok(())
   ```
5. **Drop the `run_release` import** at the top of [main.rs:5-10](../../crates/modde-xtask/src/main.rs#L5-L10). Drop `ReleaseMode` too. Keep every other `harbor_xtask` symbol — `run_check`, `run_copr_*`, `run_coverage`, `run_nix_build`, etc. are still in use.
6. **Delete `release.toml`** at the workspace root. Do this in the same commit as the xtask edit so a `git bisect` always sees both ends of the swap atomically — there is no coherent intermediate state where xtask points at simit but cargo-release config still exists, and vice versa.
7. **Integration test, devShell-side**:
   ```
   nix develop --command cargo build -p modde-xtask
   nix develop --command cargo xtask release --help
   nix develop --command cargo xtask release patch --dry-run -m "validation"
   ```
   The third command must print simit's dry-run output (starts with `simit release dry-run`). Capture both stdout and the exit status — exit 0 is the gate.
8. **Reset workspace state** after the dry-run: `git status` must be clean. simit's
   dry-run is read-only (verified in Phase 03) but the xtask wrapper shouldn't be the
   thing that breaks that invariant.
9. **Update CI**: `release.yml` does not call `cargo xtask release` (only `cargo xtask
   copr ...`), so no workflow edit is required *for this phase*. Phase 05 will add a CI
   smoke job that exercises `cargo xtask release patch --dry-run`.
10. **Commit**:
    ```
    git add crates/modde-xtask/src/main.rs
    git rm release.toml
    git commit -m 'feat(xtask): drive releases through simit, drop cargo-release config'
    ```

## Acceptance criteria

- [ ] `crates/modde-xtask/src/main.rs` no longer imports `run_release` or `ReleaseMode` from `harbor_xtask`.
- [ ] `Cmd::Release` takes a `bump: BumpArg` value-enum and a required `--message`/`-m`, not a `version: Version`.
- [ ] `Cmd::Release` execution spawns the `simit` binary (`std::process::Command::new("simit")`) — verify with `grep -n 'simit' crates/modde-xtask/src/main.rs`.
- [ ] `release.toml` is deleted (`test ! -e release.toml`).
- [ ] `nix develop --command cargo build -p modde-xtask` succeeds with no warnings beyond the workspace baseline.
- [ ] `nix develop --command cargo xtask release --help` lists `patch|minor|major|prerelease` as the bump positional and shows `--dry-run`, `-m`, `--pre`.
- [ ] `nix develop --command cargo xtask release patch --dry-run -m "validation"` exits 0 and the first line of stdout is `simit release dry-run`.
- [ ] After the dry-run, `git status --porcelain` is empty.
- [ ] `nix flake check --keep-going --print-build-logs` exits 0.
- [ ] `rg cargo-release crates/modde-xtask Cargo.toml release.toml 2>/dev/null` returns no matches (the devShell still references it; that's Phase 06).
- [ ] Commit message is `feat(xtask): drive releases through simit, drop cargo-release config`.

## Files likely touched

- [crates/modde-xtask/src/main.rs](../../crates/modde-xtask/src/main.rs) — Cmd::Release arm, imports, new BumpArg enum.
- [crates/modde-xtask/Cargo.toml](../../crates/modde-xtask/Cargo.toml) — possibly drop the `semver` dependency if it's only used for the `Version` arg that's going away. Confirm by `cargo build` after removing.
- [release.toml](../../release.toml) — deleted.

## Pitfalls

- **`simit` may not be on `$PATH` in CI** until Phase 02 lands. The build commands in step 7 assume `nix develop` provides it. If you start this phase before Phase 02 is merged, the dry-run in step 7 will fail with "simit: not found". Reorder: don't start until Phase 02 is on `trunk`.
- **harbor_xtask's `run_release` may still be needed**. If it is, leaving the import would be a warning, not an error; removing it is correct *but* run `cargo check -p modde-xtask` to be sure nothing else in main.rs uses the symbol.
- **Hidden cargo-release config references**: `cargo release` reads `[package.metadata.release]` in any `Cargo.toml` in addition to `release.toml`. Sweep with `rg 'metadata.release|metadata\.release' Cargo.toml crates/*/Cargo.toml` — leave any matches alone (they're per-package overrides and removing them would conflate concerns; Phase 06 can audit later).
- **Tag clash on dry-run**: simit's release-preflight checks that the target tag doesn't already exist. If `0.1.1` ever got created by a prior experiment, the dry-run will bail with "tag exists". Resolve by picking an unused bump kind for the validation call or deleting the orphan tag (`git tag -d 0.1.1` — local only, do not push the deletion).
- **detritus path-dep breaks `cargo metadata`**: simit calls `cargo metadata` first thing. If [Cargo.toml:46-47](../../Cargo.toml#L46-L47) points at an unreachable path, simit dies before doing anything useful. This is a pre-existing trip-wire; the dry-run gate in step 7 surfaces it. Resolution belongs to the operator; if it blocks, file under "rs-modde dev setup" not "this phase".
- **CLI flag breakage for callers**: the old `cargo xtask release 0.2.0` invocation now errors with "invalid value for bump". Grep the repo for that form before committing: `rg 'xtask release [0-9]+\.[0-9]+\.[0-9]+'` should return no matches.

## Reference

- simit CLI contract: [simit/src/cli.rs:33-103](../../../../simit/src/cli.rs#L33-L103)
- simit release pipeline: [simit/src/commands/release.rs](../../../../simit/src/commands/release.rs)
- Current xtask Release arm: [crates/modde-xtask/src/main.rs:44-49 and 177-184](../../crates/modde-xtask/src/main.rs#L44-L49)
- harbor_xtask release helper (no longer called from rs-modde after this phase): `/data/nvme0/can/Projects/rs-harbor/crates/harbor-xtask/src/release.rs:41`
- Prior cargo-release-only decision (now reversed): [docs/planning/xtask-tooling-cli-uplift/DECISION.md](../xtask-tooling-cli-uplift/DECISION.md)
- Phase 02 (devShell): [02-add-simit-to-devshell.md](./02-add-simit-to-devshell.md)
- Phase 03 (CHANGELOG): [03-reshape-changelog-for-simit.md](./03-reshape-changelog-for-simit.md)
