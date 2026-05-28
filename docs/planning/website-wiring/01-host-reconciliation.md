# Phase 01 — Reconcile rs-modde to a single canonical host

> **Recommended Codex model: GPT 5.5 medium**
>
> The work is moderate complexity at a leaf-ish role: a deterministic
> sweep of host strings across config files, planning docs, and a Nix
> flake. The non-trivial part is judging which historical/audit
> references should be left as "this was the host before the switch"
> versus rewritten in place; a smaller model is likely to either
> blanket-rewrite stale narrative or miss a Cargo metadata field.

## Working tree

`/data/nvme0/can/Projects/rs-modde` on `trunk`.

## Goal

Every in-tree reference to a public modde host points at `modde.rs`
(with `www.modde.rs` as an alias) — except deliberate references inside
deploy-script echoes that name the canonical Codeberg-Pages URL for
debug clarity. The Homebrew formula, simit homebrew block, APT
bootstrap plan, release-integration audit, install docs, and SECURITY.md
all agree.

## Why this matters now

The website config in
[website/config.toml:5](/data/nvme0/can/Projects/rs-modde/website/config.toml#L5)
sets `base_url = "https://modde.rs"`, and the combined `site` derivation
in [flake.nix:181](/data/nvme0/can/Projects/rs-modde/flake.nix#L181)
writes a `.domains` file listing `modde.rs www.modde.rs`. But the
Homebrew formula at
[flake.nix:480](/data/nvme0/can/Projects/rs-modde/flake.nix#L480) and
the simit homebrew block at
[flake.nix:1360](/data/nvme0/can/Projects/rs-modde/flake.nix#L1360)
publish `homepage = "https://modde.tartanoglu.com"`. The APT plan at
[docs/planning/apt-channel-bootstrap/01-codeberg-pages-bootstrap.md:115-118](/data/nvme0/can/Projects/rs-modde/docs/planning/apt-channel-bootstrap/01-codeberg-pages-bootstrap.md#L115-L118)
has both checkboxes unchecked. The release-integration audit at
[docs/planning/release-integration-audit/audit-report.md:139](/data/nvme0/can/Projects/rs-modde/docs/planning/release-integration-audit/audit-report.md#L139)
says "Pages serves at https://caniko.codeberg.page/rs-modde-apt/". Shipping the
website without first collapsing this split-brain wastes the first
deploy and bakes the wrong URL into Homebrew/crates.io metadata.

## Out of scope

- Configuring DNS, Codeberg-Pages verification, or pushing the secret.
  Those land in Phase 03.
- Extracting any Nix code to canix-toolbelt (Phase 04).
- Touching plinth.
- Editing rs-harbor.

## Plan

1. From the rs-modde root, list every non-historical occurrence:
   `rg -n "modde\\.tartanoglu\\.com" -- . | grep -v 'planning/.*audit-report' | grep -v 'planning/.*apt-channel-bootstrap'`.
   Read each hit and decide rewrite vs leave-as-historical-note.
2. Rewrite live config sites:
   - [flake.nix](/data/nvme0/can/Projects/rs-modde/flake.nix#L480) —
     Homebrew formula `homepage`.
   - [flake.nix](/data/nvme0/can/Projects/rs-modde/flake.nix#L1360) —
     `simitConfig.homebrew.homepage`.
   - Any other live config file flagged by step 1 (SECURITY.md,
     install docs).
3. In the APT bootstrap plan
   [docs/planning/apt-channel-bootstrap/01-codeberg-pages-bootstrap.md](/data/nvme0/can/Projects/rs-modde/docs/planning/apt-channel-bootstrap/01-codeberg-pages-bootstrap.md):
   check the URL choice. Pick choice B (`modde.rs`/`modde.tartanoglu.com`
   path) consistent with the host decision; record the chosen apex as
   `modde.rs` and update the templated URL examples accordingly.
4. In
   [docs/planning/release-integration-audit/](/data/nvme0/can/Projects/rs-modde/docs/planning/release-integration-audit/):
   change live URL examples to `modde.rs`. Leave purely historical
   narrative ("we considered tartanoglu.com because…") alone — those are
   audit prose, not config.
5. Add a one-line note at the top of
   [docs/planning/release-integration-audit/audit-report.md](/data/nvme0/can/Projects/rs-modde/docs/planning/release-integration-audit/audit-report.md)
   recording the host decision: `Canonical host as of <date>: modde.rs.
   Earlier references to modde.tartanoglu.com are pre-decision.`
6. Confirm the website + docs configs still agree:
   - [website/config.toml:5](/data/nvme0/can/Projects/rs-modde/website/config.toml#L5)
     `base_url = "https://modde.rs"`.
   - [docs/site/config.toml:5](/data/nvme0/can/Projects/rs-modde/docs/site/config.toml#L5)
     `base_url = "https://modde.rs/docs"`.
7. Run `nix flake check` to confirm Homebrew formula re-derives cleanly
   after the metadata change.
8. Commit with `simit commit` (or its equivalent) — message:
   `Canonicalize modde host to modde.rs`.

## Acceptance criteria

- [ ] `rg -n "modde\\.tartanoglu\\.com" rs-modde/` returns hits **only**
      in narrative paragraphs of `docs/planning/` files, never in any
      `Cargo.toml`, `flake.nix`, `*.toml` config, `*.yaml`, or
      `*.md` install/security instructions.
- [ ] `nix eval .#homebrew-formula --raw | grep -i homepage` reports
      `https://modde.rs`.
- [ ] `nix flake check` passes locally.
- [ ] [docs/planning/apt-channel-bootstrap/01-codeberg-pages-bootstrap.md](/data/nvme0/can/Projects/rs-modde/docs/planning/apt-channel-bootstrap/01-codeberg-pages-bootstrap.md)
      has exactly one of the two URL-choice checkboxes ticked and the
      `URL chosen:` line filled in.
- [ ] A single commit on `trunk` titled along the lines of `Canonicalize
      modde host to modde.rs`.

## Files likely touched

- [flake.nix](/data/nvme0/can/Projects/rs-modde/flake.nix) (Homebrew + simit blocks).
- [SECURITY.md](/data/nvme0/can/Projects/rs-modde/SECURITY.md).
- [docs/planning/apt-channel-bootstrap/](/data/nvme0/can/Projects/rs-modde/docs/planning/apt-channel-bootstrap/) (multiple files).
- [docs/planning/release-integration-audit/audit-report.md](/data/nvme0/can/Projects/rs-modde/docs/planning/release-integration-audit/audit-report.md).
- [docs/planning/release-integration-audit/07-crates-io-publish.md](/data/nvme0/can/Projects/rs-modde/docs/planning/release-integration-audit/07-crates-io-publish.md).
- Any install documentation under `docs/site/content/docs/getting-started/`.

## Pitfalls

- **Rewriting historical narrative.** If a planning paragraph says
  "originally we considered modde.tartanoglu.com", *do not* rewrite it
  to `modde.rs` — that destroys decision provenance. Only rewrite live
  config and forward-looking install commands.
- **Crates.io `homepage` mismatch.** If the homebrew block changes but
  individual `crates/*/Cargo.toml` files still publish
  `modde.tartanoglu.com`, the next `simit release` will publish
  inconsistent metadata. Run
  `rg -n "homepage|documentation|repository" crates/*/Cargo.toml` and
  reconcile.
- **APT plan decision drift.** The APT bootstrap plan considers the URL
  choice deferrable; locking it in this phase means downstream phases of
  that plan no longer have a choice to make. Leave a note in this
  phase's commit message so it's discoverable.

## Reference

- Research dossier: [../website-wiring-research.md](../website-wiring-research.md) —
  see "Domain decision is unresolved" and "Existing Plan Status".
- [website/config.toml](/data/nvme0/can/Projects/rs-modde/website/config.toml).
- [flake.nix:128-182, :471-506, :1346-1364](/data/nvme0/can/Projects/rs-modde/flake.nix).
