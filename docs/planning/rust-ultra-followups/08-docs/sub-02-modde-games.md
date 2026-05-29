# Phase 08 · Sub-layer 02 — Document `modde-games`

> **Recommended Codex model: GPT 5.5 low**
>
> Mechanical, accurate doc-writing on one crate. The only judgment is which
> abstraction types deserve the most care (the audit names them) and keeping
> `clippy::doc_markdown` happy. Leaf node — `low` effort.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. Edit **only**
`crates/modde-games/src/**`. Disjoint from sub-01 (modde-sources) — safe to run
concurrently. Run after the signature-changing phases (notably 04) have landed.

## Goal

Every `modde-games` source file has a `//!` header, and the crate's core
abstraction types — the policy, scanner-pattern, scanning-trait, and registry
vocabulary — are documented, bringing the crate from ~42% toward `modde-core`'s
~90% bar, with no code change.

## Why this matters now

`modde-games` is ~42% documented with no `//!` headers (doc-public-api audit). It
defines the reusable abstractions the per-game modules build on, so undocumented
they're the hardest part of the workspace to onboard to. The audit named the
highest-value gaps explicitly:

- `policies.rs` — `BareLayoutPolicy`, `DllOverridePolicy`, `ModDirectoryLayout`,
  `CollisionPolicy`, `PolicyCollisionClassifier`.
- `scanner_patterns.rs` — `DirectoryModRule`, `SingleFileModRule`, `FileGroupRule`.
- `traits.rs` — `ScanContext`, `DiscoveredMod`, `ModSource`, `ModScanner` (the
  central scanning interface; the `GamePlugin` trait at `traits.rs:159` is already
  documented — good).
- `registry.rs` — `EngineFamily`, `GameRegistration`.

## Out of scope

- Any code change — only `///`/`//!` additions.
- Exhaustive per-field docs on every per-game data struct — document the type's
  role; skip self-evident fields.
- `#![deny(missing_docs)]` / `#![warn(missing_docs)]`.

## Plan

1. `rg -L --files-without-match '^//!' crates/modde-games/src -g '*.rs'`; add a
   purpose `//!` to each module file.
2. Document the named abstraction types first (`policies.rs`,
   `scanner_patterns.rs`, `traits.rs`, `registry.rs`), then sweep remaining
   undocumented public items (`rg -n '^\s*pub (fn|struct|enum|trait|type)'`,
   cross-check for a preceding `///`).
3. Backtick code-ish tokens to satisfy `clippy::doc_markdown`.
4. `cargo doc --no-deps -p modde-games` (zero warnings) + `cargo clippy -p
   modde-games --all-targets -- -D warnings`. Commit `docs(games): module headers +
   public-API docs`.

## Acceptance criteria

- [ ] No `modde-games/src` module file lacks a `//!` header (except noted trivial shims).
- [ ] The named types in `policies.rs`, `scanner_patterns.rs`, `traits.rs`, and
      `registry.rs` are documented.
- [ ] `cargo doc --no-deps -p modde-games` emits zero warnings.
- [ ] `cargo clippy -p modde-games --all-targets -- -D warnings` clean.
- [ ] `git diff` shows only added doc lines.

## Files likely touched

`crates/modde-games/src/**.rs` (doc comments only) — priority: `policies.rs`,
`scanner_patterns.rs`, `traits.rs`, `registry.rs`; then the per-game and tools modules.

## Pitfalls

- **Symptom:** `clippy::doc_markdown` failure. **Recovery:** backtick identifiers/paths.
- **Symptom:** docs reference `game_id: &str` but Phase 04 made it `&GameId`.
  **Cause:** documenting before 04 landed. **Recovery:** run after 04; match the
  current signature.
- **Symptom:** a `///` example breaks doctests. **Recovery:** keep examples
  minimal or fence as `text`/`ignore`.

## Reference

- Doc-public-api audit (rust-ultra polish stage) — the named key-type list.
- Phase overview: [README.md](./README.md). Disjoint from
  [sub-01-modde-sources.md](./sub-01-modde-sources.md).
