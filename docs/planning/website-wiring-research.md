# Website wiring + crosscutting extraction research dossier

## Goal And Trigger

The user asked to "get the website wired up (see `website/`)" with two
crosscutting layers attached:

1. Generic Zola/mdBook/site/pages plumbing should land in
   [canix-toolbelt](/data/nvme0/can/Projects/canix-toolbelt) so other tools
   can reuse it (rs-harbor already has a near-duplicate copy in
   [`rs-harbor/nix/site.nix`](/data/nvme0/can/Projects/rs-harbor/nix/site.nix)).
2. Once modde has a presentation site, modde should appear in a
   *portfolio section* of [plinth](/data/nvme0/can/Projects/solo/plinth).
   That portfolio surface should itself be generic — adding the next tool
   should follow the same pattern as adding modde.

Trigger: maintainer wants to ship a publishable landing page for modde
and start building a personal portfolio that scales beyond a single tool.

## Current Reality

### rs-modde website — fully scaffolded, not yet served at the target URL

- [website/](/data/nvme0/can/Projects/rs-modde/website/) holds a working
  Zola tree with custom templates (no theme):
  - [website/config.toml](/data/nvme0/can/Projects/rs-modde/website/config.toml#L5) sets
    `base_url = "https://modde.rs"`, `compile_sass = true`, no feeds, no search index.
  - [website/templates/base.html](/data/nvme0/can/Projects/rs-modde/website/templates/base.html) renders the
    chrome (top nav, footer, links to `#install`, `#supported-games`, `/docs/`,
    `/comparison/`, the Codeberg repo).
  - [website/templates/index.html](/data/nvme0/can/Projects/rs-modde/website/templates/index.html) reads
    `section.extra.{logo,tagline,subtitle,primary_cta,secondary_cta,features[]}`
    from [website/content/_index.md](/data/nvme0/can/Projects/rs-modde/website/content/_index.md), then
    includes [website/templates/shortcodes/games.html](/data/nvme0/can/Projects/rs-modde/website/templates/shortcodes/games.html#L1)
    which calls `load_data(path="data/capability-matrix.toml", format="toml")`.
  - [website/templates/comparison.html](/data/nvme0/can/Projects/rs-modde/website/templates/comparison.html)
    + [website/content/comparison.md](/data/nvme0/can/Projects/rs-modde/website/content/comparison.md)
    render the MO2 comparison page.
  - [website/sass/style.scss](/data/nvme0/can/Projects/rs-modde/website/sass/style.scss) is a
    552-line single-file stylesheet using a dark palette.
  - [website/static/](/data/nvme0/can/Projects/rs-modde/website/static/) has only
    `logo.{png,svg}` — no `screenshots/` directory yet, although
    [website/content/_index.md](/data/nvme0/can/Projects/rs-modde/website/content/_index.md) reserves
    three placeholder slots under `/screenshots/`.
- [flake.nix](/data/nvme0/can/Projects/rs-modde/flake.nix#L150-L182) already
  builds the `website`, `docs` (Zola + AdiDoks), and combined `site`
  derivation. `site/` writes
  [`.domains` listing `modde.rs www.modde.rs`](/data/nvme0/can/Projects/rs-modde/flake.nix#L181),
  which is the Codeberg-Pages custom-domain mechanism.
- [scripts/deploy-pages.sh](/data/nvme0/can/Projects/rs-modde/scripts/deploy-pages.sh)
  publishes `#site` to the `pages` branch of `DEPLOY_REMOTE`, force-pushed.
  It's exposed via `apps.deploy-pages` in
  [flake.nix](/data/nvme0/can/Projects/rs-modde/flake.nix#L1292-L1301).
- [.forgejo/workflows/pages.yml](/data/nvme0/can/Projects/rs-modde/.forgejo/workflows/pages.yml)
  runs that app on every push to `trunk`, on the self-hosted `atlas` runner,
  using `secrets.codeberg_token` to push as `caniko`. The workflow is
  wired; it just needs the secret to exist and the `pages` branch convention to
  match Codeberg's "Codeberg Pages" branch (the default `pages` branch
  pattern is what Codeberg looks for, so that part is fine).

### Domain decision is unresolved

- The website config says `modde.rs`
  ([website/config.toml:5](/data/nvme0/can/Projects/rs-modde/website/config.toml#L5),
  [website/content/_index.md:11](/data/nvme0/can/Projects/rs-modde/website/content/_index.md#L11)).
- The docs site says `modde.rs/docs`
  ([docs/site/config.toml:5](/data/nvme0/can/Projects/rs-modde/docs/site/config.toml#L5)).
- The Homebrew formula and release-integration plans say
  `modde.tartanoglu.com`
  ([flake.nix:480](/data/nvme0/can/Projects/rs-modde/flake.nix#L480),
  [flake.nix:1360](/data/nvme0/can/Projects/rs-modde/flake.nix#L1360),
  [docs/planning/apt-channel-bootstrap/README.md:71](/data/nvme0/can/Projects/rs-modde/docs/planning/apt-channel-bootstrap/README.md#L71),
  [docs/planning/release-integration-audit/07-crates-io-publish.md:55](/data/nvme0/can/Projects/rs-modde/docs/planning/release-integration-audit/07-crates-io-publish.md#L55)).
- The APT bootstrap plan explicitly leaves URL-choice A
  (`caniko.codeberg.page/rs-modde-apt/`) vs URL-choice B
  (`caniko.codeberg.page/rs-modde-apt/`) as a still-open decision recorded in
  [docs/planning/apt-channel-bootstrap/01-codeberg-pages-bootstrap.md:115-118](/data/nvme0/can/Projects/rs-modde/docs/planning/apt-channel-bootstrap/01-codeberg-pages-bootstrap.md#L115-L118).

So three target URLs are live in tree at once: `modde.rs`,
`modde.tartanoglu.com`, and the canonical
`caniko.codeberg.page/rs-modde/`. Until one is picked, the deploy script's
final `echo` advertises the Codeberg canonical URL while the rendered
HTML emits `modde.rs`-prefixed links.

### Build path uses an in-tree copy of `capability-matrix.toml`

The website build in
[flake.nix:151-173](/data/nvme0/can/Projects/rs-modde/flake.nix#L151-L173)
explicitly copies
[docs/capability-matrix.toml](/data/nvme0/can/Projects/rs-modde/docs/capability-matrix.toml)
into `site/data/capability-matrix.toml` before invoking `zola build`. This
works in the Nix derivation, but `zola serve` in
[website/](/data/nvme0/can/Projects/rs-modde/website/) on its own will
fail the `load_data(...)` call at
[website/templates/shortcodes/games.html:1](/data/nvme0/can/Projects/rs-modde/website/templates/shortcodes/games.html#L1)
because `data/capability-matrix.toml` doesn't exist relative to the Zola
root. Local dev currently needs a manual symlink or copy.

### canix-toolbelt has no site/zola/pages helpers today

[canix-toolbelt/lib/default.nix](/data/nvme0/can/Projects/canix-toolbelt/lib/default.nix)
exposes: `agenixPaths, caddy, deviceTypes, dns, facterDisks, gpu,
mkPkgs, networkmanager, nexus, opsShellPackages, peerRoute, rbac,
sshAliases, storage`. None of them touch static-site builders.

[canix-toolbelt/flake-modules/](/data/nvme0/can/Projects/canix-toolbelt/flake-modules/)
exposes: `agenix-rekey-auto, caddy-helpers, dev-stack, formatters,
git-hooks, ops-shell, shebang-audit, structure-check, topology`. No
pages/site flake-module either.

[canix-toolbelt/README.md:1-15](/data/nvme0/can/Projects/canix-toolbelt/README.md#L1-L15)
states scope: "hyper-stable parts ... modules and helpers that have low
churn, no host/secret coupling, and are useful to other NixOS users",
with output families `nixosModules.*` and `flakeModules.*`. Zola/mdBook
site builders fit the stability claim and the host-agnostic claim, but
they expand the lib surface beyond NixOS-flavored helpers — a deliberate
scope decision.

### rs-harbor already has a duplicate of the helper

[rs-harbor/nix/site.nix](/data/nvme0/can/Projects/rs-harbor/nix/site.nix)
is a 44-line file that defines `website` (Zola), `docs` (mdBook), and a
combined `site` derivation. It is structurally identical to the
hand-rolled code in
[rs-modde/flake.nix:128-182](/data/nvme0/can/Projects/rs-modde/flake.nix#L128-L182).
Differences:

- rs-harbor uses pure mdBook for docs; rs-modde uses Zola with the
  AdiDoks theme injected at build time
  ([flake.nix:128-148](/data/nvme0/can/Projects/rs-modde/flake.nix#L128-L148)).
- rs-modde injects an extra `capability-matrix.toml` data file into the
  Zola tree
  ([flake.nix:163-167](/data/nvme0/can/Projects/rs-modde/flake.nix#L163-L167)).
- rs-modde writes the Codeberg-Pages `.domains` file into the combined
  `site` output
  ([flake.nix:181](/data/nvme0/can/Projects/rs-modde/flake.nix#L181));
  rs-harbor's site has no domain file.

Any extracted helper must accept those three knobs (custom Zola theme
fileset injection, data-file injection, optional `.domains`/CNAME file)
or each consumer will still need its own bespoke wrapper.

### plinth portfolio brick — fully implemented in code, zero publish path

- The portfolio brick is feature-gated by `brick-portfolio`
  ([plinth/AGENTS.md:32-43](/data/nvme0/can/Projects/solo/plinth/AGENTS.md#L32-L43)).
- Server brick: [crates/server/src/bricks/portfolio/](/data/nvme0/can/Projects/solo/plinth/crates/server/src/bricks/portfolio/) — `mod.rs`, `cache.rs`, `migrations.rs`.
- Migration: [crates/server/migrations/0004_portfolio.sql](/data/nvme0/can/Projects/solo/plinth/crates/server/migrations/0004_portfolio.sql)
  defines `portfolio_items(slug, title, description, content, html_content,
  tech_stack TEXT[], link, demo, image_url, date, featured, "order")`.
- Shared type: [crates/shared/src/portfolio_item.rs](/data/nvme0/can/Projects/solo/plinth/crates/shared/src/portfolio_item.rs)
  defines `PortfolioItem` with a `slugify` helper.
- Client page: [crates/client/src/pages/portfolio.rs](/data/nvme0/can/Projects/solo/plinth/crates/client/src/pages/portfolio.rs)
  + `portfolio_detail.rs` already render an items grid driven by
  `api::get_portfolio_items()`.
- RSS feed: [crates/server/src/api/feeds.rs:109-118](/data/nvme0/can/Projects/solo/plinth/crates/server/src/api/feeds.rs#L109-L118)
  ships `GetAllPortfolioItems` through a `/feeds/projects.xml` endpoint.
- Cache actor: [crates/server/src/bricks/portfolio/cache.rs](/data/nvme0/can/Projects/solo/plinth/crates/server/src/bricks/portfolio/cache.rs)
  caches single + list reads, supports invalidation.
- DB writer:
  [crates/server/src/services/db.rs:82-107](/data/nvme0/can/Projects/solo/plinth/crates/server/src/services/db.rs#L82-L107)
  contains the only `INSERT INTO portfolio_items` path — and it lives
  inside seed data, gated on `#[cfg(feature = "brick-portfolio")]`,
  inserting one hard-coded "sample-project" row.
- CLI: [crates/cli/src/commands/](/data/nvme0/can/Projects/solo/plinth/crates/cli/src/commands/)
  ships `check_config, completions, content, init, publish, status,
  tags, todo`. There is no `portfolio` subcommand and the
  `publish` command in
  [crates/cli/src/commands/publish.rs](/data/nvme0/can/Projects/solo/plinth/crates/cli/src/commands/publish.rs)
  only routes blog articles into `PublishArticleRequest`.
- The empty server function file
  [crates/server/src/server_fns/portfolio.rs:1](/data/nvme0/can/Projects/solo/plinth/crates/server/src/server_fns/portfolio.rs#L1)
  has a comment confirming portfolio reads moved to the client API, but
  *no write endpoint exists at all*. There is no admin route that lets a
  CLI POST a new portfolio item.

Net: a deployed plinth instance can read & cache portfolio items, render
them, syndicate them via RSS — but cannot create them except by direct
SQL or by editing the seed insert. That's the gap behind "doesn't exist
yet."

### plinth site-config already advertises a `/projects` nav link

[plinth.toml:30-32](/data/nvme0/can/Projects/solo/plinth/plinth.toml#L30-L32)
already lists "Projects" in `[[site.nav]]`, and
[plinth.toml:51-55](/data/nvme0/can/Projects/solo/plinth/plinth.toml#L51-L55)
has the `pages.portfolio` block with title/subtitle/description. So
adding items to the deployed instance is the only missing link on the
plinth side.

## Evidence Inventory

| Artifact | What it proves |
|---|---|
| [rs-modde/website/](/data/nvme0/can/Projects/rs-modde/website/) tree | Content + templates + sass already exist; no theme dependency. |
| [rs-modde/flake.nix:128-182](/data/nvme0/can/Projects/rs-modde/flake.nix#L128-L182) | Build path for `website`, `docs`, `site`; in-tree (not extracted) Zola/mdBook helpers; `.domains` file generation. |
| [rs-modde/scripts/deploy-pages.sh](/data/nvme0/can/Projects/rs-modde/scripts/deploy-pages.sh) | Codeberg-Pages-orphan-branch deploy mechanism, force-push, sandbox-safe. |
| [rs-modde/.forgejo/workflows/pages.yml](/data/nvme0/can/Projects/rs-modde/.forgejo/workflows/pages.yml) | CI deploy is wired on `push:trunk`, depends on `secrets.codeberg_token`, runs on `atlas`. |
| `rg "modde.rs|modde.tartanoglu.com"` output (above) | Two competing canonical hosts in tree; APT plan keeps the choice deliberately open. |
| [rs-harbor/nix/site.nix](/data/nvme0/can/Projects/rs-harbor/nix/site.nix) | Confirms the duplication: rs-harbor already has its own Zola+mdBook+site shim. |
| [canix-toolbelt/lib/default.nix](/data/nvme0/can/Projects/canix-toolbelt/lib/default.nix), [canix-toolbelt/flake-modules/](/data/nvme0/can/Projects/canix-toolbelt/flake-modules/) | Toolbelt has no zola/mdbook/pages helper today; scope expansion needed. |
| [canix-toolbelt/README.md](/data/nvme0/can/Projects/canix-toolbelt/README.md#L1-L80) | Confirms current scope is NixOS + flake-parts modules; adding a site helper means a deliberate scope decision. |
| [plinth/crates/server/src/bricks/portfolio/](/data/nvme0/can/Projects/solo/plinth/crates/server/src/bricks/portfolio/) + [migrations/0004_portfolio.sql](/data/nvme0/can/Projects/solo/plinth/crates/server/migrations/0004_portfolio.sql) | Portfolio brick is fully implemented up to the read path. |
| [plinth/crates/server/src/services/db.rs:82-107](/data/nvme0/can/Projects/solo/plinth/crates/server/src/services/db.rs#L82-L107) | The only existing portfolio write is a seed-data INSERT; no admin/CLI path. |
| [plinth/crates/cli/src/commands/](/data/nvme0/can/Projects/solo/plinth/crates/cli/src/commands/) | No `portfolio` subcommand; `publish` only handles articles. |
| [plinth.toml:30-55](/data/nvme0/can/Projects/solo/plinth/plinth.toml#L30-L55) | Plinth's deployed config already declares `/projects` in nav + `pages.portfolio` copy. |
| [canix/docs/planning/canix-toolbelt-extraction/README.md](/data/nvme0/can/Projects/canix/docs/planning/canix-toolbelt-extraction/README.md) | Active multi-phase plan for what *else* is moving to canix-toolbelt — useful for sequencing. |

Commands run during research:

- `ls`, `cat`, and `find` against `rs-modde/{website,docs,scripts,.forgejo}`,
  `canix-toolbelt/{lib,flake-modules,modules}`,
  `rs-harbor/{lib,nix}`, `solo/plinth/{crates,public,modules}`.
- `rg -ln "modde.rs|modde.tartanoglu.com|caniko.codeberg.page"` across
  the rs-modde tree to surface the domain inconsistency.
- `rg -n "portfolio|PortfolioItem"` across plinth crates to map the
  existing portfolio surface end-to-end.
- `rg -ln "zola|mdBook"` across rs-harbor + canix-toolbelt to confirm
  duplication.

Nothing was built or executed — research only.

## Existing Plan Status

No prior planning docs cover this task directly. Adjacent docs that
touch it:

| Doc | Item | Status against current evidence |
|---|---|---|
| [docs/planning/apt-channel-bootstrap/01-codeberg-pages-bootstrap.md:115-118](/data/nvme0/can/Projects/rs-modde/docs/planning/apt-channel-bootstrap/01-codeberg-pages-bootstrap.md#L115-L118) | URL choice A vs B (`caniko.codeberg.page/...` vs `modde.tartanoglu.com/...`) | **not-started** — both checkboxes are blank in tree; the website ships a third URL (`modde.rs`) that this plan does not reference. Resolving the website domain now forces this decision. |
| [docs/planning/canix-toolbelt-extraction/](/data/nvme0/can/Projects/canix/docs/planning/canix-toolbelt-extraction/) (in canix) | Phases 01–09 extract NixOS + flake-parts helpers into canix-toolbelt | **partial / in-flight** — none of the phases cover Zola/mdBook/pages helpers; the proposed extraction here is **additive** to that plan, not a duplicate. |
| [docs/planning/release-integration-audit/audit-report.md:139-150](/data/nvme0/can/Projects/rs-modde/docs/planning/release-integration-audit/audit-report.md#L139-L150) | "Pages serves at https://caniko.codeberg.page/rs-modde-apt/" | **stale / contradicted** by website choice of `modde.rs`. Need a single source of truth. |
| [docs/planning/release-integration-audit/07-crates-io-publish.md:55](/data/nvme0/can/Projects/rs-modde/docs/planning/release-integration-audit/07-crates-io-publish.md#L55) | Cargo metadata `homepage = "https://modde.tartanoglu.com"` | **stale / contradicted** by website choice of `modde.rs`. |

There is no existing plan for the plinth portfolio surface.

## Work That Should Survive

- Custom Zola templates in
  [rs-modde/website/templates/](/data/nvme0/can/Projects/rs-modde/website/templates/)
  and the
  [website/content/](/data/nvme0/can/Projects/rs-modde/website/content/)
  copy — they are well-written, modde-specific, and should not be
  generalized into the toolbelt.
- The combined `site` derivation pattern with `.domains` (Codeberg's
  custom-domain hook) — generalize this as a parameter in the toolbelt
  helper.
- The `scripts/deploy-pages.sh` + `apps.deploy-pages` pattern — strong
  candidate to become `canix-toolbelt.lib.mkDeployPagesApp { ... }` or a
  flake-module with options.
- The `.forgejo/workflows/pages.yml` pattern (atlas runner, attic
  substituter, `secrets.codeberg_token`, `concurrency.cancel-in-progress`)
   — strong candidate to become a documented snippet, optionally a
   generator inside `forgejo-pages`/`forgejo-site` skills if those don't
   already emit this exact shape.
- The capability-matrix data injection pattern (auxiliary data files
  pulled into a Zola tree at build time) — should be a parameter, not a
  one-off in modde's flake.

Durable constraints to preserve:

- canix-toolbelt may not import crossbow and is "no host/secret
  coupling" — Codeberg-token handling stays in the CI workflow, not in
  the helper.
- All canix-flake-family extractions land directly on the target flake's
  default branch — no PRs for our own flakes
  ([canix-toolbelt-extraction/README.md:128-133](/data/nvme0/can/Projects/canix/docs/planning/canix-toolbelt-extraction/README.md#L128-L133)).
- modde versions ship as semver with major-anytime (auto-memory:
  `project_versioning_policy.md`).
- Release workflow uses `simit`, not hand-rolled cargo+git+sed
  (auto-memory: `feedback_use_simit.md`) — relevant for any release-bump
  needed to publish portfolio entries.

## Blockers And Missing Artifacts

| Item | Producer | How to regenerate / decide | Validation |
|---|---|---|---|
| Canonical public hostname for modde (`modde.rs` vs `modde.tartanoglu.com` vs `caniko.codeberg.page/rs-modde/`) | maintainer decision | Pick one; update [website/config.toml](/data/nvme0/can/Projects/rs-modde/website/config.toml), [docs/site/config.toml](/data/nvme0/can/Projects/rs-modde/docs/site/config.toml), [flake.nix](/data/nvme0/can/Projects/rs-modde/flake.nix#L181) `.domains`, the Homebrew formula, the APT plan, and release-integration audit refs. | `rg "modde\\.(rs|tartanoglu\\.com)\|caniko\\.codeberg\\.page"` collapses to a single canonical host plus deliberate `caniko.codeberg.page` references only in the deploy-script echo. |
| DNS for the chosen apex (whichever wins) | maintainer / DNS provider | Configure A/AAAA + CNAME per Codeberg-Pages docs; verify TXT verification record is present so Codeberg auto-issues TLS. | `curl -fsSI https://<host>/ \| head -5` returns 200 from Codeberg Pages. |
| `secrets.codeberg_token` exists on the rs-modde Forgejo repo | maintainer | Create a Codeberg PAT scoped to repo write, store as `codeberg_token` in repo secrets. | First `pages` workflow run on `trunk` push succeeds; `pages` branch is updated. |
| `website/static/screenshots/` content | maintainer | Drop real PNG/GIF assets into [website/static/screenshots/](/data/nvme0/can/Projects/rs-modde/website/static/) per the slot comments in [content/_index.md](/data/nvme0/can/Projects/rs-modde/website/content/_index.md). | The placeholder `<figcaption>` blocks switch to real `<img>` tags. |
| `data/capability-matrix.toml` is reachable at `zola serve` time | local-dev workflow | Either symlink `website/data → ../docs` or add a `just website-serve` recipe that copies the file before invoking `zola serve`. | `cd website && zola serve` renders the supported-games table without errors. |
| Decision: does the Zola/mdBook helper actually belong in canix-toolbelt, or in a new dedicated flake (e.g. `canix-sites`)? | maintainer | Re-read [canix-toolbelt/README.md](/data/nvme0/can/Projects/canix-toolbelt/README.md) scope statement and decide whether to broaden it. | Whichever flake gets the helper exposes `lib.mkZolaSite`, `lib.mkMdBookDocs`, `lib.mkCombinedSite`, `lib.mkDeployPagesApp`. |
| Decision: portfolio-item publish surface in plinth (CLI subcommand vs admin REST vs static TOML/markdown manifests in-tree) | maintainer | Pick a model that matches plinth's existing publish flow (article CLI → API → server insert). | A new tool can be added to the portfolio without touching plinth's source. |
| Decision: portfolio item content shape for "tool" entries (modde-shaped vs general project-shaped) | maintainer | Confirm the `PortfolioItem` schema covers the tool case or extend it. | modde fits cleanly into the schema and the next tool follows the same path. |

## Risks And Constraints

- **Domain churn cost.** Several non-website artifacts already publish
  `modde.tartanoglu.com`: the Homebrew formula
  ([flake.nix:480](/data/nvme0/can/Projects/rs-modde/flake.nix#L480)), the
  simit homebrew block
  ([flake.nix:1360](/data/nvme0/can/Projects/rs-modde/flake.nix#L1360)),
  and the audit/APT plans. Switching the canonical host has a non-trivial
  blast radius across crates.io metadata, Homebrew tap, and any printed
  install instructions. Picking `modde.rs` for the website while
  keeping `modde.tartanoglu.com` as the project canonical is *possible*
  but creates a long-term split-brain UX.
- **Codeberg Pages multi-domain quirks.** The `.domains` file lists both
  `modde.rs` and `www.modde.rs`. Codeberg-Pages honors only verified
  domains; both need DNS TXT verification and at least one needs to be
  picked as the apex for canonical redirects. Without that, browsers see
  mixed-host cookies.
- **rs-harbor duplication.** If the helper lands in canix-toolbelt,
  rs-harbor's
  [nix/site.nix](/data/nvme0/can/Projects/rs-harbor/nix/site.nix) should
  be deleted in the same merge that flips rs-harbor's flake input — but
  rs-harbor is *consumed by rs-modde* (flake.nix:14). The migration
  ordering is:
  toolbelt → rs-harbor → rs-modde (with a flake input bump at each
  step). Without that ordering, rs-modde's `flake.lock` will drag in two
  duplicate helpers.
- **Scope expansion for canix-toolbelt.** The toolbelt's stated identity
  is NixOS + flake-parts modules. Static-site builders are a different
  flavor of helper. There is a real risk of bloating the toolbelt's
  scope. A clean alternative is a sibling flake `canix-sites` (or
  `canix-toolbelt-sites`) with the same publishing conventions. This is
  a deliberate decision, not a default.
- **Plinth feature-flag drift.** If we add a `portfolio publish` CLI
  subcommand, the CLI crate's
  [Cargo.toml](/data/nvme0/can/Projects/solo/plinth/crates/cli/Cargo.toml)
  feature gates must align with `brick-portfolio`. Forgetting to gate it
  breaks `cargo check --no-default-features --features "ssr,brick-blog,brick-todo"`
  (per [AGENTS.md:50](/data/nvme0/can/Projects/solo/plinth/AGENTS.md#L50)).
- **Schema migration risk in plinth.** Adding a writer requires nothing
  schema-side — table already exists at
  [migrations/0004_portfolio.sql](/data/nvme0/can/Projects/solo/plinth/crates/server/migrations/0004_portfolio.sql).
  But the next migration would need to be `0006_*.sql`
  (counting `0005_todo.sql`); whatever sequence assumptions the brick
  framework has must be respected.
- **No fastembed for portfolio items.** Blog publish uses fastembed at
  CLI side to embed; the portfolio item has no `embedding` column and no
  embedding pipeline. Stay consistent: portfolio items are skip-embed
  unless the maintainer explicitly wants vector search over them.
- **Sandbox-safe Nix in plinth.** Per
  [AGENTS.md:64-72](/data/nvme0/can/Projects/solo/plinth/AGENTS.md#L64-L72),
  fastembed must not be used in tests, and `reqwest::Client::new()`
  panics in the Nix sandbox. New portfolio publish code must follow the
  same rules.

## Candidate Next Steps

Ordered for dependency clarity; the maintainer drives sequencing.

1. **Pick canonical host for modde.** Decide between `modde.rs`,
   `modde.tartanoglu.com`, or canonical `caniko.codeberg.page/rs-modde`.
   Single decision, blocks everything downstream. *Blocking — must
   land before any deploy verification has meaning.*
2. **Reconcile in-tree references.** Sweep
   [website/config.toml](/data/nvme0/can/Projects/rs-modde/website/config.toml),
   [website/content/_index.md](/data/nvme0/can/Projects/rs-modde/website/content/_index.md),
   [docs/site/config.toml](/data/nvme0/can/Projects/rs-modde/docs/site/config.toml),
   [flake.nix](/data/nvme0/can/Projects/rs-modde/flake.nix) (`.domains`
   writer + Homebrew + simit blocks), and the
   `docs/planning/apt-channel-bootstrap/` + `release-integration-audit/`
   docs that still say `modde.tartanoglu.com`. *Depends on (1).*
3. **DNS + Codeberg Pages verification for the chosen host.** Set TXT
   verification + A/AAAA, confirm Codeberg auto-issues TLS, confirm
   `https://<host>/` returns the built `site` output. *Depends on (2).*
4. **Ensure `secrets.codeberg_token` exists on the rs-modde Forgejo
   mirror** and trigger the first `pages.yml` run by pushing to
   `trunk`. *Depends on (3); independent of the canix-toolbelt extraction.*
5. **Fix local-dev `zola serve` UX.** Either add a small `justfile`
   recipe that copies/symlinks `docs/capability-matrix.toml` into
   `website/data/`, or move the matrix into the website tree. Avoids
   landing-page contributors having to run `nix build .#website`
   for the games table to render locally. *Independent.*
6. **Add real `website/static/screenshots/` content.** Trivial follow-up
   once captures exist. *Independent.*
7. **Decide canix-toolbelt vs a sibling flake for the Zola helpers.**
   Either land helpers in
   [canix-toolbelt/lib/](/data/nvme0/can/Projects/canix-toolbelt/lib/)
   (and add `canix-toolbelt.flakeModules.pages-deploy` for the CI
   workflow + deploy app), or stand up a new `canix-sites` flake. The
   helpers to extract are: `mkZolaSite { themeSrc?, dataFiles?, ... }`,
   `mkMdBookDocs`, `mkCombinedSite { website, docs, domains? }`,
   `mkDeployPagesApp { remote?, branch?, commitMessage? }`. *Depends on
   nothing in tree, but coordinates with the canix-toolbelt-extraction
   plan in canix.*
8. **Migrate rs-harbor to consume the helper** and delete
   [rs-harbor/nix/site.nix](/data/nvme0/can/Projects/rs-harbor/nix/site.nix).
   Same-commit rule per canix's extraction global constraints. *Depends
   on (7).*
9. **Migrate rs-modde to consume the helper** and delete the in-tree
   Zola/mdBook/site/deploy-pages duplication from
   [flake.nix](/data/nvme0/can/Projects/rs-modde/flake.nix#L128-L182)
   + [scripts/deploy-pages.sh](/data/nvme0/can/Projects/rs-modde/scripts/deploy-pages.sh).
   Run `nix flake check`. *Depends on (7) and ideally (8) so the
   flake.lock bump is single-step.*
10. **Design plinth portfolio publish surface.** Two viable shapes:
    - *(a) CLI + admin API.* Mirror the blog flow: `plinth-cli
      portfolio publish path/to/modde.toml` → POST `/api/admin/portfolio`
      → DB insert + cache invalidate. Per-tool content lives in a small
      TOML/markdown file owned by each tool's repo.
    - *(b) Static manifests imported at boot.* Plinth reads
      `portfolio.d/*.toml` at startup and upserts into the DB. Tool
      repos contribute their manifest to a single registry repo. Easier
      to "generic" but loses the cache-invalidation story plinth already
      built for blog posts.
    Recommend (a) — it reuses the existing publish/cache-invalidate
    primitives and matches how blog publishing already works.
    *Depends on nothing structurally; just a design decision.*
11. **Land the chosen plinth portfolio publish surface.** Wire the
    server admin handler, the CLI subcommand, and feature-gate it on
    `brick-portfolio`. *Depends on (10).*
12. **Publish modde as the first portfolio entry.** Author
    `website/portfolio.toml` (or wherever the contract lands) in rs-modde
    with `slug = "modde"`, `tech_stack = ["Rust", "Leptos-free", "Nix",
    "Wabbajack"]`, link to the website, image to the modde logo. Run the
    new CLI command against the deployed plinth instance.
    *Depends on (11) and (3).*
13. **Document the "add a tool to my portfolio" runbook** in
    [plinth/AGENTS.md](/data/nvme0/can/Projects/solo/plinth/AGENTS.md)
    so the next tool gets to repeat step (12) without reading code.
    *Depends on (12).*

Parallelism hints:

- Steps 1–6 form one chain (website-side).
- Steps 7–9 form a second chain (canix-toolbelt extraction).
- Steps 10–13 form a third chain (plinth portfolio).
- Chains 1 and 3 converge at step 12 (modde portfolio entry needs the
  canonical host).
- Chain 2 is fully independent of 1 and 3 until step 9.

## Open Decisions For The User

1. **Canonical host.** `modde.rs` (new, slick, requires DNS), or
   `modde.tartanoglu.com` (already referenced by Homebrew/APT/audit
   docs, needs the same DNS work), or accept the
   `caniko.codeberg.page/rs-modde/` canonical and treat the friendly
   names as redirects. This decision propagates everywhere — it should
   be made first.
2. **Home for the Zola/mdBook helpers.** Add to canix-toolbelt and
   accept the scope broadening, or stand up a sibling
   `canix-sites`/`canix-toolbelt-sites` flake. Maintainer is on record
   that canix-toolbelt should be "hyper-stable" + NixOS-flavored, so the
   sibling-flake option is the lower-risk path.
3. **Plinth portfolio publish surface shape.** CLI + admin API (mirrors
   blog flow) vs static manifests at boot. Recommendation in step 10
   above leans CLI-based, but the static path is genuinely simpler if
   modde is going to be one of a half-dozen items and the cache-invalidation
   story doesn't matter for that scale.
4. **Whether modde's own website should embed a "portfolio" backlink to
   plinth.** Out of scope for "wire up the website" but worth thinking
   about now: if the plinth portfolio is canonical, modde's site footer
   could add `Made by <a href="https://plinth-host/projects/modde">…</a>`
   for navigability.
