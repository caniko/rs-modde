# modde-sources Changelog

This crate is released as part of the `modde` workspace. The workspace
changelog is the authoritative release history.

## [Unreleased]

### Added

- Release workflow changes are tracked in the workspace changelog.

### Changed

- Wabbajack runner integration was updated for the async database backend.

## [0.2.0] - 2026-05-19

### Added

- Wabbajack `GameFileSourceDownloader` support for local game-file verification.
- Home Manager Wabbajack profile support for declarative modlist install and deploy flows.
- Non-fatal Home Manager waiting for missing game installs through `installMode = "await-game"`.
- Wabbajack game-file-source and deploy-pipeline regression coverage.

## [0.1.0] - 2026-04-13

### Added

- Nexus Mods, Wabbajack, Direct URL, GitHub releases, MEGA, and Google Drive source backends.
- Native Wabbajack modlist parsing and installation.
- FOMOD integration and BAIN installer support.
- `nxm://` protocol handling support.
