# modde

A NixOS-native game mod manager written in Rust. Provides declarative, reproducible mod management with virtual filesystem deployment, profile management, save versioning, and conflict detection.

## Supported games

| Game | Features |
|------|----------|
| Skyrim SE/AE | Plugins, VFS, LOOT sorting, save tracking |
| Fallout 4 | Plugins, VFS, LOOT sorting, save tracking |
| Fallout 76 | Plugins, VFS |
| Starfield | Plugins, VFS, save tracking |
| Cyberpunk 2077 | REDmod, CET, TweakXL, scripts, conflict detection |
| Stellar Blade | UE4 framework (experimental) |

Games are auto-detected via Steam (Proton) and Heroic (GOG, Epic) launchers.

## Features

- **Virtual filesystem deployment**: Symlink farm keeps the game directory clean and unmodified; atomic rollback to previous deployments
- **Wabbajack on Linux**: Native parsing of `.wabbajack` modlist archives without a Windows VM
- **Profile management**: Create, fork, switch, and delete profiles; stackable experiments with rollback (like git branches); load order locking
- **Save management**: Git-backed save vaults with SHA-256 fingerprinting, compatibility warnings, auto-capture on game exit
- **Conflict detection**: Graph-based collision analysis with classification (dangerous vs cosmetic) and resolution suggestions
- **Mod sources**: Nexus Mods API, Wabbajack modlists, Nexus Collections, GitHub releases, direct URLs, Google Drive, MEGA
- **FOMOD support**: Parse, generate declarative configs, and apply non-interactively
- **Gaming tools**: MangoHud, vkBasalt, GameMode, ReShade, OptiScaler integration
- **Diagnostics**: Form 43 errors, missing masters, shadowed mods, dangerous collisions

## Architecture

| Crate | Purpose |
|-------|---------|
| `modde-core` | SQLite database, VFS/symlink farm, profiles, collision detection, save management, load order resolver |
| `modde-games` | Game plugins (Bethesda, Cyberpunk, Stellar Blade), trait system, launcher detection, overlay tools |
| `modde-sources` | Download backends (Nexus, Wabbajack, GitHub, MEGA, etc.), archive extraction, FOMOD installer |
| `modde-cli` | ~20 subcommands: play, deploy, install, scan, collisions, saves, diagnostics, LOOT sorting |
| `modde-ui` | GUI built with Iced |

## Usage

```bash
# Detect installed games
modde detect

# Install a Wabbajack modlist
modde install wabbajack /path/to/modlist.wabbajack --game skyrim-se

# Deploy mods and play
modde play my-skyrim --game skyrim-se

# Manage saves
modde saves capture --game skyrim-se
modde saves history --game skyrim-se

# Analyze conflicts
modde collisions --profile my-skyrim --game skyrim-se

# Check for updates
modde update check --profile my-skyrim --game skyrim-se --period 1w
```

## Installation

```bash
# Via Nix
nix run .#modde

# Via Cargo
cargo install --path crates/modde-cli

# Development
nix develop
cargo build --release
```

A home-manager module is available for declarative configuration.

## CI

Woodpecker CI on Codeberg runs `cargo build`, `cargo test`, `cargo clippy`, and `cargo fmt --check` on every push and pull request to catch build and lint regressions.

## License

GPL-3.0-only
