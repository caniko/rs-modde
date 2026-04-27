# modde

A NixOS-native game mod manager written in Rust. Provides declarative, reproducible mod management with virtual filesystem deployment, profile management, save versioning, and conflict detection.

## Supported games

| Game | Current status |
|------|----------------|
| Skyrim SE/AE | `Done`: plugins, VFS, LOOT sorting, diagnostics, save tracking |
| Fallout 4 | `Done`: plugins, VFS, LOOT sorting, diagnostics, save tracking |
| Fallout 76 | `Partial`: plugins and VFS; saves are effectively server-side |
| Starfield | `Partial`: plugins and VFS; save tracking is not shipped yet |
| Cyberpunk 2077 | `Done`: REDmod, CET, TweakXL, scripts, conflict detection |
| Stellar Blade | `Partial`: UE4/UE5-style deployment and scanning |

Games are auto-detected via Steam (Proton) and Heroic (GOG, Epic) launchers.
The canonical status baseline for these claims lives in `docs/capability-matrix.toml`.

## Features

- **Virtual filesystem deployment**: Symlink farm keeps the game directory clean and unmodified; atomic rollback to previous deployments
- **Wabbajack on Linux**: Native parsing of `.wabbajack` modlist archives without a Windows VM
- **Profile management**: Create, fork, switch, and delete profiles; stackable experiments with rollback (like git branches); load order locking
- **Save management**: Git-backed save vaults with SHA-256 fingerprinting, compatibility warnings, and auto-capture for games with real save tracker support
- **Conflict detection**: Graph-based collision analysis with classification (dangerous vs cosmetic) and resolution suggestions
- **Nexus-first installs**: Nexus Mods API, `nxm://`, Browse Nexus, Wabbajack modlists, and Nexus Collections are the primary shipped install flows
- **Additional download backends**: GitHub, Direct, Google Drive, and MEGA backends exist today mainly for Wabbajack/directive installs
- **Installers**: FOMOD is shipped end to end; BAIN detection/execution exists but still requires missing user-input flow
- **Gaming tools**: MangoHud, vkBasalt, GameMode, ReShade, and OptiScaler configs/patching are wired into the UI, but MO2-style executable management is still missing
- **Diagnostics**: CLI and UI diagnostics now use real plugin order plus resolved conflicts instead of placeholder inputs
- **Reachable advanced views**: Downloads, Data Files, Diagnostics, and Tools are now connected in the UI; some remain `Partial` rather than MO2-complete

## Architecture

| Crate | Purpose |
|-------|---------|
| `modde-core` | SQLite database, VFS/symlink farm, profiles, collision detection, save management, load order resolver |
| `modde-games` | Game plugins (Bethesda, Cyberpunk, Stellar Blade), trait system, launcher detection, overlay tools |
| `modde-sources` | Download backends (Nexus, Wabbajack, GitHub, MEGA, etc.), archive extraction, FOMOD, and partial BAIN support |
| `modde-cli` | 24 top-level commands with 60+ subcommands covering the full modding workflow |
| `modde-ui` | Iced GUI with reachable Downloads, Data Files, Diagnostics, and Tools views |

## Usage

```bash
# Detect installed games
modde detect

# Install a Wabbajack modlist
modde install wabbajack /path/to/modlist.wabbajack \
  --profile my-skyrim \
  --game-dir "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition"

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
nix develop . -c cargo build --release
# Binary at target/release/modde
```

Use `nix develop . -c cargo test --workspace` for authoritative validation. A plain `cargo test --workspace` outside the Nix shell is not a reliable signal because `openssl-sys` will fail to locate OpenSSL on an unprepared host.

## Home-Manager Module

A NixOS home-manager module is included for declarative mod profile configuration:

```nix
{
  programs.modde = {
    enable = true;
    profiles = {
      my-skyrim = {
        game = "skyrim-se";
        installMode = "auto";
        gameDir = "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition";
        wabbajackList = {
          url = "https://example.com/modlist.wabbajack";
          hash = "sha256-...";
        };
      };
      my-local-skyrim = {
        game = "skyrim-se";
        gameDir = "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition";
        wabbajackList = {
          path = /nix/store/...-Legends-of-the-Frost.wabbajack;
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

Set `installMode = "await-game"` while the game is not installed yet. modde
waits for Steam/Heroic-managed game installs and does not install the base game
itself.

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
