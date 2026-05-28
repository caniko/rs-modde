# Phase 07 — crates.io library publishing

> **Recommended Codex model: GPT 5.5 medium**
>
> Decide which workspace crates are _library surface_ vs _binary internals_, set their `Cargo.toml` metadata for publishing, then add a `simit release`-driven (or `cargo workspaces publish`-driven) publish flow that runs after the Codeberg release succeeds. Mechanical with one design call (which crates publish? `modde-core` and `modde-sources` look library-shaped; `modde-cli` and `modde-ui` are binaries that shouldn't); routine.

## Working tree

- `crates/modde-core/Cargo.toml`, `crates/modde-sources/Cargo.toml`, `crates/modde-games/Cargo.toml` — add `description`, `repository`, `license`, `readme`, `keywords`, `categories`, `homepage`, `documentation`.
- `crates/modde-cli/Cargo.toml`, `crates/modde-ui/Cargo.toml` — `publish = false`.
- `.forgejo/workflows/release.yml` — new "Publish to crates.io" step OR document doing it via local `simit release`.
- `simit.toml` — verify simit's release flow handles workspace publish ordering.

## Goal

1. Every product crate is published to crates.io on each tag, in dependency
   order. The maintainer's design call is to publish **all** product crates,
   including the binaries `modde-cli` and `modde-ui` (so `cargo install
modde-cli` works), and to keep only the workspace's pure tooling crate
   (`modde-xtask`) as `publish = false`. The library crates `modde-core`,
   `modde-sources`, `modde-games` publish for downstream library use.
2. Each publishable crate has the metadata required to render well on
   crates.io / docs.rs (description, license, license-file, repository,
   readme, keywords, categories, rust-version, documentation).
3. docs.rs builds successfully for each published crate (no platform-specific
   deps in default features that break docs.rs's linux container).
4. The publish flow is driven per crate by `simit init-ci`-generated
   `.forgejo/workflows/publish-crate-<name>.yaml` workflows so each crate can
   re-publish independently when a version-only release of one crate is
   needed.

## Why

The user's memory notes preference for `simit` (commit/release/init-ci/init-flake/changelog). If `simit release` is the canonical flow, this phase mostly verifies it covers crates.io. Publishing library crates lets others build on top of modde-core (e.g., FOMOD parsers, Wabbajack format readers) and adds a real distribution surface beyond binaries.

## Out of scope

- Splitting binaries into separate library + thin-binary crates beyond what already exists. Audit the current split; don't refactor.
- Yanking strategy for broken releases — covered in phase 09.
- crates.io ownership / co-maintainer setup.

## Plan

1. Inventory `crates/*/Cargo.toml`:
   - `modde-core` → publish (likely the most-reused).
   - `modde-sources` → publish (Nexus/Wabbajack/etc. format parsers — reusable).
   - `modde-games` → publish (game-detection logic — reusable).
   - `modde-cli` → `publish = false`.
   - `modde-ui` → `publish = false`.
     Adjust based on what each crate actually exposes; if a "library" crate has no `lib.rs` it's not actually a library.
2. For each publishable crate, ensure `Cargo.toml [package]` has:
   ```toml
   description = "..."             # one-line, ≤200 chars
   repository = "https://codeberg.org/caniko/rs-modde"
   homepage   = "https://modde.tartanoglu.com"
   license    = "GPL-3.0-only"
   readme     = "README.md"        # crate-local README; create if missing
   keywords   = ["modding", "games", "..."]   # ≤5 entries
   categories = ["game-development", "..."]   # crates.io's controlled list
   rust-version = "1.86"           # match workspace MSRV
   ```
3. Add a `README.md` to each publishable crate (short, scope-of-crate explainer + link to main repo).
4. Verify `cargo publish --dry-run -p modde-core` succeeds (catches missing files, banned deps, version-bump issues).
5. Add `CRATES_IO_API_TOKEN` Forgejo secret.
6. Decide flow:
   - **Option A — simit handles it**: run `simit release` from the maintainer's machine (per the user's memory preference), and add a CI step that only verifies the published versions match the tag (post-flight check). _Preferred._
   - **Option B — CI publishes**: add a `cargo workspaces publish --from-git --token "$CRATES_IO_API_TOKEN"` step after the Codeberg release; uses workspace ordering automatically.
7. Add a docs.rs check: ensure `cargo doc --no-deps -p modde-core --all-features` succeeds on a Linux container (docs.rs's environment).
8. Document the policy in `CONTRIBUTING.md`: "Which crates publish to crates.io".

## Acceptance criteria

- [ ] `cargo publish --dry-run -p <crate>` succeeds for every publishable crate
      (`modde-core`, `modde-sources`, `modde-games`, `modde-cli`, `modde-ui`).
      The `Dry-run publish` step in each
      `.forgejo/workflows/publish-crate-<name>.yaml` enforces this on every tag.
- [ ] Pure tooling crates that are not user-facing have `publish = false`.
      Today this is only `modde-xtask`.
- [ ] Each publishable crate sets `description`, `repository`, `license`,
      `license-file`, `readme`, `keywords`, `categories`, `rust-version`, and
      `documentation` (the last pinned to `https://docs.rs/<crate>`).
- [ ] Before publishing, each crate's per-crate workflow queries
      `crates.io/api/v1/crates/<crate>/<version>`; if HTTP 200 (already
      published) the publish step exits 0, and only HTTP 404 (new version) is
      allowed to call `cargo publish`. Any other status fails the workflow.
- [ ] docs.rs renders each published crate without build errors (check after
      first publish; if it fails, add `[package.metadata.docs.rs]` with
      `no-default-features` or `features = [...]` as needed).
- [ ] `CONTRIBUTING.md` documents the publish policy (which crates publish,
      per-crate release tag flow, and how to re-publish a single crate).

## Files likely touched

- `crates/modde-core/Cargo.toml`, `crates/modde-sources/Cargo.toml`, `crates/modde-games/Cargo.toml`, plus their `README.md` files.
- `crates/modde-cli/Cargo.toml`, `crates/modde-ui/Cargo.toml` (`publish = false`).
- `.forgejo/workflows/release.yml` (post-flight verification step).
- `CONTRIBUTING.md`.
- `simit.toml` (only if simit needs an explicit `[publish]` section).

## Pitfalls

- crates.io has a hard limit of 5 keywords and they must be lowercase alphanumeric+hyphen.
- `cargo publish` requires all path-deps to also be published with the same version OR have a `version = "..."` field in `[dependencies]` (path-deps without versions are unpublishable). Workspace inheritance often hides this; check each crate.
- docs.rs builds in a clean Linux container with no GPU; if `modde-ui` is ever published (it shouldn't be), wgpu/winit will fail there.
- crates.io versions are immutable; an accidentally-published broken version requires `cargo yank` (covered in phase 09's hotfix policy).
- Memory says the user prefers `simit` for releases — verify `simit release` actually invokes `cargo publish` for workspace libraries before assuming Option A works; if simit doesn't, fall back to Option B.

## Reference

- crates.io publishing: https://doc.rust-lang.org/cargo/reference/publishing.html
- docs.rs config: https://docs.rs/about/metadata
- User memory: `feedback_use_simit.md` — prefer simit for releases.
