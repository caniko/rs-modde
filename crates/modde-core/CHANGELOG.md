# modde-core Changelog

This crate is released as part of the `modde` workspace. The workspace
changelog is the authoritative release history.

## [Unreleased]

### Added

- Async SQLx-backed database layer with SQLite by default and optional
  PostgreSQL support.
- Database migrations and PostgreSQL parity coverage for profile, mod, save,
  settings, tool, and deployment flows.

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

- SQLite-backed profiles and mod database.
- VFS symlink-farm deployment, rollback, and conflict detection.
- Profile management, load-order locking, save management, and diagnostics.
