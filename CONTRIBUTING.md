# Contributing to modde

Thank you for your interest in contributing to modde!

## Getting Started

### Prerequisites

- [Nix](https://nixos.org/) with flakes enabled (recommended)
- Alternatively: Rust 2024 edition toolchain, SQLite, and system dependencies for Iced

### Development Setup

```sh
# Clone the repository
git clone https://codeberg.org/caniko/rs-modde.git
cd rs-modde

# Enter the dev shell (provides all dependencies)
nix develop

# Project-local cargo alias: `cargo xtask ...` runs `crates/modde-xtask`.
# It depends on the flake-pinned `harbor-xtask` git crate, so a fresh
# checkout may fetch rs-harbor.

# Build
cargo build --workspace

# Run tests
cargo xtask test

# Run clippy
cargo xtask lint

# Format check
cargo xtask check

# Run the GUI directly with development logging
cargo xtask gui

# Override GUI logging when needed
RUST_LOG=modde_ui=trace,modde_core=debug cargo xtask gui
```

### Project Structure

```
crates/
  modde-core/     Core types, database, VFS, profiles, collision detection
  modde-sources/  Download backends (Nexus, Wabbajack, GitHub, MEGA, etc.)
  modde-games/    Game plugins, launcher detection, mod scanners
  modde-cli/      CLI interface (24 commands, 60+ subcommands)
  modde-ui/       GUI built with Iced
```

## Making Changes

1. Fork the repository on Codeberg
2. Create a feature branch from `trunk`
3. Make your changes
4. Ensure `cargo xtask check` passes
5. Submit a pull request

### Code Style

- Follow existing patterns in the codebase
- Workspace-wide clippy pedantic warnings are enforced
- Use `thiserror` for error types, `anyhow` for CLI/application errors
- Prefer small, focused commits with clear messages

### Adding Game Support

Game plugins implement traits in `crates/modde-games/src/traits.rs`:
- `GamePlugin` for game detection, paths, and configuration
- `ModScanner` for filesystem-based mod discovery
- `SaveTracker` for save file management

See `crates/modde-games/src/cyberpunk/` for a complete example.

## Reporting Issues

Please file issues on the [Codeberg issue tracker](https://codeberg.org/caniko/rs-modde/issues).

Include:
- modde version (`modde --version`)
- Operating system and distribution
- Steps to reproduce
- Expected vs actual behavior

## Releases

See [docs/copr-release.md](docs/copr-release.md) for the Fedora COPR wiring (one-time setup + per-tag flow).

## License

By contributing, you agree that your contributions will be licensed under GPL-3.0-only.
