# Phase 06 — Migrate rs-modde onto canix-toolbelt site helpers

> **Recommended Codex model: GPT 5.5 medium**
>
> Moderate complexity, sub-agent role. Mostly mechanical, but exercises
> three of the four helpers (`mkZolaSite` with theme + dataFiles,
> `mkMdBookDocs`-style Zola docs path, `mkCombinedSite` with `domains`)
> plus the `mkDeployPagesApp` swap. The non-trivial reasoning is
> threading rs-modde's Zola-docs-via-AdiDoks path (not mdBook) through
> the helper API and choosing whether the `pages.yml` workflow consumes
> the flake-parts module or just the lib function.

## Working tree

`/data/nvme0/can/Projects/rs-modde` on `trunk`.

## Goal

rs-modde's flake exposes `website`, `docs`, and `site` packages built
through `canix-toolbelt.lib.*`, with
[scripts/deploy-pages.sh](/data/nvme0/can/Projects/rs-modde/scripts/deploy-pages.sh)
deleted and [.forgejo/workflows/pages.yml](/data/nvme0/can/Projects/rs-modde/.forgejo/workflows/pages.yml)
calling the same `apps.deploy-pages` it does today (the app's
implementation comes from canix-toolbelt now). `nix flake check` is
green. The deployed site is unchanged byte-for-byte from before.

## Why this matters now

rs-modde is the second and final consumer of the Phase 04 helpers.
After this lands, the duplication is gone and the helper API is locked
in. Doing this *after* Phase 05 means the lock bump is single-step
(canix-toolbelt directly). Doing it after only Phase 04 forces a
two-step lock dance to keep rs-harbor consistent.

rs-modde's docs are Zola (via AdiDoks theme injection) — not mdBook.
That means rs-modde uses `mkZolaSite` for *both* outputs (website +
docs), feeding the `theme` parameter for the AdiDoks-themed docs build.

## Out of scope

- Changing website content or templates.
- Changing the deploy workflow's trigger or runner.
- Modifying canix-toolbelt's helper API. If the API doesn't fit, *stop*
  and amend Phase 04 — don't bolt a wrapper into rs-modde.
- Changing the `.domains` file's contents (already
  `["modde.rs" "www.modde.rs"]`).

## Plan

1. Add a `canix-toolbelt` flake input to
   [rs-modde/flake.nix](/data/nvme0/can/Projects/rs-modde/flake.nix):
   ```nix
   canix-toolbelt = {
     url = "git+ssh://git@codeberg.org/caniko/canix-toolbelt.git";
     inputs.nixpkgs.follows = "nixpkgs";
   };
   ```
2. `nix flake update canix-toolbelt` to pin the lock to (or past) the
   commit that ships rs-harbor's adoption.
3. Replace
   [flake.nix:128-182](/data/nvme0/can/Projects/rs-modde/flake.nix#L128-L182)
   with calls into `canix-toolbelt.lib`:
   ```nix
   docs = canix-toolbelt.lib.mkZolaSite {
     inherit pkgs;
     src = ./docs/site;
     theme = { name = themeName; src = adidoks; };
   };
   website = canix-toolbelt.lib.mkZolaSite {
     inherit pkgs;
     src = ./website;
     dataFiles."capability-matrix.toml" = ./docs/capability-matrix.toml;
   };
   site = canix-toolbelt.lib.mkCombinedSite {
     inherit website docs;
     domains = [ "modde.rs" "www.modde.rs" ];
   };
   ```
   Confirm `themeName` is still derived from the adidoks input.
4. Replace the `apps.deploy-pages` block at
   [flake.nix:1292-1301](/data/nvme0/can/Projects/rs-modde/flake.nix#L1292-L1301)
   with `canix-toolbelt.lib.mkDeployPagesApp { inherit pkgs; sitePackage = site; }`
   (or wire via `canix-toolbelt.flakeModules.pages-deploy` if rs-modde
   imports flake-parts — it currently does not, so the lib path is
   simpler).
5. Delete [scripts/deploy-pages.sh](/data/nvme0/can/Projects/rs-modde/scripts/deploy-pages.sh).
6. Confirm
   [.forgejo/workflows/pages.yml](/data/nvme0/can/Projects/rs-modde/.forgejo/workflows/pages.yml)
   still uses `nix run .#deploy-pages` and `DEPLOY_REMOTE=pages-origin` —
   no workflow edits expected, but verify the new app honors the env
   var.
7. Run `nix flake check` locally. Run `nix build .#site --no-link
   --print-out-paths` and diff against the pre-migration `site` output:
   `diff -r <pre> <post>` should report zero meaningful differences
   (timestamps in derivation hashes are fine; HTML content must match
   byte-for-byte).
8. Run `nix run .#deploy-pages` with `DEPLOY_REMOTE` pointed at a scratch
   git remote (or `--dry-run` if the helper supports it) to confirm the
   app at least gets to the push step.
9. Commit on `trunk` — message: `Adopt canix-toolbelt site helpers;
   delete scripts/deploy-pages.sh`.

## Acceptance criteria

- [ ] `scripts/deploy-pages.sh` no longer exists.
- [ ] `nix flake check` is green.
- [ ] `nix build .#site --no-link` succeeds; the resulting tree
      contains `.domains` with `modde.rs` on the first line.
- [ ] `nix build .#website` and `nix build .#docs` succeed.
- [ ] `diff -r` between the pre-migration `site` output and the new
      `site` output shows no HTML content differences.
- [ ] `.forgejo/workflows/pages.yml` is unchanged (or only adjusted
      cosmetically).
- [ ] Single commit on `trunk`.

## Files likely touched

- [flake.nix](/data/nvme0/can/Projects/rs-modde/flake.nix) (delete the
  in-tree derivations, register `canix-toolbelt` input, wire the lib
  calls, swap `apps.deploy-pages`).
- [flake.lock](/data/nvme0/can/Projects/rs-modde/flake.lock).
- [scripts/deploy-pages.sh](/data/nvme0/can/Projects/rs-modde/scripts/deploy-pages.sh) (deleted).

## Pitfalls

- **Losing the AdiDoks theme injection.** The current build copies
  `${adidoks}/* themes/<name>` before `zola build`
  ([flake.nix:140-143](/data/nvme0/can/Projects/rs-modde/flake.nix#L140-L143)).
  The `mkZolaSite` `theme` parameter must replicate this; verify by
  building `docs` and checking `public/index.html` is the AdiDoks
  layout, not a "no theme found" Zola error.
- **Capability-matrix path mismatch.** The site derivation today copies
  `docs/capability-matrix.toml` into `site/data/capability-matrix.toml`
  before `zola build`
  ([flake.nix:163-167](/data/nvme0/can/Projects/rs-modde/flake.nix#L163-L167)).
  The helper's `dataFiles` parameter must place the file at the same
  relative path, or the `load_data` call in
  [website/templates/shortcodes/games.html](/data/nvme0/can/Projects/rs-modde/website/templates/shortcodes/games.html#L1)
  silently produces an empty table.
- **deploy-app env var contract.** The workflow at
  [pages.yml:34](/data/nvme0/can/Projects/rs-modde/.forgejo/workflows/pages.yml#L34)
  exports `DEPLOY_REMOTE=pages-origin`. The new app must honor that
  exact env var name; if Phase 04 used a different name, either swap
  here or amend Phase 04.
- **`themeName` source.** The current code reads it from
  `${adidoks}/theme.toml`. Don't hardcode `"adidoks"` — keep the
  derivation pure by passing the parsed name through.
- **Output structure regressions.** The `site` output today writes
  `.domains` at the top level
  ([flake.nix:181](/data/nvme0/can/Projects/rs-modde/flake.nix#L181)).
  If `mkCombinedSite` writes it elsewhere (e.g., under `.well-known/`),
  Codeberg-Pages won't pick it up. Verify with `cat $(nix build .#site
  --no-link --print-out-paths)/.domains`.

## Reference

- Research dossier: [../website-wiring-research.md](../website-wiring-research.md) —
  "rs-harbor duplication" risk note (mentions the ordering toolbelt →
  rs-harbor → rs-modde).
- Phase 04: [04-toolbelt-site-helpers.md](./04-toolbelt-site-helpers.md).
- Phase 05: [05-rs-harbor-migration.md](./05-rs-harbor-migration.md).
- Current in-tree implementation: [flake.nix:128-182](/data/nvme0/can/Projects/rs-modde/flake.nix#L128-L182), [scripts/deploy-pages.sh](/data/nvme0/can/Projects/rs-modde/scripts/deploy-pages.sh).
