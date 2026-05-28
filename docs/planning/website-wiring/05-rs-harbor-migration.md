# Phase 05 — Migrate rs-harbor onto canix-toolbelt site helpers

> **Recommended Codex model: GPT 5.5 medium**
>
> Moderate complexity, sub-agent role: a single flake.lock bump, a flake
> input addition, a delete of `nix/site.nix`, and re-pointing the
> packages that consumed it. The non-trivial part is keeping rs-harbor's
> `nix flake check` green through the swap.

## Working tree

`/data/nvme0/can/Projects/rs-harbor` on its default branch.

## Goal

rs-harbor builds its website + docs through
`canix-toolbelt.lib.mkZolaSite` and `mkMdBookDocs`, with
[rs-harbor/nix/site.nix](/data/nvme0/can/Projects/rs-harbor/nix/site.nix)
deleted in the same commit. `nix flake check` is green.

## Why this matters now

Phase 04 ships the helper; until at least one consumer adopts it, the
API isn't truly battle-tested and rs-modde's Phase 06 lacks a precedent
to follow. rs-harbor is the safer first consumer because its docs are
plain mdBook (no theme injection, no data-file injection, no
`.domains`) — it exercises the helper's simplest path.

## Out of scope

- Adding `mkDeployPagesApp` consumption to rs-harbor. rs-harbor doesn't
  currently publish to Pages — leave its deploy story untouched.
- Changing rs-harbor's website content or templates.
- Touching rs-modde — that's Phase 06.

## Plan

1. From rs-harbor's root, add a `canix-toolbelt` flake input that
   follows nixpkgs:
   ```nix
   canix-toolbelt = {
     url = "git+ssh://git@codeberg.org/caniko/canix-toolbelt.git";
     inputs.nixpkgs.follows = "nixpkgs";
   };
   ```
2. Update the lock: `nix flake update canix-toolbelt`. Verify the lock
   resolves to the canix-toolbelt commit that landed Phase 04.
3. In rs-harbor's flake outputs (likely
   [rs-harbor/flake.nix](/data/nvme0/can/Projects/rs-harbor/flake.nix)),
   replace `import ./nix/site.nix { inherit pkgs; }` (or whatever the
   current call site is) with:
   ```nix
   site = let
     website = canix-toolbelt.lib.mkZolaSite { inherit pkgs; src = ./website; };
     docs = canix-toolbelt.lib.mkMdBookDocs { inherit pkgs; src = ./docs; };
   in canix-toolbelt.lib.mkCombinedSite { inherit website docs; domains = null; };
   ```
   Also expose `website` and `docs` as separate packages if they were
   exposed before.
4. Delete [rs-harbor/nix/site.nix](/data/nvme0/can/Projects/rs-harbor/nix/site.nix).
5. Run `nix flake check`. Fix any path or attribute drift.
6. Build the artifacts: `nix build .#website .#docs .#site` and
   spot-check the outputs render the same as before.
7. Commit on default branch — message: `Adopt canix-toolbelt site
   helpers; delete nix/site.nix`.

## Acceptance criteria

- [ ] `rs-harbor/nix/site.nix` no longer exists.
- [ ] `nix flake show .#packages` lists `website`, `docs`, and `site`.
- [ ] `nix flake check` is green.
- [ ] `nix build .#site --no-link` succeeds; output structure mirrors
      pre-migration output (top-level `index.html`, `docs/`
      subdirectory).
- [ ] Single commit on default branch.

## Files likely touched

- [rs-harbor/flake.nix](/data/nvme0/can/Projects/rs-harbor/flake.nix).
- [rs-harbor/flake.lock](/data/nvme0/can/Projects/rs-harbor/flake.lock).
- [rs-harbor/nix/site.nix](/data/nvme0/can/Projects/rs-harbor/nix/site.nix) (deleted).

## Pitfalls

- **Attribute renames.** Phase 04's helper names (`mkZolaSite`,
  `mkMdBookDocs`, `mkCombinedSite`) may have been adjusted during
  implementation. Check the actual API surface in canix-toolbelt's
  lib/default.nix before writing the call sites.
- **Implicit `pkgs` plumbing.** rs-harbor's flake may have a top-level
  `pkgs` binding under a different name (e.g., a per-system `pkgsFor`).
  Match the existing convention.
- **Forgetting to expose `website`/`docs` individually.** If rs-harbor
  previously exposed `packages.website` and `packages.docs`
  independently, keep those — downstream consumers may rely on them.
- **canix-toolbelt commit pin.** Until canix-toolbelt cuts a tag,
  rs-harbor's lock pins a specific commit. The Phase 06 lock bump must
  match-or-postdate this one.

## Reference

- Research dossier: [../website-wiring-research.md](../website-wiring-research.md) —
  "rs-harbor already has a duplicate of the helper".
- Phase 04: [04-toolbelt-site-helpers.md](./04-toolbelt-site-helpers.md).
- Current copy: [rs-harbor/nix/site.nix](/data/nvme0/can/Projects/rs-harbor/nix/site.nix).
