# Website wiring + canix-toolbelt extraction + plinth portfolio

> **Recommended Codex model for plan-set orchestration: GPT 5.5 high**
>
> This plan coordinates eight phases across three flakes (rs-modde,
> canix-toolbelt, rs-harbor) plus one Leptos-side server (plinth). Two
> phases are tier `5.5 high` (helper extraction and plinth publish
> surface); the rest are mechanical or routine. Orchestration risk is
> ordering the canix-toolbelt extraction so each downstream repo bumps a
> single flake.lock — not raw output volume.

Source: [website-wiring-research.md](../website-wiring-research.md).

## Scope

Land a publishable modde landing page at the chosen canonical host,
extract the duplicated Zola/mdBook/site/pages plumbing into
canix-toolbelt so rs-harbor and rs-modde both consume it, and add a
generic portfolio-publish surface to plinth so modde (and the next tool)
can be listed under `/projects`.

## Decisions baked in

The research dossier left three decisions open; for this plan set the
reasonable defaults are committed so phases can be executed
unsupervised. The maintainer can override mid-execution by editing the
relevant phase doc.

1. **Canonical host = `modde.rs`** (with `www.modde.rs` as the alt). The
   site already advertises it; the change forces a sweep of stale
   `modde.tartanoglu.com` references in Homebrew metadata + APT plans.
2. **Helper home = canix-toolbelt** (user's explicit direction). The
   `lib.mk{ZolaSite,MdBookDocs,CombinedSite,DeployPagesApp}` helpers and
   a `flakeModules.pages-deploy` flake-parts module land in
   canix-toolbelt's existing lib + flake-modules trees.
3. **Plinth portfolio publish = CLI + admin API** mirroring the blog
   publish flow. Per-tool entries are authored as a TOML in the tool's
   own repo (e.g., `rs-modde/website/portfolio.toml`).

## Phase table

| Phase | File | Depends on | Touches | Model | Blocking? |
|---|---|---|---|---|---|
| 01 | [01-host-reconciliation.md](./01-host-reconciliation.md) | — | rs-modde: website, docs/site, flake.nix, docs/planning/{apt-channel-bootstrap,release-integration-audit} | `5.5 medium` | unlocks 03 |
| 02 | [02-local-dev-polish.md](./02-local-dev-polish.md) | — | rs-modde: justfile, website/static/screenshots/ | `5.5 low` | independent |
| 03 | [03-dns-and-first-deploy.md](./03-dns-and-first-deploy.md) | 01 | external DNS, Codeberg repo secrets, observation only | `5.5 medium` | terminal in chain A |
| 04 | [04-toolbelt-site-helpers.md](./04-toolbelt-site-helpers.md) | — | canix-toolbelt: lib/, flake-modules/, README.md | `5.5 high` | unlocks 05, 06 |
| 05 | [05-rs-harbor-migration.md](./05-rs-harbor-migration.md) | 04 | rs-harbor: flake.nix, flake.lock, nix/site.nix (delete) | `5.5 medium` | unlocks 06's lock bump |
| 06 | [06-rs-modde-migration.md](./06-rs-modde-migration.md) | 04 (ideally 05) | rs-modde: flake.nix, flake.lock, scripts/deploy-pages.sh, .forgejo/workflows/pages.yml | `5.5 medium` | terminal in chain B |
| 07 | [07-plinth-portfolio-publish.md](./07-plinth-portfolio-publish.md) | — | plinth: crates/{shared,server,cli}, migrations, AGENTS.md | `5.5 high` | unlocks 08 |
| 08 | [08-publish-modde-portfolio-entry.md](./08-publish-modde-portfolio-entry.md) | 03, 07 | rs-modde: website/portfolio.toml; plinth instance (data) | `5.5 low` | terminal in chain C |

## Parallelism Layer

### Wave 0 — start immediately from current tree

Three phases can fan out concurrently. They touch disjoint repos and do
not share files.

- **Phase 01** — host reconciliation across rs-modde tree.
- **Phase 02** — local-dev polish in rs-modde (`justfile` recipe +
  `screenshots/` directory).
- **Phase 04** — canix-toolbelt site helper extraction.
- **Phase 07** — plinth portfolio publish surface.

Phases 01 and 02 both touch rs-modde but never the same file; safe to
parallelise. Phases 04 and 07 touch disjoint repos.

### Wave 1 — chain converges on canonical host + helper

- **Phase 03** — DNS + first deploy verification. Starts as soon as
  Phase 01 has landed and the chosen host's strings are consistent.
- **Phase 05** — rs-harbor migration to the toolbelt helper. Starts as
  soon as Phase 04 lands a tagged version on canix-toolbelt's default
  branch.

### Wave 2 — second consumer migration

- **Phase 06** — rs-modde migration. Ideally runs after Phase 05 so the
  flake.lock bump is single-step; can run after Phase 04 alone if the
  maintainer accepts a two-step lock dance.

### Wave 3 — modde appears in portfolio

- **Phase 08** — author the portfolio manifest in rs-modde and run the
  new plinth CLI command. Gates on Phase 07 (publish path exists) and
  Phase 03 (canonical URL resolves so the portfolio entry can point at
  the real site).

## Whole-set acceptance criteria

- [ ] `rg "modde\\.tartanoglu\\.com"` returns zero hits in rs-modde
      outside historical/audit notes that have been explicitly retired.
- [ ] `https://modde.rs/` returns the rendered website with valid TLS,
      issued by Codeberg-Pages.
- [ ] `https://modde.rs/docs/` returns the AdiDoks docs site.
- [ ] `nix flake check` is green on canix-toolbelt, rs-harbor, and
      rs-modde.
- [ ] `rs-harbor/nix/site.nix` is deleted; rs-harbor consumes
      `canix-toolbelt.lib.mk{ZolaSite,MdBookDocs,CombinedSite}`.
- [ ] `rs-modde/scripts/deploy-pages.sh` is deleted; rs-modde consumes
      `canix-toolbelt.flakeModules.pages-deploy` (or the equivalent
      `mkDeployPagesApp`).
- [ ] Plinth's CLI exposes `plinth-cli portfolio publish <path>` (or
      equivalent), feature-gated on `brick-portfolio`, gated by the
      admin Bearer token.
- [ ] Plinth's deployed `/projects` page lists a modde entry whose link
      resolves to `https://modde.rs/`.
- [ ] [plinth/AGENTS.md](/data/nvme0/can/Projects/solo/plinth/AGENTS.md)
      gains a one-paragraph runbook on adding a new tool to the
      portfolio.

## Global constraints

- **No PRs for our own flakes.** rs-modde, rs-harbor, and canix-toolbelt
  commits land on default branches directly (matches the
  `canix-toolbelt-extraction` plan's posture).
- **No deprecation aliases.** When rs-harbor and rs-modde switch off
  their in-tree helpers, delete the old files in the same commit that
  flips the consumer.
- **`simit` for release motions** (auto-memory: `feedback_use_simit.md`)
  — any version bump needed by phase 06 or 07 goes through `simit
  release` / `simit commit`, not hand-rolled cargo+git+sed.
- **SemVer major-anytime** (auto-memory: `project_versioning_policy.md`)
  — phase 07 is free to land a portfolio CLI breaking change as a major
  bump in plinth if the schema requires it.
- **canix-toolbelt scope statement.** The toolbelt's README claims
  "hyper-stable parts ... modules and helpers that have low churn, no
  host/secret coupling." The site helpers fit; the deploy-pages helper
  must accept the Codeberg-Pages token via parameter, never bake it in.
- **Sandbox-safe Nix in plinth.** Per plinth's
  [AGENTS.md](/data/nvme0/can/Projects/solo/plinth/AGENTS.md#L64-L72),
  `reqwest::Client::new()` panics in the Nix sandbox and `fastembed`
  downloads at runtime. Phase 07 must not introduce either in test
  paths.

## External-repo coordination

| Phase | Working tree | Default branch | Maintainer surface |
|---|---|---|---|
| 01, 02, 03, 06 | `/data/nvme0/can/Projects/rs-modde` | `trunk` | self (codeberg `caniko/rs-modde`) |
| 04 | `/data/nvme0/can/Projects/canix-toolbelt` | check `git branch --show-current` (likely `trunk`) | self (codeberg `caniko/canix-toolbelt`) |
| 05 | `/data/nvme0/can/Projects/rs-harbor` | check | self (codeberg `caniko/rs-harbor`) |
| 07, 08 | `/data/nvme0/can/Projects/solo/plinth` | check | self (codeberg `caniko/plinth`) |

## Reference

- Research dossier: [website-wiring-research.md](../website-wiring-research.md).
- Plinth portfolio brick: [plinth/crates/server/src/bricks/portfolio/](/data/nvme0/can/Projects/solo/plinth/crates/server/src/bricks/portfolio/).
- canix-toolbelt scope: [canix-toolbelt/README.md](/data/nvme0/can/Projects/canix-toolbelt/README.md).
- Sister extraction plan: [canix/docs/planning/canix-toolbelt-extraction/](/data/nvme0/can/Projects/canix/docs/planning/canix-toolbelt-extraction/).
