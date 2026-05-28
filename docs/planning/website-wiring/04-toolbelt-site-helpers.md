# Phase 04 — Extract Zola/mdBook/site helpers into canix-toolbelt

> **Recommended Codex model: GPT 5.5 high**
>
> Complex sub-agent/orchestrator-flavored work: designing a small lib
> API that survives at least two consumers (rs-harbor and rs-modde),
> picking the parameter set that doesn't force every consumer back to
> bespoke wrappers, and broadening canix-toolbelt's stated scope. The
> non-trivial path choice (per-helper signature, where the
> `.domains`/deploy-script live, whether to ship a flake-parts module
> for the pages workflow) needs real reasoning, not raw output volume.
> A smaller model is likely to ship a helper that rs-modde immediately
> has to re-wrap.

## Working tree

`/data/nvme0/can/Projects/canix-toolbelt` on its default branch
(`trunk` per convention; confirm with `git branch --show-current`).

## Goal

`canix-toolbelt` exposes a stable, host-agnostic API for building Zola
sites, mdBook docs, combined website+docs trees, and a Codeberg-Pages
deploy app. The API is parameterized enough that rs-harbor (mdBook-only
docs) and rs-modde (Zola docs via AdiDoks + data-file injection +
`.domains`) both consume it without resurrecting their own copies.

## Why this matters now

[rs-harbor/nix/site.nix](/data/nvme0/can/Projects/rs-harbor/nix/site.nix)
already duplicates the same Zola+mdBook+combined shape that
[rs-modde/flake.nix:128-182](/data/nvme0/can/Projects/rs-modde/flake.nix#L128-L182)
hand-rolls. Each new tool the maintainer publishes pays the same tax.
[canix-toolbelt/lib/default.nix](/data/nvme0/can/Projects/canix-toolbelt/lib/default.nix)
has no site/zola/pages helpers today, but is the maintainer's stated
home for "reusable, host-agnostic Nix building blocks" per
[canix-toolbelt/README.md:1-15](/data/nvme0/can/Projects/canix-toolbelt/README.md#L1-L15).

## Out of scope

- Migrating any consumer (rs-harbor or rs-modde). Those are Phases 05
  and 06.
- Splitting canix-toolbelt into a sibling `canix-sites` flake. The
  maintainer's instruction is "move the generics to canix toolbelt".
- Adding non-Codeberg deploy targets (GitHub Pages, GitLab Pages,
  Cloudflare Pages). Keep the helper Codeberg-Pages-flavored; document
  it as such.
- Designing a generic "publish to any pages provider" abstraction.

## Plan

1. Read both source copies cold:
   - [rs-modde/flake.nix:128-182](/data/nvme0/can/Projects/rs-modde/flake.nix#L128-L182)
     for the Zola+AdiDoks docs derivation, data-file injection,
     combined site, and `.domains` file generation.
   - [rs-modde/scripts/deploy-pages.sh](/data/nvme0/can/Projects/rs-modde/scripts/deploy-pages.sh)
     and [rs-modde/.forgejo/workflows/pages.yml](/data/nvme0/can/Projects/rs-modde/.forgejo/workflows/pages.yml)
     for the CI deploy shape.
   - [rs-harbor/nix/site.nix](/data/nvme0/can/Projects/rs-harbor/nix/site.nix)
     for the mdBook-only docs and combined site without `.domains`.
2. Design the lib API. Aim for four functions, none of which import the
   others by default:
   - `lib.mkZolaSite { pkgs, src, dataFiles ? {}, theme ? null, ... }`
     where `dataFiles` is `{ <relpath> = derivationOrPath; ... }` and
     `theme` accepts `{ name, src }` for AdiDoks-style theme injection.
   - `lib.mkMdBookDocs { pkgs, src, ... }`.
   - `lib.mkCombinedSite { website, docs, domains ? null }` where
     `domains` is `null` or `[ "modde.rs" "www.modde.rs" ]`; nullable
     so rs-harbor's apex-free case stays clean.
   - `lib.mkDeployPagesApp { pkgs, sitePackage, remoteEnvVar ? "DEPLOY_REMOTE", branch ? "pages", commitMessageTemplate ? null }`
     returning a `writeShellApplication` deploy script. Token handling
     stays in CI, never in the helper.
3. Implement those functions under
   [canix-toolbelt/lib/](/data/nvme0/can/Projects/canix-toolbelt/lib/)
   as `site.nix`, exported from `default.nix`. Keep each helper as a
   plain Nix function — no `inputs` capture, no `flake-utils`.
4. Add a `flakeModules.pages-deploy` flake-parts module under
   [canix-toolbelt/flake-modules/](/data/nvme0/can/Projects/canix-toolbelt/flake-modules/)
   that, given `canix-toolbelt.pages-deploy = { sitePackage, branch ?
   "pages", ... }`, registers `apps.deploy-pages` per system. This is
   purely a convenience over step 2's `mkDeployPagesApp` for consumers
   that prefer flake-parts.
5. Add a small NixOS-test or evaluation check under
   [canix-toolbelt/nixos-tests/](/data/nvme0/can/Projects/canix-toolbelt/nixos-tests/)
   or as a `pkgs.runCommand` derivation in
   `perSystem.checks.site-helpers-eval`. The check:
   - Builds a tiny fixture Zola site (single `_index.md`, single
     template) via `lib.mkZolaSite` and confirms the output contains an
     `index.html`.
   - Builds an mdBook fixture via `lib.mkMdBookDocs`.
   - Composes both via `lib.mkCombinedSite` with `domains = [ "test.example" ]`
     and asserts `.domains` exists and starts with `test.example`.
6. Update [canix-toolbelt/README.md](/data/nvme0/can/Projects/canix-toolbelt/README.md)
   with a "Site helpers" section. Be explicit that:
   - The helpers target Codeberg-Pages but the build outputs are
     plain static dirs and reusable elsewhere.
   - The deploy helper takes the remote URL via env var (`DEPLOY_REMOTE`)
     so secrets stay in CI.
   - The scope expansion was deliberate (mention the maintainer's
     directive, paraphrased: "tools' website plumbing").
7. Run `nix flake check` on canix-toolbelt — must be green.
8. Commit on default branch (no PR per global constraints). Use a
   descriptive message: `Add lib.{mkZolaSite,mkMdBookDocs,mkCombinedSite,mkDeployPagesApp} + flakeModules.pages-deploy`.

## Acceptance criteria

- [ ] `nix eval .#lib.mkZolaSite` from canix-toolbelt's root returns a
      function (not `null`, not an error).
- [ ] Same for `mkMdBookDocs`, `mkCombinedSite`, `mkDeployPagesApp`.
- [ ] `nix eval .#flakeModules.pages-deploy` returns a flake-parts
      module path.
- [ ] `nix flake check` is green on canix-toolbelt (includes the
      site-helpers-eval check from step 5).
- [ ] README has a "Site helpers" section that documents the four lib
      functions and the flake-parts module with one usage snippet each.
- [ ] `mkCombinedSite` works with `domains = null` (no `.domains` file
      emitted) and with a non-empty list.
- [ ] `mkDeployPagesApp` never references a hardcoded token or remote
      URL — both come from env vars at run time.

## Files likely touched

- [canix-toolbelt/lib/site.nix](/data/nvme0/can/Projects/canix-toolbelt/lib/site.nix) (new).
- [canix-toolbelt/lib/default.nix](/data/nvme0/can/Projects/canix-toolbelt/lib/default.nix) (add `site = import ./site.nix { inherit lib; };` or merge into the existing returned set).
- [canix-toolbelt/flake-modules/pages-deploy.nix](/data/nvme0/can/Projects/canix-toolbelt/flake-modules/pages-deploy.nix) (new).
- [canix-toolbelt/flake.nix](/data/nvme0/can/Projects/canix-toolbelt/flake.nix) (register new flake-module).
- [canix-toolbelt/README.md](/data/nvme0/can/Projects/canix-toolbelt/README.md) (Site helpers section).
- A fixture under `canix-toolbelt/nixos-tests/` or `canix-toolbelt/lib/site-fixtures/`.

## Pitfalls

- **Over-parameterizing.** Resist the temptation to expose every Zola
  config knob. The two real consumers only need: `src`, optional
  `theme`, optional `dataFiles`. Anything else is YAGNI and forces
  consumers to thread defaults.
- **Coupling deploy helper to a specific remote.** `mkDeployPagesApp`
  must default to reading the remote name from `DEPLOY_REMOTE` env var
  (matching the existing
  [deploy-pages.sh:12](/data/nvme0/can/Projects/rs-modde/scripts/deploy-pages.sh#L12)
  convention) so CI can swap the remote without rebuilding the app.
- **mdBook theme assumption.** rs-harbor and rs-modde have different
  mdBook configs; do not bake a theme into `mkMdBookDocs`. Take the
  full source tree as `src` and let consumers configure mdBook
  themselves via `book.toml`.
- **`.domains` semantics.** Codeberg interprets the first line of
  `.domains` as the canonical apex. Document this in the helper's
  doc-comment. Consumers passing a list should know order matters.
- **Scope creep into pages-deploy module.** The flake-parts module
  should be a thin wrapper around `mkDeployPagesApp` — not another
  abstraction layer with `options.canix-toolbelt.pages-deploy.token`
  or similar. Keep tokens out of the option tree.

## Reference

- Research dossier: [../website-wiring-research.md](../website-wiring-research.md) —
  "canix-toolbelt has no site/zola/pages helpers today",
  "rs-harbor already has a duplicate of the helper".
- [canix-toolbelt/README.md](/data/nvme0/can/Projects/canix-toolbelt/README.md).
- [rs-harbor/nix/site.nix](/data/nvme0/can/Projects/rs-harbor/nix/site.nix).
- [rs-modde/flake.nix:128-182](/data/nvme0/can/Projects/rs-modde/flake.nix#L128-L182).
- Sister extraction plan: [canix/docs/planning/canix-toolbelt-extraction/](/data/nvme0/can/Projects/canix/docs/planning/canix-toolbelt-extraction/).
