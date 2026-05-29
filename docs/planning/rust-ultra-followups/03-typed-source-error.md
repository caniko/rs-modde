# Phase 03 — Error architecture: typed `SourceError` on the download boundary

> **Recommended Codex model: GPT 5.5 high**
>
> Complex design + threading: introducing a typed error enum on the
> `DownloadSource` boundary means classifying every `bail!`/`anyhow!` in the
> network layer into the right variant, wiring `#[from]`/`map_err` through ~8
> source implementations and an `AnySource` enum dispatcher, and updating the two
> call sites to branch on it — without losing any context users currently see.
> That's a non-trivial design call across a subsystem; `medium` would likely
> flatten variants or miss a classification. Orchestrator role within one crate.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. **Run after Phase 02**
(both edit `modde-cli/src/commands/install.rs`) and after Phase 04 has *not*
necessarily landed — but if 04 lands first, expect `game_id` signatures here to
be `&GameId` rather than `&str`; adapt. Mostly inside `crates/modde-sources`.

## Goal

The download path surfaces *typed*, matchable failures instead of opaque anyhow
strings. A `SourceError` enum distinguishes the conditions a caller can act on —
unauthorized (re-auth), rate-limited (back off / retry-after), not-found,
hash-mismatch, and transport errors — so `modde-cli`/`modde-ui` can offer real
recovery instead of only printing text. The user-visible error messages for the
non-actionable cases are at least as informative as today.

## Why this matters now

The error-architecture audit found the modde-sources Nexus/network layer
(`nexus/api.rs`, `nexus/cdn.rs`, `nexus/graphql.rs`, `mega/`, `gdrive/`,
`mediafire/`) uses `bail!`/`anyhow!` for distinct conditions a caller *should*
branch on (401 vs 429 vs 404 vs hash-mismatch vs network), all flattened into
anyhow strings. So the UI can't, e.g., prompt re-auth on 401 or honor a
`Retry-After` on 429 — it can only display the string. The audit also confirmed
the *good* precedent already in the tree: `InstallerError`
(`modde-core/src/installer/types.rs:243`) is matched by callers
(`nexus/install.rs:147,161`, `cli/commands/install.rs:763,777`) to drive distinct
UX. This phase extends that proven pattern to the download boundary.

Note the `DownloadSource` trait design (from the trait-design audit): it uses
RPIT (`impl Future` methods, *not* `dyn`-compatible) plus a hand-rolled
`AnySource` enum for dispatch (`modde-sources/src/traits.rs:35`). The error type
flows through the trait methods and `AnySource`, not through a `Box<dyn>`.

## Out of scope

- Converting the *other* lib trait APIs (`GamePlugin`, `SaveTracker`, `ModScanner`
  in modde-games) off anyhow — the audit found zero callers `match` on them, so
  it's high-ripple/no-payoff. Leave them.
- The modde-ui `Result<T, String>` `Task` boundary — that's framework-required;
  leave it.
- Building the actual re-auth/retry-after *UX* in the UI/CLI (this phase makes it
  *possible* by typing the error; wiring the recovery flow is a separate feature).
  Do update call sites to *propagate/translate* the typed error, but don't invent
  new interactive flows.
- Duplicating `HashMismatch` — reuse `CoreError::HashMismatch` if it fits, or
  reference it; don't define a second hash-mismatch type.

## Plan

1. Inventory the download-boundary failures: `rg -n 'bail!|anyhow!|\.context\(|map_err'
   crates/modde-sources/src/{nexus,mega,gdrive,mediafire}` and read
   `crates/modde-sources/src/traits.rs` (`DownloadSource`, `AnySource`). Classify
   each failure into: `Unauthorized`, `RateLimited { retry_after: Option<Duration> }`,
   `NotFound`, `HashMismatch { expected, actual }` (reuse `CoreError`'s if
   suitable), `Network(reqwest::Error)`, and a catch-all `Other(anyhow::Error)` for
   genuinely opaque cases.
2. Define `#[derive(Debug, thiserror::Error)] pub enum SourceError { ... }` in
   `modde-sources` (e.g. `src/error.rs` or in `traits.rs`). Add `#[from]` for
   `reqwest::Error` where it cleanly maps; keep an `Other(#[from] anyhow::Error)`
   escape hatch so migration is incremental.
3. Change the `DownloadSource` trait's `resolve`/`download` (and whatever returns
   the network failures) to return `Result<_, SourceError>`. Update `AnySource`'s
   dispatch to thread `SourceError` through. Update each source impl (`nexus`,
   `mega`, `gdrive`, `mediafire`, github, http, manual, wabbajack-cdn) to return
   the right variant — translate `reqwest` status codes (401→Unauthorized,
   429→RateLimited with `Retry-After`, 404→NotFound) at the HTTP boundary.
4. Update the call sites (the installer/`ensure_archive_trusted` download path and
   `cli/commands/install.rs`) to consume `SourceError`. At minimum, convert to the
   existing anyhow/`InstallerError` flow with the *same or better* message; where
   the existing `continue_on_error`/missing-archive policy already branches, route
   the typed variant in. Do not regress any current error text a user sees.
5. Keep the change **incremental and green**: the `Other(anyhow::Error)` variant
   plus `#[from]` lets you migrate sources one at a time, compiling between each.
6. Gate and commit (`refactor(sources): typed SourceError on the download boundary`).

## Acceptance criteria

- [ ] `SourceError` exists with at least `Unauthorized`, `RateLimited`, `NotFound`,
      `HashMismatch`, `Network`, and an anyhow escape-hatch variant; it derives
      `thiserror::Error` with useful `Display`.
- [ ] `DownloadSource::resolve`/`download` (and `AnySource`) return
      `Result<_, SourceError>`; every source impl compiles against it.
- [ ] At least the HTTP sources map 401/429/404 to the typed variants (verify with
      a `wiremock`-backed unit test per status code — the workspace already uses
      `wiremock`).
- [ ] Existing download/install tests pass unchanged; no user-facing error message
      regresses (spot-check the messages for a failed download).
- [ ] `cargo clippy -p modde-sources -p modde-cli --all-targets -- -D warnings` clean.

## Files likely touched

- `crates/modde-sources/src/traits.rs` (trait + `AnySource` + maybe the enum).
- `crates/modde-sources/src/error.rs` (new, if you put `SourceError` there).
- `crates/modde-sources/src/{nexus/api.rs,nexus/cdn.rs,nexus/graphql.rs,nexus/install.rs,mega/mod.rs,gdrive/mod.rs,mediafire/mod.rs}` and the github/http/manual/cdn sources.
- `crates/modde-sources/src/wabbajack/installer.rs` (download call site).
- `crates/modde-cli/src/commands/install.rs` (consume the typed error).

## Pitfalls

- **Symptom:** RPIT trait method won't compile with the new error. **Cause:** the
  `DownloadSource` methods return `impl Future`; changing the `Output` error type
  must be consistent across the trait and `AnySource`. **Recovery:** change the
  associated return types together; lean on `Other(#[from] anyhow::Error)` to keep
  partially-migrated sources compiling.
- **Symptom:** lost error context (user sees "request failed" instead of the URL).
  **Cause:** `#[from] reqwest::Error` drops the `.context(...)` chain. **Recovery:**
  carry context in the variant (e.g. a `String`/url field) or keep a `.context`
  before converting.
- **Symptom:** `Retry-After` parsing is wrong. **Cause:** the header can be
  seconds or an HTTP-date. **Recovery:** handle both; default to `None` on parse
  failure (still classified `RateLimited`).
- **Symptom:** scope creep into modde-games trait errors. **Recovery:** stop — out
  of scope.

## Reference

- The proven typed-error precedent: `InstallerError`
  (`modde-core/src/installer/types.rs:243`) and its match sites
  (`nexus/install.rs:147,161`, `cli/commands/install.rs:763,777`).
- `DownloadSource`/`AnySource`: `crates/modde-sources/src/traits.rs:35`.
- Error-architecture audit finding #3 (rust-ultra design stage).
- Land after [02](./02-observability-library-output.md) (shared cli/install.rs).
