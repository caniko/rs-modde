# modde-ui Changelog

This crate is released as part of the `modde` workspace. The workspace
changelog is the authoritative release history.

## [Unreleased]

### Added

- Homebrew tap publication support is tracked at the workspace release level.

### Changed

- Release CI Homebrew tap generation is tracked at the workspace release level.

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
