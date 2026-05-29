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

# Regenerate the committed Nix tool schema
just export-tool-schema

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
- `nix/tool-schema.nix` is generated via `just export-tool-schema`; do not hand-edit it

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

Release tags are bare semver names such as `0.2.0` or `1.0.0-rc.1`.
See [docs/copr-release.md](docs/copr-release.md) for the Fedora COPR wiring (one-time setup + per-tag flow).

### Release tooling

- Run releases through `cargo xtask release {patch|minor|major|prerelease} -m "<message>"`.
- The xtask wrapper delegates to `simit release` from the devShell. For a new minor release candidate, use `cargo xtask release minor --pre rc.1 -m "Release 1.0.0-rc.1"`; for the next RC on an already-prerelease version, use `cargo xtask release prerelease --pre rc.2 -m "Release 1.0.0-rc.2"`.
- Tags are bare semver with no `v` prefix; `.forgejo/workflows/release.yml` triggers on `[0-9]*`.
- Stable tags must match `X.Y.Z`. Prerelease tags must match `X.Y.Z-rc.N`, `X.Y.Z-beta.N`, or `X.Y.Z-alpha.N`.
- Every tag must have a matching `## [X.Y.Z] - YYYY-MM-DD` or `## [X.Y.Z-rc.N] - YYYY-MM-DD` heading in `CHANGELOG.md` before CI will build.
- Prerelease tags run the full build, signing, smoke, Codeberg release, Attic push, and COPR upload, but Codeberg marks them as prereleases. Stable-only channels are skipped: crates.io, Homebrew, AUR `modde-bin`, winget, Scoop, and Flathub.
- COPR prereleases publish to `caniko/rs-modde-testing`; create that COPR project before the first RC tag.
- Keep the `## [Unreleased]` heading in `CHANGELOG.md` exactly as-is so simit can update it.
- rs-modde does not run `simit init-ci --check` or `simit init-flake --check`.
- Those checks would treat this repo's bespoke `atlas` workflows and rs-harbor-driven flake as drift.
- The rationale, revisit conditions, and other non-obvious choices live in [docs/architecture-decisions.md](docs/architecture-decisions.md).

### Hotfix release

Hotfixes ship from the last good release tag, not from `trunk`, when `trunk`
contains unrelated or unfinished work.

```sh
git fetch --tags origin
git switch -c hotfix/0.2.0 0.2.0
git cherry-pick <fix-commit>
cargo xtask check
cargo xtask release patch -m "Release 0.2.1"
git push origin hotfix/0.2.0 0.2.1
```

For a hotfix on a prerelease line, use `cargo xtask release prerelease --pre
rc.2 -m "Release 0.3.0-rc.2"`. No back-merge from `trunk` is required to
publish a hotfix. After the release is out, either cherry-pick the fix back to
`trunk` or open a follow-up PR that explains why the fix is hotfix-only.

### Release announcements

Stable tags announce after the Codeberg release is created. Prerelease tags do
not announce. Configure these Forgejo secrets to enable announcements:

- `MASTODON_TOKEN` and `MASTODON_BASE_URL`, for example `https://fosstodon.org`
- `MATRIX_TOKEN`, `MATRIX_HOMESERVER`, and `MATRIX_ROOM`

When any of those secrets are absent, CI logs a skip and keeps the release
running. The announcement body includes the Codeberg release URL and the first
five lines from the matching `CHANGELOG.md` section.

### Yank / withdraw a release

Use this drill when a published stable release must be pulled. Announce the
withdrawal first if users may already have downloaded artifacts.

```sh
VERSION=0.2.1
```

- crates.io: `cargo yank --version "$VERSION" -p modde-cli`; repeat for every published crate at that version.
- Codeberg release: delete or mark the release draft-only from the Codeberg UI, then delete the tag only if the tag itself is invalid: `git push origin ":refs/tags/${VERSION}"`.
- Homebrew tap: in `caniko/homebrew-modde`, revert the formula bump commit with `git revert <commit> && git push`.
- Debian / Ubuntu apt: publish a corrected stable release to `https://modde.rs/apt/`, or temporarily remove the affected `.deb` entries from the served APT tree and force-push the rebuilt `pages` branch.
- AUR: revert the affected package repo commit and push. Use `git push --force-with-lease` only if the bad commit must disappear from the AUR history.
- winget: comment `Withdrawn: modde ${VERSION}` on the generated PR and close it. If merged, open a removal/revert PR in `microsoft/winget-pkgs`.
- Scoop: revert the bucket manifest bump in `caniko/scoop-modde` and push.
- Flathub: close the release PR if unmerged. If merged, open a `revert/${VERSION}` PR against `flathub/com.tartanoglu.modde`.
- COPR: find the build id with `copr-cli list-builds caniko/rs-modde --output-format json`, then run `copr-cli delete-build <build-id>`. Use `caniko/rs-modde-testing` for prerelease builds.

After rollback, add a `CHANGELOG.md` note under `Unreleased` that names the
withdrawn version and points to the fixed follow-up release.

## License

By contributing, you agree that your contributions will be licensed under GPL-3.0-only.
