# modde-cli Changelog

This crate is released as part of the `modde` workspace. The workspace
changelog is the authoritative release history.

## [Unreleased]

## [0.3.0] - 2026-06-03

### Added

- `modde config show` and `modde config set-database` for inspecting and
  setting the database backend.
- Async database dispatch for CLI commands that read or write profile, mod,
  save, tool, and diagnostics data.

### Changed

- Release workflow changes are tracked in the workspace changelog.

## [0.2.0] - 2026-05-19

### Added

- Wabbajack `GameFileSourceDownloader` support for local game-file verification.
- Home Manager Wabbajack profile support for declarative modlist install and deploy flows.
- Non-fatal Home Manager waiting for missing game installs through `installMode = "await-game"`.
- Wabbajack game-file-source and deploy-pipeline regression coverage.

## [0.1.0] - 2026-04-13

### Added

- `modde` CLI with 24 top-level commands and 60+ subcommands.
- Nexus, Wabbajack, profile, save, deployment, diagnostics, tooling, and update commands.
- `nxm://` dispatch support.
