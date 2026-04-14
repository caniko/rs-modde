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
- **Mod sources**: Nexus Mods API (with `nxm://` protocol handler), Wabbajack modlists, Nexus Collections, GitHub releases, direct URLs, Google Drive, MEGA
- **Installers**: FOMOD (parse, generate declarative configs, apply non-interactively) and BAIN
- **Gaming tools**: MangoHud, vkBasalt, GameMode, ReShade, OptiScaler integration
- **Import/Export**: TOML profile import, CSV export with configurable columns
- **Diagnostics**: Form 43 errors, missing masters, shadowed mods, dangerous collisions

## Architecture

| Crate | Purpose |
|-------|---------|
| `modde-core` | SQLite database, VFS/symlink farm, profiles, collision detection, save management, load order resolver |
| `modde-games` | Game plugins (Bethesda, Cyberpunk, Stellar Blade), trait system, launcher detection, overlay tools |
| `modde-sources` | Download backends (Nexus, Wabbajack, GitHub, MEGA, etc.), archive extraction, FOMOD and BAIN installers |
| `modde-cli` | 24 top-level commands with 60+ subcommands covering the full modding workflow |
| `modde-ui` | GUI built with Iced |

## Usage

```bash
# Detect installed games
modde detect

# Install a Wabbajack modlist
modde install wabbajack /path/to/modlist.wabbajack --game skyrim-se

# Deploy mods and play
modde play my-skyrim --game skyrim-se

# Manage profiles
modde profile fork my-skyrim experimental-build --game skyrim-se
modde profile try experimental-build --game skyrim-se   # start experiment
modde profile rollback experimental-build --game skyrim-se  # revert

# Manage saves
modde saves capture --game skyrim-se
modde saves history --game skyrim-se
modde saves watch --game skyrim-se  # auto-capture on changes

# Analyze conflicts
modde collisions --profile my-skyrim --game skyrim-se

# Check for updates
modde update check --profile my-skyrim --game skyrim-se --period 1w

# Launch the GUI
modde gui
```

## Installation

### Nix (recommended)

```bash
# Run directly
nix run codeberg:caniko/rs-modde#modde

# Install to profile
nix profile install codeberg:caniko/rs-modde#modde

# Development shell
nix develop codeberg:caniko/rs-modde
```

### Cargo

```bash
cargo install modde-cli
```

Requires a Rust 2024 edition toolchain, SQLite development headers, and system libraries for Iced (see the Nix flake for the complete dependency list).

### From source

```bash
git clone https://codeberg.org/caniko/rs-modde.git
cd rs-modde
cargo build --release
# Binary at target/release/modde
```

## Home-Manager Module

A NixOS home-manager module is included for declarative mod profile configuration:

```nix
{
  programs.modde = {
    enable = true;
    profiles = {
      my-skyrim = {
        game = "skyrim-se";
        wabbajackList = {
          url = "https://example.com/modlist.wabbajack";
          hash = "sha256-...";
        };
      };
      my-cyberpunk = {
        game = "cyberpunk2077";
        nexusCollection = {
          slug = "my-collection";
          version = "1";
        };
      };
    };
  };
}
```

Add the module to your home-manager imports from the flake:

```nix
# In your flake.nix inputs:
inputs.modde.url = "codeberg:caniko/rs-modde";

# In your home-manager config:
imports = [ inputs.modde.homeManagerModules.modde ];
```

## Platform Support

| Platform | Status |
|----------|--------|
| Linux (NixOS) | Primary target, fully supported |
| Linux (other) | Supported via Cargo or Nix |
| macOS | Experimental (builds but untested) |
| Windows | Not supported |

## CI

Woodpecker CI on Codeberg runs `cargo fmt --check`, `cargo clippy`, `cargo test --workspace`, and `cargo build --release` on every push and pull request. Pushes to `main` and `rapid` also build and deploy the documentation site and presentation website to Codeberg Pages.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## Security

See [SECURITY.md](SECURITY.md) for the security policy.

## License

GPL-3.0-only
