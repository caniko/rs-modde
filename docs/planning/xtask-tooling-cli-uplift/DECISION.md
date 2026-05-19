# harbor-xtask CLI contract decision

## Decision

This phase chooses a reusable Rust tooling library named `harbor-xtask`, housed in `rs-harbor` at `crates/harbor-xtask/`, consumed by thin per-project `xtask` binaries. The existing `rs-harbor` binary does **not** gain top-level `dev`, `coverage`, `release`, `copr`, or `docs` subcommands in v0.1.

The first consumer is `rs-modde`, through a new `crates/modde-xtask/` binary in a later phase. That binary owns all `rs-modde` bindings: `modde.spec`, the workspace crate list, the `modde` and `site` Nix package names, and the `docs/site` plus `website` Zola roots.

Release backend decision: choose **option (a), cargo-release only**, for v0.1. `harbor-xtask::release` shells to `cargo release` and owns the RPM spec `Version:` rewrite. `simit` remains a capability provider to watch, not a dependency and not an `rs-harbor` workspace member for Phase 02.

## Working tree placement

Yes: add a new reusable library crate in `rs-harbor`:

```text
/data/nvme0/can/Projects/rs-harbor/crates/harbor-xtask/
```

Rationale:

- `rs-harbor` already has the reusable-crate shape: `crates/harbor-sdk` and `crates/harbor-cache` sit beside the `cli/` binary.
- The inspected `cli/src/main.rs` is a clap dispatcher for harbor-specific domains: `audit`, `cache`, `sdk`, `stage`, and `steam-runtime`. Adding `modde` release/COPR/docs config there would couple `rs-harbor`'s user-facing binary to downstream project layouts.
- `harbor-xtask` is cross-project build tooling, not a harbor runtime feature. A library crate lets each consumer bind project paths and package names locally.

No: do not put this in `rs-modde`. That would keep the current workflows working but fail the DRY goal once bikipy and SynDB adopt the same cargo/Nix/check wrappers.

No: do not make `simit` an `rs-harbor` workspace member now. It is published independently, has its own CLI surface and release semantics, and can be reconsidered after `harbor-xtask` has two consumers.

## CLI exposure model

Chosen model: **library plus per-project thin binary**.

Users run:

```sh
cargo xtask check
cargo xtask coverage --ci --fail-under 80
cargo xtask release 0.2.0
cargo xtask copr vendor
cargo xtask docs serve docs
```

They do **not** run:

```sh
rs-harbor release 0.2.0
rs-harbor copr vendor
rs-harbor docs serve docs
```

Per-project binaries are required because at least these bindings are project-specific:

- `rs-modde`'s RPM spec path is `modde.spec`.
- `rs-modde`'s release crate set is `modde-core`, `modde-sources`, `modde-games`, `modde-ui`, and `modde-cli`.
- `rs-modde` has two Zola roots: `docs/site` for docs and `website` for the public site.
- `rs-modde` Nix package targets include `modde`, `site`, `modde-windows`, `appimage-ui`, `appimage-cli`, and `flatpak-manifest`.
- `rs-modde` COPR source tarballs come from `https://codeberg.org/caniko/rs-modde/archive/v{version}.tar.gz`.

## Public API surface

Phase 02 must implement this v0.1 API exactly unless a compile-time blocker is found and documented in that commit.

```rust
use std::path::{Path, PathBuf};

use anyhow::Result;
use semver::Version;

#[derive(Debug, Clone)]
pub struct ProjectConfig {
    pub workspace_root: PathBuf,
    pub cargo_workspace: CargoWorkspace,
    pub spec_file: Option<PathBuf>,
    pub copr: Option<CoprConfig>,
    pub docs: Vec<DocsSite>,
    pub nix_packages: Vec<NixPackage>,
}

#[derive(Debug, Clone)]
pub struct CargoWorkspace {
    pub packages: Vec<String>,
    pub all_features: bool,
}

#[derive(Debug, Clone)]
pub struct CoprConfig {
    pub source_archive_url_template: String,
    pub srpm_dir: PathBuf,
    pub vendor_tarball: PathBuf,
}

#[derive(Debug, Clone)]
pub struct DocsSite {
    pub name: String,
    pub root: PathBuf,
    pub engine: DocsEngine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocsEngine {
    Zola,
    Mdbook,
}

#[derive(Debug, Clone)]
pub struct NixPackage {
    pub name: String,
    pub flake_ref: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageMode {
    Summary,
    Html,
    Lcov,
    Ci { fail_under_lines: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseMode {
    DryRun,
    Execute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatMode {
    Check,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NixBuildOptions {
    pub impure: bool,
}

impl ProjectConfig {
    pub fn from_workspace_root(workspace_root: impl Into<PathBuf>) -> Self;
    pub fn resolve(&self, path: impl AsRef<Path>) -> PathBuf;
    pub fn docs_site(&self, name: &str) -> Result<&DocsSite>;
    pub fn nix_package(&self, name: &str) -> Result<&NixPackage>;
}

impl DocsSite {
    pub fn zola(name: impl Into<String>, root: impl Into<PathBuf>) -> Self;
    pub fn mdbook(name: impl Into<String>, root: impl Into<PathBuf>) -> Self;
}

pub fn run_check(cfg: &ProjectConfig) -> Result<()>;
pub fn run_fmt(cfg: &ProjectConfig, mode: FormatMode) -> Result<()>;
pub fn run_lint(cfg: &ProjectConfig) -> Result<()>;
pub fn run_test(cfg: &ProjectConfig, extra_args: &[String]) -> Result<()>;
pub fn run_cargo_build(cfg: &ProjectConfig, release: bool) -> Result<()>;
pub fn run_cargo_package(cfg: &ProjectConfig, package: &str, extra_args: &[String]) -> Result<()>;

pub fn run_coverage(cfg: &ProjectConfig, mode: CoverageMode) -> Result<()>;

pub fn rewrite_spec_version(spec_file: &Path, version: &Version) -> Result<()>;
pub fn run_release(cfg: &ProjectConfig, version: &Version, mode: ReleaseMode) -> Result<()>;

pub fn run_copr_vendor(cfg: &ProjectConfig) -> Result<()>;
pub fn run_copr_vendor_check(cfg: &ProjectConfig) -> Result<()>;
pub fn run_copr_srpm(cfg: &ProjectConfig, version: &Version) -> Result<()>;

pub fn run_nix_build(cfg: &ProjectConfig, package: &str, opts: NixBuildOptions) -> Result<()>;
pub fn run_nix_develop(cfg: &ProjectConfig) -> Result<()>;

pub fn run_docs_serve(cfg: &ProjectConfig, site: &str) -> Result<()>;
```

API notes:

- Use `anyhow::Result` in v0.1 to match the existing `rs-harbor` CLI style and to avoid speculative error enums before there are multiple consumers.
- `ProjectConfig` stores project-relative paths. Runner functions resolve paths against `workspace_root`.
- The library is synchronous. Shell-outs use `std::process::Command`; no async runtime is introduced.
- `run_check` is the composed workflow: `fmt --check`, `lint`, `test`, then workspace build.
- `run_cargo_package` intentionally covers `rs-modde`'s `run` and `gui` targets without adding project names to `harbor-xtask`.

## Release backend

Chosen: **(a) cargo-release only**.

`run_release(cfg, version, ReleaseMode::DryRun)`:

1. If `cfg.spec_file` is present, rewrite its first and only `Version:` line to `Version:        {version}` in a temporary copy or dry-run plan output; do not commit.
2. Run `cargo release {version} --workspace --no-confirm`.

`run_release(cfg, version, ReleaseMode::Execute)`:

1. Rewrite the real spec file with `rewrite_spec_version`.
2. Stage and commit only that spec file with `release: bump <spec-file> Version to {version}`.
3. Run `cargo release {version} --workspace --execute --no-confirm`.

The spec rewrite must fail if zero or more than one `Version:` line is found. That replaces the current GNU `sed -i` dependency with testable Rust behavior.

Why not `simit` yet:

- The inspected `simit release` command accepts bump kinds (`patch`, `minor`, `major`, `prerelease`), not an exact version argument. `rs-modde`'s current release workflow is exact-version driven.
- `simit` currently runs its own `cargo test` and `cargo clippy --all-targets --all-features -- --deny warnings`, while `rs-modde`'s justfile uses `cargo clippy --workspace -- -D warnings` and a separate release flow. Adopting it would change release semantics in the design phase.
- `simit` does not know about RPM spec files. The spec rewrite remains in `harbor-xtask` either way.

Recommendation: after Phase 02, consider adding a library-grade `simit` API that accepts an exact `Version` and separates version planning from commit/tag creation. Revisit option (b) or (c) only then.

## DRY scorecard

The inspected `rs-modde` `justfile` currently has 21 executable targets, not 25+. This table records every `rs-modde` target and the surveyed overlap.

| target | rs-modde? | bikipy? | SynDB? | bootswain? | steampipe? | category | reuse evidence |
|---|---:|---:|---:|---:|---:|---|---|
| check | yes | yes | future | future | future | generic | rs-modde + bikipy; SynDB likely once Rust checks are centralized |
| test | yes | yes | future | future | future | generic | rs-modde + bikipy |
| lint | yes | yes | future | future | future | generic | rs-modde + bikipy |
| fmt | yes | yes | future | future | future | generic | rs-modde + bikipy |
| build | yes | yes | future | future | future | generic | rs-modde + bikipy |
| run | yes | yes | no | no | no | generic | rs-modde + bikipy CLI package wrapper |
| gui | yes | no | no | no | no | project-specific | `modde-ui` package binding only |
| coverage | yes | future | future | future | future | generic | common Rust workspace need; implement for rs-modde first |
| coverage-html | yes | future | future | future | future | generic | same cargo-llvm-cov family as coverage |
| coverage-lcov | yes | future | future | future | future | generic | CI upload format likely reused |
| coverage-ci | yes | future | future | future | future | generic | thresholded CI coverage reusable once projects opt in |
| release-dry | yes | future | future | no | no | generic | exact cargo-release dry-run wrapper reusable for Rust crates |
| release | yes | future | future | no | no | generic | cargo-release + optional spec rewrite |
| copr-vendor | yes | future | no | no | no | generic | reusable for any Rust RPM/COPR packaging; rs-modde first |
| copr-vendor-check | yes | future | no | no | no | generic | pairs with `copr-vendor` |
| copr-srpm | yes | future | no | no | no | generic | RPM/COPR packaging path, config-gated |
| nix-build | yes | future | future | yes | yes | generic | rs-modde + bootswain flake app + common Nix projects |
| nix-docs | yes | future | future | no | no | generic | Zola/mdBook Nix build targets recur in docs projects |
| nix-dev | yes | future | future | future | future | drop | plain `nix develop`; document, do not wrap unless UX requires |
| docs-serve | yes | future | future | no | no | generic | Zola/mdBook server abstraction |
| site-serve | yes | future | future | no | no | generic | same as docs serve with named site |

Survey-only candidates, included to validate the cross-project surface and keep Phase 05 grounded:

| target | rs-modde? | bikipy? | SynDB? | bootswain? | steampipe? | category | reuse evidence |
|---|---:|---:|---:|---:|---:|---|---|
| default | no | yes | no | no | no | drop | `just --list`; clap help replaces it |
| fmt-check | no | yes | future | future | future | generic | maps to `run_fmt(Check)` |
| doc | no | yes | future | no | no | generic | cargo docs wrapper can be added after v0.1 if needed |
| audit | no | yes | future | no | no | generic | bikipy uses `cargo audit` + `cargo deny`; defer until second consumer requests it |
| nautilus-package | no | no | yes | no | no | project-specific | Helm chart packaging, outside Rust/Nix xtask core |
| flash-targets | no | no | no | yes | no | project-specific | bootswain flake app wrapper for hardware flashing |
| flash | no | no | no | yes | no | project-specific | destructive hardware workflow; keep local |
| rockpro64-validate-scenario | no | no | no | yes | no | project-specific | board lab orchestration |
| rockpro64-validate-spi | no | no | no | yes | no | project-specific | board lab orchestration |
| rockpro64-validate-installed-firmware | no | no | no | yes | no | project-specific | board lab orchestration |
| rockpro64-validate-boot-media-scenario | no | no | no | yes | no | project-specific | board lab orchestration |
| rockpro64-validate-boot-media | no | no | no | yes | no | project-specific | board lab orchestration |
| rockpro64-validate-stable | no | no | no | yes | no | project-specific | board lab orchestration |
| update-hash | no | no | no | no | yes | project-specific | Nix fixed-output hash patching for steampipe only |
| diag-fixture | no | no | no | no | yes | project-specific | VM fixture diagnostic script |
| diag-fixture-keep | no | no | no | no | yes | project-specific | VM fixture diagnostic script |

`generic` means lift to `harbor-xtask` when v0.1 needs it. `project-specific` means keep in the downstream xtask binary if migrated. `drop` means do not implement a wrapper unless a later phase proves the wrapper adds behavior beyond the underlying tool.

## rs-modde target mapping

| just target | xtask command | category | implementation owner |
|---|---|---|---|
| `check` | `cargo xtask check` | generic | `harbor-xtask::run_check` |
| `test *ARGS` | `cargo xtask test -- <ARGS>` | generic | `harbor-xtask::run_test` |
| `lint` | `cargo xtask lint` | generic | `harbor-xtask::run_lint` |
| `fmt` | `cargo xtask fmt` | generic | `harbor-xtask::run_fmt(Write)` |
| `build` | `cargo xtask build --release` | generic | `harbor-xtask::run_cargo_build` |
| `run *ARGS` | `cargo xtask run -- <ARGS>` | generic | `harbor-xtask::run_cargo_package` with `modde-cli` |
| `gui` | `cargo xtask gui` | project-specific | `modde-xtask` binds `modde-ui` |
| `coverage` | `cargo xtask coverage` | generic | `harbor-xtask::run_coverage(Summary)` |
| `coverage-html` | `cargo xtask coverage --html` | generic | `harbor-xtask::run_coverage(Html)` |
| `coverage-lcov` | `cargo xtask coverage --lcov` | generic | `harbor-xtask::run_coverage(Lcov)` |
| `coverage-ci FAIL_UNDER` | `cargo xtask coverage --ci --fail-under <N>` | generic | `harbor-xtask::run_coverage(Ci)` |
| `release-dry VERSION` | `cargo xtask release <VERSION> --dry-run` | generic | `harbor-xtask::run_release(DryRun)` |
| `release VERSION` | `cargo xtask release <VERSION>` | generic | `harbor-xtask::run_release(Execute)` |
| `copr-vendor` | `cargo xtask copr vendor` | generic | `harbor-xtask::run_copr_vendor` |
| `copr-vendor-check` | `cargo xtask copr vendor-check` | generic | `harbor-xtask::run_copr_vendor_check` |
| `copr-srpm VERSION` | `cargo xtask copr srpm <VERSION>` | generic | `harbor-xtask::run_copr_srpm` |
| `nix-build` | `cargo xtask nix build modde` | generic | `harbor-xtask::run_nix_build` |
| `nix-docs` | `cargo xtask nix build site` | generic | `harbor-xtask::run_nix_build` |
| `nix-dev` | none | drop | call `nix develop` directly |
| `docs-serve` | `cargo xtask docs serve docs` | generic | `harbor-xtask::run_docs_serve` |
| `site-serve` | `cargo xtask docs serve site` | generic | `harbor-xtask::run_docs_serve` |

## CI compatibility

The inspected `.forgejo/workflows/release.yml` currently runs:

```sh
nix shell nixpkgs#rpm-build nixpkgs#cargo nixpkgs#rustc -c bash
just copr-vendor
just copr-vendor-check
rpmbuild -bs modde.spec ...
```

The chosen design works in that environment because the replacement is cargo-only:

```sh
cargo xtask copr vendor
cargo xtask copr vendor-check
```

No additional toolchain beyond `cargo` and `rustc` is required for the xtask binary itself. `rpmbuild` remains provided by `nixpkgs#rpm-build`. The workflow still needs `tar`/gzip behavior for `vendor.tar.gz`, but Phase 02 should implement tarball creation and layout validation in Rust libraries rather than shelling to host-specific `tar` flags.

The COPR upstream `.copr/Makefile` should remain self-contained shell in Phase 04. COPR's builder invokes that Makefile outside the repository's cargo alias context, so replacing it with `cargo xtask` would add an avoidable bootstrap dependency.

## Migration phasing

The downstream phase graph remains:

1. **Phase 02: `harbor-xtask` library in rs-harbor.** Implement the API in this document. Do not modify `rs-harbor/cli/src/main.rs`.
2. **Phase 03: rs-modde xtask binary.** Add `crates/modde-xtask/`, configure `ProjectConfig`, add `cargo xtask` alias, and keep the existing `justfile` until CI migrates.
3. **Phase 04: CI migration and justfile removal.** Replace Forgejo release workflow `just` calls with `cargo xtask`; leave `.copr/Makefile` self-contained; delete `justfile` in a separate commit after the release path is verified.
4. **Phase 05: cross-project rollout.** Write a rollout playbook and pilot bikipy first. Treat SynDB as a follow-up. Defer bootswain hardware-lab workflows and steampipe diagnostic/hash helpers unless a separate request makes them worthwhile.

New constraints discovered:

- `rs-modde` has uncommitted local changes in release-related files during this design phase. This phase must commit only `docs/planning/xtask-tooling-cli-uplift/DECISION.md`.
- `simit release` is bump-kind based, not exact-version based. That blocks direct delegation for the current `just release VERSION` parity goal.
- `.copr/Makefile` cannot assume `cargo xtask` is available inside COPR's SCM builder. Keep it shell-based unless COPR setup is changed deliberately.
- The current `rs-modde` justfile has 21 executable targets. Any reference to 25+ targets is stale relative to the inspected working tree.

## Rejected alternatives

### Extend `rs-harbor` CLI with tooling subcommands

Rejected for v0.1. It would force the shared `rs-harbor` binary to accept downstream-specific config flags for spec paths, docs roots, package names, and COPR archive URLs. That turns every consumer workflow into part of the harbor CLI's compatibility surface.

### Put `harbor-xtask` in rs-modde first

Rejected. It is fastest for Phase 03 but undermines the cross-project goal. The surveyed bikipy justfile already overlaps on check/test/lint/fmt/build/run/audit, and SynDB is a likely Rust workspace consumer.

### Depend on simit for release in v0.1

Rejected for now. `simit` is useful, but its release API currently centers on semantic bump kinds and changelog/commit/tag flow. `rs-modde` needs exact-version parity with `just release VERSION` and RPM spec rewriting. Revisit after `simit` exposes a library API that accepts an exact `semver::Version`.

### Hybrid cargo-release plus simit

Rejected for now. Combining two release tools in v0.1 creates ordering ambiguity: which tool owns manifest edits, lockfile updates, changelog edits, commit staging, and tag creation. The conservative cargo-release path preserves today's behavior.

### Wrap every just target in harbor-xtask

Rejected. `nix-dev` is better as direct `nix develop`; bootswain's board validation and steampipe's fixture diagnostics are project automation, not reusable build-tooling primitives.

## Open questions

- **Phase 02 owner:** decide whether `run_copr_vendor` uses Rust `tar`/`flate2` crates or shells to `tar`. Recommendation: Rust implementation for portable layout validation.
- **Phase 02 owner:** decide whether to expose clap `Args` structs from `harbor-xtask`. Recommendation: no for v0.1; keep clap in per-project binaries.
- **Phase 03 owner:** choose the exact `modde-xtask` dependency wiring: git dependency pinned through `Cargo.lock` versus local path into a flake-unpacked `rs-harbor` source. Recommendation: git dependency unless the flake devshell already exposes a stable local path.
- **Phase 05 owner:** pilot bikipy and then decide whether `audit` deserves a `harbor-xtask` v0.2 function.
