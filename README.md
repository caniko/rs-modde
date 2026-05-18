# modde

A NixOS-native game mod manager written in Rust. Provides declarative, reproducible mod management with virtual filesystem deployment, profile management, save versioning, and conflict detection.

Project site: <https://caniko.codeberg.page/rs-modde/>
Documentation: <https://caniko.codeberg.page/rs-modde/docs/>

## Supported games

| Game | Current status |
|------|----------------|
| Skyrim SE/AE | `Done`: plugins, VFS, LOOT sorting, diagnostics, save tracking |
| Fallout 4 | `Done`: plugins, VFS, LOOT sorting, diagnostics, save tracking |
| Fallout 76 | `Partial`: plugins, VFS, BA2 scanning; saves are effectively server-side |
| Starfield | `Partial`: plugins, VFS, diagnostics, save tracking |
| Cyberpunk 2077 | `Done`: REDmod, CET, TweakXL, scripts, conflict detection |
| Stellar Blade | `Partial`: UE4/UE5-style deployment, scanning, conflicts, save tracking |

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
- **Gaming tools**: MangoHud, vkBasalt, GameMode, ReShade, OptiScaler, and Proton configs/patching are wired into the UI, but MO2-style executable management is still missing
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
modde save capture --game skyrim-se --profile my-skyrim
modde save history --game skyrim-se --profile my-skyrim
modde save watch --game skyrim-se  # auto-capture on changes

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
| Windows | Experimental (release artifacts are built but untested) |

## CI

Forgejo Actions on Codeberg runs `cargo fmt --check`, `cargo clippy`,
coverage tests, Rust release builds, Nix package builds, and `nix flake check`
on every push and pull request. Pushes to `trunk` also build and deploy the
documentation site and presentation website to Codeberg Pages.

Tag releases are managed with `cargo-release`. Run `just release-dry X.Y.Z` to
preview a workspace release, then `just release X.Y.Z` to publish all workspace
crates to crates.io and push the `vX.Y.Z` tag. Release tags build and publish
Linux and Windows CLI/GUI artifacts; macOS artifacts are intentionally not
shipped yet.

## Website and docs

The presentation site lives in `website/`; the documentation site lives in `docs/site/`.

```bash
nix build .#website
nix build .#docs
nix build .#site
```

For local editing:

```bash
cd website && zola serve
cd docs/site && zola serve
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## Security

See [SECURITY.md](SECURITY.md) for the security policy.

## License

GPL-3.0-only
