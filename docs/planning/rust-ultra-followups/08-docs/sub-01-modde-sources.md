# Phase 08 · Sub-layer 01 — Document `modde-sources`

> **Recommended Codex model: GPT 5.5 low**
>
> Mechanical, accurate doc-writing on one crate: add `//!` module headers and
> doc comments to meaningful public items, skipping self-evident serde DTO fields.
> The only constraint is `clippy::doc_markdown` cleanliness. A leaf node — `low`
> effort is right; higher tiers waste tokens on prose.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. Edit **only**
`crates/modde-sources/src/**`. Disjoint from sub-02 (modde-games) — safe to run
concurrently. Run this after the signature-changing phases (03, 05) have landed so
docs describe the final API.

## Goal

Every `modde-sources` source file has a `//!` header explaining its purpose, and
the crate's meaningful public items (the `DownloadSource`/`AnySource` surface,
`SourceError` if Phase 03 landed, the wabbajack installer/manifest public types,
cache/staging types) carry accurate doc comments — without documenting
self-evident serde DTO fields and without changing any code.

## Why this matters now

`modde-sources` sits at ~40% public-item doc coverage with no `//!` module headers
(doc-public-api audit). The download-source and wabbajack-install surface is the
crate's contract with `modde-cli`/`modde-ui`; undocumented, it's hard to navigate.

## Out of scope

- Any code change — only `///` and `//!` additions.
- Per-field docs on serde DTOs mirroring external APIs (`NexusMod`,
  `GitHubReleaseAsset`, MEGA/MediaFire/GDrive response structs) — document the
  *type's* role, skip obvious fields.
- `#![deny(missing_docs)]` / `#![warn(missing_docs)]`.
- The `src/bin/update-wabbajack-fixture.rs` dev tool (not library API).

## Plan

1. `rg -L --files-without-match '^//!' crates/modde-sources/src -g '*.rs'` to list
   files missing a module header. Add a one-to-three-line `//!` to each stating
   what the module does (skip pure `mod.rs` re-export shims if truly trivial, but
   prefer a one-liner even there).
2. For undocumented public items (`rg -n '^\s*pub (fn|struct|enum|trait|type)'` and
   cross-check for a preceding `///`), add concise, accurate doc comments. Prioritize:
   `traits.rs` (`DownloadSource`, `AnySource`), `wabbajack/installer.rs` public
   types/methods, `wabbajack/manifest`-facing types, `cache`/`staging` types, and
   `SourceError` (if present).
3. Backtick every code-ish token in docs (`` `DownloadSource` ``, `` `u64` ``,
   paths, fn names) to satisfy `clippy::doc_markdown`.
4. `cargo doc --no-deps -p modde-sources` (zero warnings) and `cargo clippy -p
   modde-sources --all-targets -- -D warnings`. Commit `docs(sources): module
   headers + public-API docs`.

## Acceptance criteria

- [ ] No `modde-sources/src` module file lacks a `//!` header (except noted trivial
      re-export shims).
- [ ] `DownloadSource`, `AnySource`, the wabbajack installer public surface, and
      `SourceError` (if present) are documented.
- [ ] `cargo doc --no-deps -p modde-sources` emits zero warnings.
- [ ] `cargo clippy -p modde-sources --all-targets -- -D warnings` clean.
- [ ] `git diff` shows only added doc lines (no code change).

## Files likely touched

`crates/modde-sources/src/**.rs` (doc comments only) — heaviest in `traits.rs`,
`wabbajack/*.rs`, `nexus/*.rs`, `cache.rs`, `staging.rs`, `decompress/mod.rs`.

## Pitfalls

- **Symptom:** `clippy::doc_markdown` fails the build. **Cause:** an un-backticked
  identifier/path in a doc comment. **Recovery:** backtick it.
- **Symptom:** a doc comment is inaccurate after a recent signature change.
  **Cause:** docs written before Phases 03/05 landed. **Recovery:** run this
  sub-layer *after* those phases; describe the current signature.
- **Symptom:** doc-test compile failure. **Cause:** a `///` example with real code.
  **Recovery:** keep examples minimal or mark fenced blocks `text`/`ignore` if
  they're illustrative, not runnable.

## Reference

- Doc-public-api audit (rust-ultra polish stage).
- Phase overview: [README.md](./README.md). Disjoint from
  [sub-02-modde-games.md](./sub-02-modde-games.md).
