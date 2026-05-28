# Phase 08 — Publish modde as the first portfolio entry + runbook

> **Recommended Codex model: GPT 5.5 low**
>
> Trivial mechanical work: author one TOML in rs-modde, run one CLI
> command against the deployed plinth instance, append a paragraph to
> plinth's AGENTS.md. No design content, no log interpretation, leaf
> node.

## Working tree

Primary: `/data/nvme0/can/Projects/rs-modde` on `trunk`. Secondary:
the deployed plinth instance (`PLINTH_API_URL` + `PLINTH_API_KEY`)
plus `/data/nvme0/can/Projects/solo/plinth` for the runbook edit.

## Goal

The deployed plinth `/projects` page lists a modde entry whose `link`
field points at `https://modde.rs/`. A short runbook in plinth's
AGENTS.md tells the next tool's maintainer how to do the same.

## Why this matters now

After Phases 03 and 07, both halves of the loop exist: a live website
to link to, and a publish surface to push an entry through. This phase
proves the loop closes end-to-end with the first real (non-seed) entry.

## Out of scope

- Adding more than one entry. Other tools can follow the same recipe.
- Designing how multiple entries are sorted/featured beyond what the
  schema's `order` and `featured` columns already provide.
- Setting up CI that auto-publishes the manifest on rs-modde release.
  That's a follow-up if the maintainer wants it.

## Plan

1. Author `rs-modde/website/portfolio.toml` with the fields plinth
   expects (cross-reference Phase 07's manifest schema). Suggested
   content:
   ```toml
   slug = "modde"
   title = "modde"
   description = "NixOS-native game mod manager — Wabbajack on Linux, no Windows VM."
   tech_stack = ["Rust", "Nix", "Wabbajack", "Leptos"]
   link = "https://modde.rs/"
   image_url = "https://modde.rs/logo.svg"
   date = "<YYYY-MM-DD of today>"
   featured = true
   order = 0

   content = """
   modde is a cross-platform game mod manager that runs Wabbajack
   modlists natively on Linux, with declarative NixOS / home-manager
   integration, save vaults, and conflict-graph analysis.
   """
   ```
2. Export credentials in the local shell:
   ```sh
   export PLINTH_API_URL=https://<plinth-host>
   export PLINTH_API_KEY=<the admin bearer token>
   ```
3. Run the new CLI command from rs-modde's root:
   ```sh
   plinth-cli portfolio publish website/portfolio.toml
   ```
4. Verify:
   - `curl -fsS "$PLINTH_API_URL/api/portfolio" | jq '.[] | select(.slug=="modde")'`
     returns the manifest content.
   - Visit `https://<plinth-host>/projects` in a browser; modde appears
     with the configured tech stack pills.
   - Click through to `/projects/modde`; the detail page renders.
5. Commit `website/portfolio.toml` to rs-modde — message: `Publish modde
   portfolio manifest`.
6. Switch to plinth's working tree and append to
   [plinth/AGENTS.md](/data/nvme0/can/Projects/solo/plinth/AGENTS.md) a
   one-paragraph "Adding a tool to the portfolio" runbook:
   - Tool ships a `portfolio.toml` in its repo (suggested path
     `website/portfolio.toml`).
   - Tool maintainer runs `plinth-cli portfolio publish
     website/portfolio.toml` with `PLINTH_API_URL` and `PLINTH_API_KEY`
     set.
   - Re-running the command upserts on `slug`.
7. Commit on plinth's default branch — message: `Document portfolio
   publishing runbook`.

## Acceptance criteria

- [ ] `rs-modde/website/portfolio.toml` exists and validates against
      Phase 07's schema.
- [ ] `GET /api/portfolio` on the deployed plinth returns an entry with
      `slug = "modde"` and `link = "https://modde.rs/"`.
- [ ] `/projects` on the deployed plinth renders a modde card.
- [ ] `/projects/modde` on the deployed plinth renders without error.
- [ ] plinth's AGENTS.md has a "Adding a tool to the portfolio"
      paragraph.
- [ ] Two commits: one on rs-modde (the manifest), one on plinth (the
      runbook).

## Files likely touched

- `rs-modde/website/portfolio.toml` (new).
- [plinth/AGENTS.md](/data/nvme0/can/Projects/solo/plinth/AGENTS.md).

## Pitfalls

- **Schema mismatch.** Phase 07 may have settled on slightly different
  field names (e.g., `tags` vs `tech_stack`, `published_at` vs
  `date`). Re-read Phase 07's final manifest spec before authoring
  `portfolio.toml` — don't trust the example above verbatim.
- **Image URL pointing at the static asset.** `https://modde.rs/logo.svg`
  serves the SVG directly today
  ([website/static/logo.svg](/data/nvme0/can/Projects/rs-modde/website/static/logo.svg)).
  If Phase 07 requires the image URL to be an Immich asset
  (`/api/images/<asset_id>`), upload the logo to Immich first and use
  the resulting URL.
- **Re-publishing without upsert support.** If Phase 07 missed the
  upsert path, re-running the command will hit a unique-constraint
  error. Stop and amend Phase 07 — don't paper over it here.
- **Date format.** plinth's `PortfolioItem.date` is `DateTime<Utc>`
  ([crates/shared/src/portfolio_item.rs:50](/data/nvme0/can/Projects/solo/plinth/crates/shared/src/portfolio_item.rs#L50)).
  Use RFC3339 / ISO-8601 (`2026-05-28T00:00:00Z`) if the TOML parser
  chokes on a date-only value.

## Reference

- Research dossier: [../website-wiring-research.md](../website-wiring-research.md).
- Phase 03: [03-dns-and-first-deploy.md](./03-dns-and-first-deploy.md).
- Phase 07: [07-plinth-portfolio-publish.md](./07-plinth-portfolio-publish.md).
- Schema authority: [plinth/crates/shared/src/portfolio_item.rs](/data/nvme0/can/Projects/solo/plinth/crates/shared/src/portfolio_item.rs).
