# Phase 02 — Local-dev polish for the website

> **Recommended Codex model: GPT 5.5 low**
>
> Two mechanical edits: add a `justfile` recipe that injects
> `capability-matrix.toml` into the Zola tree before `zola serve`, and
> create the empty `website/static/screenshots/` directory with a
> README. No design content, no log reading, leaf-node role.

## Working tree

`/data/nvme0/can/Projects/rs-modde` on `trunk`.

## Goal

A contributor can run a single `just` recipe to preview the website
locally (`zola serve`), with the supported-games table rendering
correctly. The screenshots directory exists so future PNG/GIF additions
are obvious.

## Why this matters now

The website's index template includes
[website/templates/shortcodes/games.html:1](/data/nvme0/can/Projects/rs-modde/website/templates/shortcodes/games.html#L1)
which calls `load_data(path="data/capability-matrix.toml", format="toml")`.
The file is only injected at Nix-build time per
[flake.nix:163-167](/data/nvme0/can/Projects/rs-modde/flake.nix#L163-L167);
`cd website && zola serve` fails because `data/capability-matrix.toml`
does not exist relative to the Zola root. New contributors hit this
immediately. Same story for `website/static/screenshots/` — placeholder
slots reserved in
[website/content/_index.md](/data/nvme0/can/Projects/rs-modde/website/content/_index.md#L74-L94)
have no enclosing directory yet.

## Out of scope

- Adding real screenshot images.
- Changing the Nix build path (the derivation continues to inject the
  data file).
- Migrating to a different shortcode design.

## Plan

1. Open [justfile](/data/nvme0/can/Projects/rs-modde/justfile) and add:
   ```
   website-serve:
       mkdir -p website/data
       cp docs/capability-matrix.toml website/data/capability-matrix.toml
       cd website && zola serve
   ```
   If a `just` recipe of similar shape already exists, append rather
   than duplicate.
2. Add `website/data/` to `.gitignore` (it's a build-time staging dir).
3. `mkdir -p website/static/screenshots` and add a `.gitkeep` plus a
   `README.md` inside it that names the three slot filenames already
   referenced from [content/_index.md](/data/nvme0/can/Projects/rs-modde/website/content/_index.md):
   `mod-list.png`, `download-queue.gif`, `fomod-wizard.png`.
4. Sanity-run: `just website-serve` from the repo root, hit
   `http://127.0.0.1:1111/`, scroll to "Supported Games", confirm the
   table renders. Stop the server.
5. Commit with `simit commit`.

## Acceptance criteria

- [ ] `just website-serve` from a clean checkout produces a running Zola
      server on the default port and renders the supported-games table
      without errors.
- [ ] `website/data/` is gitignored.
- [ ] [website/static/screenshots/README.md](/data/nvme0/can/Projects/rs-modde/website/static/screenshots/README.md)
      exists and names the three placeholder filenames.
- [ ] Single commit on `trunk`.

## Files likely touched

- [justfile](/data/nvme0/can/Projects/rs-modde/justfile).
- [.gitignore](/data/nvme0/can/Projects/rs-modde/.gitignore).
- `website/static/screenshots/README.md` (new).
- `website/static/screenshots/.gitkeep` (new).

## Pitfalls

- **Copying into a tracked directory.** `website/data/` must be
  gitignored; otherwise the recipe pollutes `git status` on every run.
- **Shell vs nu in justfile.** The maintainer's shell is `nu`; `just`
  recipes default to `sh` which is correct here, but if the existing
  `justfile` uses a non-sh shebang, match it.

## Reference

- Research dossier: [../website-wiring-research.md](../website-wiring-research.md) —
  "Build path uses an in-tree copy of `capability-matrix.toml`".
- [website/templates/shortcodes/games.html](/data/nvme0/can/Projects/rs-modde/website/templates/shortcodes/games.html).
