# Skyrim Integration Issue #1 Northstar Plan

Issue: https://codeberg.org/caniko/rs-modde/issues/1

## Summary

This plan tracks the end-to-end Skyrim SE early-adopter path exposed by the
Legends of the Frost report: Wabbajack manifests with local game-file sources,
Home Manager declarative installation, `--game-dir` plumbing, CI-safe
regressions, and corrected user guidance.

Follow-up: the Home Manager flow also needs a non-failing awaiting state so
users can declare a Wabbajack profile before Skyrim is installed. modde waits
for Steam/Heroic-managed game installs, then installs/deploys once `gameDir`
points at the real game directory.

Existing dirty-tree work to preserve:

- `GameFileSourceDownloader` manifest parsing.
- `WabbajackInstaller::set_game_dir` and local game-file-source reads.
- Nix sandbox fix for `test_deploy_pipeline_end_to_end`.
- Optional ignored real LOTF regression test using `MODDE_REAL_LOTF_WABBAJACK`.

## Phase 1: Wabbajack Core Correctness

Parallel ownership:

- Manifest parsing: keep `GameFileSourceDownloader` support, ensure game-file
  archives are not download directives, and keep a compact parser regression
  for the exact Wabbajack `$type`.
- Installer behavior: validate game-file paths under `game_dir`, reject
  traversal and symlink sources, verify Wabbajack xxh64, and read game-file
  sources through the same directive flow used for downloaded archives.
- Regression coverage: keep CI-safe synthetic tests for missing `--game-dir`,
  missing files, hash mismatch, successful copies, and case-insensitive source
  lookup.

Acceptance:

- `modde install wabbajack <lotf.wabbajack> --profile lotf --game-dir <SkyrimSE>`
  no longer fails on `GameFileSourceDownloader`.
- CI does not require a real LOTF file or local Skyrim installation.
- The ignored real LOTF test remains available for manual smoke testing.

## Phase 2: Home Manager Declarative Install

Parallel ownership:

- Module options: add `profiles.<name>.gameDir` and
  `profiles.<name>.installMode`, preserve `wabbajackList`, and reject profiles
  that set both `wabbajackList` and `nexusCollection`.
- Activation flow: fetch the `.wabbajack` file with Nix, install missing
  Wabbajack profiles before deploy, pass `--game-dir` when configured, enter an
  awaiting state when the game is not installed, and keep manual profiles
  deploy-only.
- CLI support: rely on existing `profile lock-info` as the read-only profile
  existence probe unless a stronger inspection command becomes necessary.

Acceptance:

- The issue's Home Manager shape works with real `url`, `hash`, and `gameDir`.
- A clean user profile installs before deployment.
- Repeated activation skips reinstall and still deploys.
- A profile can be declared before Skyrim exists; activation prints the next
  action and exits successfully until `gameDir` is valid.

## Phase 3: Documentation And Guidance

Parallel ownership:

- Wabbajack guide: use `modde install wabbajack`, document `--game-dir`, and
  explain authored-files URLs from Wabbajack registry pages.
- Home Manager docs: show `gameDir`, clarify the Nix fetch hash, and update
  README, quick start, and module reference examples.
- Troubleshooting: cover missing `--game-dir`, game-file hash mismatches,
  Wabbajack URL/hash mismatch, and missing Nexus API keys.

Acceptance:

- Public examples use valid CLI syntax.
- A Skyrim SE Wabbajack user can reproduce the Home Manager flow without
  guessing where local game paths belong.

## Phase 4: Nix And CI Hardening

Parallel ownership:

- Nix sandbox: keep the tempdir materialization regression fix.
- Module validation: keep a flake check proving Wabbajack profiles render an
  install step, manual profiles render deploy-only, `gameDir` is passed, and
  mutual exclusion is asserted.
- Rust validation: run targeted Wabbajack/deploy tests, then workspace tests,
  format, and clippy.

Acceptance:

- The NixOS check failure from issue #1 is fixed.
- Workspace tests pass in the Nix dev shell.
- Tests do not require network access or a local Skyrim install.

## Phase 5: Issue Closeout

Parallel ownership:

- Changelog: mention LOTF/Wabbajack game-file sources, HM Wabbajack install,
  and the Nix sandbox test fix.
- Maintainer response: prepare a concise Codeberg reply with the corrected
  Home Manager example, manual command, `gameDir` note, parser fix, and
  `checkPhase` status.

Acceptance:

- Issue #1 has a concrete response path.
- This document remains the durable northstar for future Skyrim integration
  follow-up.
