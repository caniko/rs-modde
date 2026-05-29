# Phase 08 — Documentation: module headers + public-API docs

> **Recommended model for merge/orchestration: GPT 5.5 low**
>
> The two sub-layers touch disjoint crates and the merge is trivial (commit each
> independently). No orchestration judgment is needed beyond confirming both
> `cargo doc` runs are warning-clean — so the merge step is `low`, and each
> sub-layer is itself `low` (mechanical, accurate doc-writing).

## Sub-layers

| # | Slug | Model | Touches | Sub-layer file |
|---|------|-------|---------|----------------|
| 01 | modde-sources | 5.5 low | `crates/modde-sources/src/**` (doc comments only) | [sub-01-modde-sources.md](./sub-01-modde-sources.md) |
| 02 | modde-games | 5.5 low | `crates/modde-games/src/**` (doc comments only) | [sub-02-modde-games.md](./sub-02-modde-games.md) |

## Goal (phase-level)

The two library crates with the weakest documentation — `modde-sources` (~40%
public-item coverage) and `modde-games` (~42%) — gain `//!` module headers on
every source file and doc comments on their meaningful public items, bringing them
toward `modde-core`'s ~90% bar. No `#![deny(missing_docs)]` is added (the crates
are unpublished); this is about navigability, not lint enforcement.

## Why this matters now

The doc-public-api audit found no `//!` module headers anywhere in the three lib
crates and item-doc coverage of ~40% in sources/games vs ~90% in core. The biggest
navigability gaps are the core abstraction types in `modde-games`
(`policies.rs`, `scanner_patterns.rs`, `traits.rs`, `registry.rs`). Because the
crates are unpublished (GPL-3.0 app), this is the lowest-priority phase — run it
**last**, after every signature is final, so the docs describe the shipped API
rather than an intermediate one.

## Out of scope

- `modde-core` item docs (already ~90% — leave; a `//!` top-up is optional and
  folded into whichever sub-layer touches a shared boundary, but not required).
- `modde-cli` / `modde-ui` (binaries/GUI, not library API).
- Adding `#![deny(missing_docs)]` or `#![warn(missing_docs)]` (would break `-D
  warnings`; explicitly not wanted).
- Per-field docs on obvious serde DTOs that mirror external APIs (`NexusMod`,
  `GitHubReleaseAsset`, etc.) — document the *type*, skip the self-evident fields.

## Merge plan

The sub-layers edit disjoint crates, so there is no real merge conflict. The user
(or a single session) dispatches sub-01 and sub-02 — in parallel or sequentially —
and commits each as its own `docs(sources): …` / `docs(games): …` commit. The only
cross-cutting check is the phase-level acceptance below (run `cargo doc` for the
whole workspace once both land). No script, no harness.

## Phase-level acceptance criteria

- [ ] Every `crates/modde-sources/src/**.rs` and `crates/modde-games/src/**.rs`
      module file has a `//!` header stating its purpose (`rg -L --files-without-match
      '^//!' crates/modde-{sources,games}/src -g '*.rs'` returns only `mod.rs`
      re-export shims or genuinely trivial files, noted in the commit).
- [ ] The key abstraction types are documented (games: `policies.rs`,
      `scanner_patterns.rs`, `traits.rs`, `registry.rs` types; sources: the
      `DownloadSource`/`AnySource` surface and non-DTO public types).
- [ ] `cargo doc --no-deps -p modde-sources -p modde-games` builds with **zero**
      warnings.
- [ ] `cargo clippy -p modde-sources -p modde-games --all-targets -- -D warnings`
      stays clean (new doc comments must satisfy `clippy::doc_markdown` —
      backtick code-ish words).
- [ ] No code changed — `git diff` shows only added `///`/`//!` lines.

## Reference

- Doc-public-api audit (rust-ultra polish stage): coverage figures and the
  games-crate key-type gap list.
- Run last (README Wave 6), after all signature-changing phases land.
