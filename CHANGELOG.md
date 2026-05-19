# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-05-19

### Added

- **Skyrim/Wabbajack**: Support Wabbajack `GameFileSourceDownloader` entries used by Legends of the Frost, with local game-file verification through `--game-dir`.
- **Nix**: Home Manager Wabbajack profiles now fetch, install, and deploy declarative modlists, including explicit `gameDir` support.
- **Nix**: Home Manager profiles can now wait non-fatally for a game install through `installMode = "await-game"` or missing `gameDir` prerequisites.
- **Tests**: Hardened Wabbajack game-file-source regressions and the Nix sandbox-sensitive deploy pipeline test.

## [0.1.0] - 2026-04-13

### Added

- **Core**: SQLite-backed profile and mod database with VFS (symlink farm) deployment
- **Core**: Content-addressed mod store with per-file hiding and conflict detection
- **Core**: Profile management with forking, experiment mode (try/rollback/commit), and load order locking
- **Core**: Save management with Git-backed vaults, fingerprinting, and auto-capture
- **Core**: FOMOD installer integration via fomod-oxide (declarative config support)
- **Games**: Bethesda support (Skyrim SE/AE, Fallout 4, Fallout 76, Starfield) with plugins.txt, LOOT sorting, INI management, BSA/BA2 indexing
- **Games**: Cyberpunk 2077 support (REDmod, CET, TweakXL, REDscript, conflict detection, save tracking)
- **Games**: Stellar Blade support (UE4 framework, experimental)
- **Games**: Auto-detection via Steam (Proton) and Heroic (GOG/Epic) launchers
- **Games**: Mod scanning with game-specific filesystem discovery and Wabbajack manifest matching
- **Sources**: Nexus Mods API v1 client (search, trending, mod details, CDN downloads, collections)
- **Sources**: Wabbajack modlist parsing and native installation (no Windows VM required)
- **Sources**: Direct URL, GitHub releases, MEGA, and Google Drive download backends
- **Sources**: BAIN installer support
- **Sources**: nxm:// protocol handler with XDG desktop integration
- **CLI**: 24 top-level commands with 60+ subcommands covering the full modding workflow
- **GUI**: Iced-based GUI with 20+ views (mod list, downloads, saves, settings, FOMOD wizard, Nexus browser, diagnostics, tools)
- **Tools**: Integration with MangoHud, vkBasalt, GameMode, ReShade, and OptiScaler
- **Diagnostics**: Form 43 detection, missing master detection, shadowed mod detection, load order validation
- **Nix**: Flake with binary, docs, and website outputs; home-manager module for declarative configuration
