# Tool & Installer Support

## Overview

modde supports multiple mod installation formats (FOMOD, Wabbajack, Nexus Collections, manual) and can run external tools with overwrite capture. The truthful status today is mixed: FOMOD and the Nexus/Wabbajack install paths are real, the tools view is now DB-backed and can apply/revert tracked patches, but BAIN is still partial and MO2-style executable management does not exist yet.

## Installer Formats

### FOMOD

**Key files:** `crates/modde-games/src/bethesda/fomod.rs`, `crates/modde-cli/src/commands/fomod.rs`

Uses the `fomod-oxide` library for full FOMOD support:

| CLI Command | Purpose |
|-------------|---------|
| `fomod generate <mod_path> [--all] [--format toml\|json\|nix]` | Generate declarative config template |
| `fomod apply <mod_path> --config C --dest D` | Apply config non-interactively |
| `fomod inspect <mod_path>` | Display FOMOD structure |

Features:
- Interactive wizard (Iced UI) with step-by-step navigation
- Declarative configs for reproducible installs (TOML/JSON/Nix)
- Conflict detection between FOMOD selections
- Stored config in `EnabledMod.fomod_config` for re-application during deploy

### Wabbajack

**Key file:** `crates/modde-sources/src/wabbajack/`

Full Wabbajack modlist installer:
- Manifest parsing (`WabbajackManifest`, `ArchiveEntry`, `Directives`)
- Download directives: Nexus, GitHub, Google Drive, Mega, Direct
- First-class user install flow today: Nexus, Wabbajack, Nexus Collections
- Backend-only for most users today: GitHub, Google Drive, Mega, Direct outside Wabbajack/directive installs
- Install directives: `FromArchive`, `CreateDirectory`, `InlineFile`, `PatchedFromArchive`, `CreateBSA`
- BSA repacking for Bethesda games
- Binary patching support
- Concurrent downloads (default 4, configurable)

CLI: `modde install wabbajack <path> [--profile P] [--game-dir D] [--force]`

### Nexus Collections

Two-step install flow with auto-discovery:

CLI: `modde install nexus-collection <slug> [--version V] [--profile P]`

### Manual Mod Installation

CLI: `modde install mod <url> [--profile P] [--fomod-config C]`

## External Tool Launcher

### Tool Execution with Overwrite Capture

**Key file:** `crates/modde-cli/src/commands/tool.rs`

```
modde tool run <executable> [--profile P] [--game G] [-- args...]
```

Workflow:
1. Snapshots game mod directory (records all file paths)
2. Runs the tool with game install dir as CWD
3. Diffs to find new files written by the tool
4. Moves new files to `__overwrite__` mod in the content store

### Tool Detection

```
modde tool list --game <id>
```

Scans install directory for: xEdit, FNIS, Nemesis, BodySlide, Creation Kit, LOOT, zEdit.

## Game Detection

**Key file:** `crates/modde-games/src/detection.rs`

| Source | Detection Method |
|--------|-----------------|
| Steam | Parses `libraryfolders.vdf`, scans `common/` for known app IDs |
| Heroic GOG | Parses `gog_store/installed.json` |
| Heroic Epic | Parses `store/installed.json` |
| Heroic Sideload | Matches directory names in sideloaded installs |

### Supported Games

| Game | ID | Launcher Support |
|------|-----|-----------------|
| Skyrim SE | `skyrim-se` | Steam, Heroic GOG |
| Skyrim AE | `skyrim-ae` | Steam, Heroic GOG |
| Fallout 4 | `fallout4` | Steam, Heroic GOG |
| Fallout 76 | `fallout76` | Steam |
| Cyberpunk 2077 | `cyberpunk2077` | Steam, Heroic GOG, Heroic Epic |

### Wine/Proton Integration

- Proxy DLL detection: `version.dll`, `winmm.dll`, `dinput8.dll`, `d3d11.dll`, etc.
- Automatic `WINEDLLOVERRIDES` configuration
- Proton prefix path resolution for saves, plugins.txt, INI files
- Launch wrapper generation with proper Wine environment
- fgmod DLL auto-restoration in launch wrapper

### Play Command

**Key file:** `crates/modde-cli/src/commands/play.rs`

```
modde play [profile] --game <id> [--no-deploy] [--no-switch] [--no-capture]
```

Full pipeline: switch profile -> deploy mods -> detect launcher -> launch game -> auto-capture saves.

## Stock Game Snapshots

Vanilla game preservation:
- `modde stock snapshot <game_id>` — create hardlink backup with tree hash
- `modde stock verify <game_id>` — verify snapshot integrity
- xxh3-64 deterministic tree hashing over sorted file hashes

## MO2 Feature Comparison

| MO2 Feature | modde | Notes |
|-------------|-------|-------|
| FOMOD installer | **Done** | Interactive + declarative configs |
| BAIN installer | Partial | Detection/execution exists, but the required user-selection flow is not finished |
| OMOD installer | -- | Legacy Oblivion format |
| NCC installer | -- | Legacy NMM format |
| Bundle installer | -- | MO2-specific multi-archive |
| Manual install with drag-and-drop | Partial | CLI manual install, no DnD |
| Reinstall mod from original archive | -- | MO2 re-runs installer from stored archive |
| Tool launcher through VFS | **Done** | Symlinks globally visible |
| Overwrite folder | **Done** | `__overwrite__` mod via diff |
| Per-executable output mod | -- | MO2 can direct tool output to a specific named mod |
| Executables management dialog | -- | MO2 has full executable config: args, working dir, Steam overlay, output mod |
| Forced DLL injection per executable | -- | MO2 can inject DLLs into launched processes |
| Desktop/Start Menu shortcut creation | -- | MO2 creates shortcuts that launch through VFS |
| Wabbajack compatibility | **Done** | Full manifest parser |
| Game auto-detection | **Done** | Steam + Heroic multi-source |
| Wine/Proton support | **Done** | Native Linux, proxy DLL handling |
| Stock game preservation | **Done** | Hardlink snapshots with tree hash |
| Mod information dialog (9 tabs) | -- | MO2 has: TextFiles, INI, Images, Optional ESPs, Conflicts, Categories, Nexus, Notes, Filetree |
| In-mod text/INI file editor | -- | MO2 edits .txt/.ini files inside mods in-place |
| In-mod image browser | -- | MO2 shows thumbnails + preview for images inside mods |
| File preview system (DDS, images, text) | -- | MO2 has plugin-based file preview |
| 50+ supported games | -- | modde is intentionally narrower today |
| Generic game support | Not shipped | `GenericGame` exists as an internal helper, not as a polished user-facing feature |
| Instance management (portable/global) | Partial | Active data-root switching is real now, but the portable/global UX is still far from MO2 |
| Offline mode | -- | MO2 can disable all network access |
| Tutorial/first-run guidance | -- | MO2 has overlay hints on first use |
| Theming | **Done** | 6 themes: Dark, Light, Dracula, Nord, Gruvbox, Catppuccin |
| Log panel | -- | MO2 has dockable log panel with configurable verbosity |
