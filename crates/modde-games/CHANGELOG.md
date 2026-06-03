# modde-games Changelog

This crate is released as part of the `modde` workspace. The workspace
changelog is the authoritative release history.

## [Unreleased]

## [0.3.0] - 2026-06-03

### Added

- Release workflow changes are tracked in the workspace changelog.

### Changed

- Launcher integration was updated for the async database backend.

## [0.2.0] - 2026-05-19

### Added

- Wabbajack `GameFileSourceDownloader` support for local game-file verification.
- Home Manager Wabbajack profile support for declarative modlist install and deploy flows.
- Non-fatal Home Manager waiting for missing game installs through `installMode = "await-game"`.
- Wabbajack game-file-source and deploy-pipeline regression coverage.

## [0.1.0] - 2026-04-13

### Added

- Bethesda game support for Skyrim SE/AE, Fallout 4, Fallout 76, and Starfield.
- Cyberpunk 2077 support for REDmod, CET, TweakXL, REDscript, conflicts, and saves.
- Stellar Blade support.
- Steam and Heroic launcher auto-detection.
