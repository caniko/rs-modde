# Phase 07 — Add a generic portfolio-publish surface to plinth

> **Recommended Codex model: GPT 5.5 high**
>
> Complex orchestrator-flavored work: design the publish surface
> (admin REST endpoint + CLI subcommand + per-tool manifest schema),
> align it with the existing blog-publish pattern, thread the
> `brick-portfolio` feature gate through three Cargo.toml files, and
> respect plinth's sandbox-safety constraints. A smaller model is
> likely to ship something that breaks `cargo check
> --no-default-features --features "ssr,brick-blog,brick-todo"` or
> reaches for `reqwest::Client::new()` in tests.

## Working tree

`/data/nvme0/can/Projects/solo/plinth` on its default branch.

## Goal

A tool's repository can ship a small `portfolio.toml` (or
`portfolio.md` with TOML frontmatter — pick one shape, document it)
that describes one portfolio entry. Running
`plinth-cli portfolio publish path/to/portfolio.toml` against a deployed
plinth instance authenticates with the admin Bearer token and inserts
or updates the corresponding row in `portfolio_items`, invalidating the
portfolio cache. The CLI command is feature-gated on
`brick-portfolio`. No existing blog/todo flow regresses.

## Why this matters now

Plinth's portfolio brick is fully implemented from the database up
through the read path: migration
[crates/server/migrations/0004_portfolio.sql](/data/nvme0/can/Projects/solo/plinth/crates/server/migrations/0004_portfolio.sql),
cache actor
[crates/server/src/bricks/portfolio/cache.rs](/data/nvme0/can/Projects/solo/plinth/crates/server/src/bricks/portfolio/cache.rs),
client page
[crates/client/src/pages/portfolio.rs](/data/nvme0/can/Projects/solo/plinth/crates/client/src/pages/portfolio.rs),
RSS feed
[crates/server/src/api/feeds.rs:109-118](/data/nvme0/can/Projects/solo/plinth/crates/server/src/api/feeds.rs#L109-L118),
nav config
[plinth.toml:30-32](/data/nvme0/can/Projects/solo/plinth/plinth.toml#L30-L32).
The only write path is a hard-coded seed `INSERT INTO portfolio_items`
at
[crates/server/src/services/db.rs:82-107](/data/nvme0/can/Projects/solo/plinth/crates/server/src/services/db.rs#L82-L107).
No admin endpoint, no CLI subcommand. That's the gap behind "doesn't
exist yet."

## Out of scope

- Adding `embedding`-backed vector search over portfolio items. Stay
  with simple slug-based reads.
- Adding image upload from the CLI for the `image_url` field. Authors
  pass an already-hosted URL (Immich asset URL or external).
- A web admin UI for portfolio editing. CLI-only for now.
- Adding "publish all `portfolio.d/*.toml` at startup" autoload —
  that's the alternative design rejected in the dossier.
- Backporting the publish flow to blog/todo.

## Plan

1. Define the manifest schema in
   [crates/shared/src/portfolio_item.rs](/data/nvme0/can/Projects/solo/plinth/crates/shared/src/portfolio_item.rs):
   add a `PublishPortfolioRequest` mirroring the `PortfolioItem` struct
   but with `id`, `html_content` optional/computed, and a content
   format hint (`markdown` for now; matches `ContentFormat::Markdown`
   in [crates/shared/src/content_format.rs](/data/nvme0/can/Projects/solo/plinth/crates/shared/src/content_format.rs)).
   Feature-gate the type on `brick-portfolio`.
2. Add an admin POST handler in plinth-server that:
   - Lives under `crates/server/src/api/admin/portfolio.rs` (create the
     `admin` module if it doesn't exist; pattern-match the blog admin
     handler if there is one — check `rg -ln "admin" crates/server/src`).
   - Path: `POST /api/admin/portfolio`.
   - Auth: same Bearer-token middleware as the existing blog publish
     path uses (per `PLINTH_API_KEY`).
   - Body: `PublishPortfolioRequest`.
   - Action: render markdown → HTML server-side (reuse whatever
     existing blog publish uses; check `crates/server/src/services/`
     for a markdown helper), insert/upsert into `portfolio_items` via
     a new function in
     [crates/server/src/services/db.rs](/data/nvme0/can/Projects/solo/plinth/crates/server/src/services/db.rs)
     parameterized on the manifest, then send `InvalidateAll` to the
     `PortfolioCache` actor.
   - Wire into the router behind `#[cfg(feature = "brick-portfolio")]`.
3. Add `portfolio` subcommand to
   [crates/cli/src/commands/](/data/nvme0/can/Projects/solo/plinth/crates/cli/src/commands/):
   - New file `portfolio.rs` exposing `publish(path: &Path, api: &ApiClient)`.
   - Reads the file (`portfolio.toml` — parse via `toml`, the existing
     plinth dep).
   - Validates required fields locally (`slug`, `title`, `description`,
     `tech_stack`, `date`).
   - Generates `slug` via `PortfolioItem::slugify` if absent.
   - POSTs to `/api/admin/portfolio` via the existing `ApiClient`.
   - Emits clear `ui::status` lines for success/failure.
   - Gate the module behind `#[cfg(feature = "brick-portfolio")]` in
     [crates/cli/src/commands/mod.rs](/data/nvme0/can/Projects/solo/plinth/crates/cli/src/commands/mod.rs)
     and in [crates/cli/src/main.rs](/data/nvme0/can/Projects/solo/plinth/crates/cli/src/main.rs)'s
     clap derive.
4. Update the three Cargo.toml files (`crates/cli`, `crates/server`,
   `crates/shared`) to expose `brick-portfolio` consistently. The
   plinth AGENTS.md already documents this: "Add a feature flag to all
   4 `Cargo.toml` files" — confirm the workspace root
   `Cargo.toml`'s metadata leptos block still includes
   `brick-portfolio` in the default feature set.
5. Add integration tests under `crates/server/tests/` using `#[sqlx::test]`
   per [AGENTS.md:64-72](/data/nvme0/can/Projects/solo/plinth/AGENTS.md#L64-L72):
   - Posting a valid manifest creates a `portfolio_items` row.
   - Posting an updated manifest with the same `slug` upserts (does not
     duplicate).
   - Posting without a Bearer token returns 401.
   - Posting with `brick-portfolio` disabled returns 404 / route does
     not exist (skipped or `#[cfg]`-ignored).
   Do **not** use `fastembed::TextEmbedding` or `reqwest::Client::new()`
   anywhere in these tests.
6. Run `nix flake check` — must be green.
7. Verify the bricks-disabled build still compiles:
   `cargo check -p plinth-server --no-default-features --features "ssr,brick-blog,brick-todo"`.
8. Update [AGENTS.md](/data/nvme0/can/Projects/solo/plinth/AGENTS.md):
   - Document the `portfolio.toml` schema (fields, types, defaults).
   - Document `plinth-cli portfolio publish <path>` (auth via
     `PLINTH_API_KEY`, target via `PLINTH_API_URL`).
   - Mention "this is how you add a new tool to the portfolio".
9. Commit on default branch — message: `Add portfolio publish CLI +
   admin endpoint behind brick-portfolio`.

## Acceptance criteria

- [ ] `plinth-cli portfolio publish --help` is documented in CLI help
      when built with `brick-portfolio`.
- [ ] `cargo check --workspace` passes with default features.
- [ ] `cargo check -p plinth-server --no-default-features --features "ssr,brick-blog,brick-todo"`
      passes (portfolio code is properly gated).
- [ ] `cargo test --workspace --exclude plinth-client` passes.
- [ ] `nix flake check` is green.
- [ ] Posting a `portfolio.toml` against a local plinth instance
      inserts a row visible at `GET /api/portfolio` and the cached
      list refreshes (verified with `curl`).
- [ ] Posting the same manifest twice upserts the row instead of
      creating duplicates.
- [ ] Posting without Bearer auth returns 401.
- [ ] AGENTS.md has a "Portfolio publishing" section documenting the
      schema and command.

## Files likely touched

- [crates/shared/src/portfolio_item.rs](/data/nvme0/can/Projects/solo/plinth/crates/shared/src/portfolio_item.rs).
- [crates/shared/Cargo.toml](/data/nvme0/can/Projects/solo/plinth/crates/shared/Cargo.toml).
- [crates/shared/src/lib.rs](/data/nvme0/can/Projects/solo/plinth/crates/shared/src/lib.rs).
- `crates/server/src/api/admin/portfolio.rs` (new).
- [crates/server/src/api/](/data/nvme0/can/Projects/solo/plinth/crates/server/src/api/) — `mod.rs` and router wiring.
- [crates/server/src/services/db.rs](/data/nvme0/can/Projects/solo/plinth/crates/server/src/services/db.rs).
- [crates/server/Cargo.toml](/data/nvme0/can/Projects/solo/plinth/crates/server/Cargo.toml).
- `crates/server/tests/portfolio_publish.rs` (new).
- `crates/cli/src/commands/portfolio.rs` (new).
- [crates/cli/src/commands/mod.rs](/data/nvme0/can/Projects/solo/plinth/crates/cli/src/commands/mod.rs).
- [crates/cli/src/main.rs](/data/nvme0/can/Projects/solo/plinth/crates/cli/src/main.rs).
- [crates/cli/Cargo.toml](/data/nvme0/can/Projects/solo/plinth/crates/cli/Cargo.toml).
- [Cargo.toml](/data/nvme0/can/Projects/solo/plinth/Cargo.toml) (workspace + leptos metadata).
- [AGENTS.md](/data/nvme0/can/Projects/solo/plinth/AGENTS.md).

## Pitfalls

- **Forgetting one Cargo.toml feature gate.** Per
  [AGENTS.md:55-61](/data/nvme0/can/Projects/solo/plinth/AGENTS.md#L55-L61),
  bricks span 4 Cargo.toml files. Missing any one breaks the
  no-default-features build. Run the bricks-disabled `cargo check`
  before considering the phase done.
- **`reqwest::Client::new()` in test paths.** Sandbox-illegal per
  [AGENTS.md:67](/data/nvme0/can/Projects/solo/plinth/AGENTS.md#L67).
  If the new code needs an HTTP client for tests, use
  `Client::builder().build()` and handle errors.
- **Raw string literals.** The seed-data path at
  [crates/server/src/services/db.rs:60](/data/nvme0/can/Projects/solo/plinth/crates/server/src/services/db.rs#L60)
  uses `r##"..."##`. Match that pattern in any new SQL with embedded
  `"#`.
- **Skipping migration.** No new migration is needed — the
  `portfolio_items` table from
  [migrations/0004_portfolio.sql](/data/nvme0/can/Projects/solo/plinth/crates/server/migrations/0004_portfolio.sql)
  already has every column required. Do not add `0006_*.sql` unless a
  genuinely new column appears.
- **Slug collisions vs upsert semantics.** Decide explicitly: this plan
  prescribes upsert on `slug`. Make the SQL `INSERT ... ON CONFLICT
  (slug) DO UPDATE SET ...` and write a test for the second-publish
  case.
- **Cache staleness.** After insert, send `InvalidateAll` to the
  `PortfolioCache` actor. Missing this means the new entry doesn't
  appear until the process restarts or the next single-item miss.
- **`brick-portfolio` already on by default.** The default feature set
  in `Cargo.toml` already includes all bricks per
  [AGENTS.md:43](/data/nvme0/can/Projects/solo/plinth/AGENTS.md#L43);
  the new code paths are *additive*, not *opt-in*.

## Reference

- Research dossier: [../website-wiring-research.md](../website-wiring-research.md) —
  "plinth portfolio brick — fully implemented in code, zero publish path".
- Plinth conventions: [AGENTS.md](/data/nvme0/can/Projects/solo/plinth/AGENTS.md).
- Existing publish flow (article side): [crates/cli/src/commands/publish.rs](/data/nvme0/can/Projects/solo/plinth/crates/cli/src/commands/publish.rs).
- Portfolio brick: [crates/server/src/bricks/portfolio/](/data/nvme0/can/Projects/solo/plinth/crates/server/src/bricks/portfolio/).
