# modde-ui Changelog

This crate is released as part of the `modde` workspace. The workspace
changelog is the authoritative release history.

## [Unreleased]

### Added

- Async profile and tool write operations backed by the shared database handle.

### Changed

- Profile, tool, and settings writes now run off the iced render thread.
- Release workflow changes are tracked in the workspace changelog.

## [0.2.0] - 2026-05-19

### Added

- Wabbajack `GameFileSourceDownloader` support for local game-file verification.
- Home Manager Wabbajack profile support for declarative modlist install and deploy flows.
- Non-fatal Home Manager waiting for missing game installs through `installMode = "await-game"`.
- Wabbajack game-file-source and deploy-pipeline regression coverage.

## [0.1.0] - 2026-04-13

### Added

- Iced GUI with mod list, downloads, saves, settings, FOMOD wizard, Nexus browser, diagnostics, and tools views.
- Reachable advanced views for Downloads, Data Files, Diagnostics, and Tools.
