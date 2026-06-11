# Phase 03 — Adopt fixed simit in rs-modde

> **Recommended Codex model: GPT 5.5 high**
>
> This phase is complex integration work because rs-modde has a dirty tree, generated workflow drift, a flake input pin, and public release docs. GPT 5.5 high is appropriate to avoid trampling unrelated edits while making the generated release workflow and simit pin line up exactly.

## Working tree

`/data/nvme0/can/Projects/rs-modde`. Requires Phase 02's fixed simit commit SHA. The tree is dirty; do not revert unrelated changes. Pay special attention to existing modifications in `flake.nix`, `flake.lock`, `.forgejo/workflows/release.yml`, `docs/src/SUMMARY.md`, and release docs.

## Goal

rs-modde pins the fixed simit revision and its generated release workflow is clean, trusted-runner based, dispatch-backfillable, and ready to create Codeberg release assets automatically.

## Why this matters now

The repo currently has no downloadable Codeberg release assets. rs-modde's release docs and install instructions point users to the Codeberg releases page, so the generated workflow must reliably create those assets before another public release claim is made.

## Out of scope

- Do not run the public release workflow.
- Do not create, move, or delete tags.
- Do not hand-edit generated workflow semantics.
- Do not resolve unrelated rs-modde dirty changes unless they block simit adoption.

## Plan

1. Capture baseline:
   `git status --short --branch`,
   `git diff -- .forgejo/workflows/release.yml flake.nix flake.lock docs/src/reference/architecture.md docs/src/getting-started/installation.md`.
2. Update `flake.nix` simit input to the Phase 02 fixed commit or tag.
3. Run `nix flake lock --update-input simit` or the equivalent targeted lock update required by the flake input format.
4. Decide runner config based on Phase 01 behavior:
   if simit now resolves the trusted Nix runner from user config, remove `release.artifacts.runner = "atlas-nix-trusted"` from rs-modde; if explicit trusted runner remains the supported path, keep it.
5. Regenerate:
   `nix develop -c simit init release`
   or, if the dev shell is not available yet, use the fixed simit binary directly.
6. Validate:
   `simit init release --check --diff`,
   `nix eval --raw .#modde.version`.
7. Inspect `.forgejo/workflows/release.yml` and confirm:
   `runs-on: atlas-nix-trusted`,
   `workflow_dispatch` has `version`,
   no `cachix/install-nix-action`,
   `Publish Codeberg release` remains present,
   `CODEBERG_TOKEN: ${{ secrets.codeberg_token }}` remains present.
8. Update stable docs only if necessary to describe the regenerated release contract. Do not add a false claim that assets exist until Phase 04 proves it.

## Acceptance criteria

- [ ] rs-modde pins the fixed simit revision in `flake.nix` and `flake.lock`.
- [ ] `simit init release --check --diff` passes in rs-modde.
- [ ] Generated `.forgejo/workflows/release.yml` uses `atlas-nix-trusted` and omits `cachix/install-nix-action`.
- [ ] Generated workflow supports dispatch `version` for existing tag backfill.
- [ ] Generated workflow still uploads all files under `release/` to the configured Codeberg release.
- [ ] The rs-modde diff does not include unrelated reversions.

## Files likely touched

- `/data/nvme0/can/Projects/rs-modde/flake.nix`
- `/data/nvme0/can/Projects/rs-modde/flake.lock`
- `/data/nvme0/can/Projects/rs-modde/.forgejo/workflows/release.yml`
- `/data/nvme0/can/Projects/rs-modde/docs/src/reference/architecture.md` only if documentation needs correction.

## Pitfalls

- **Symptom:** `simit init release --check --diff` still wants `install-nix-action`. **Cause:** rs-modde is using the old simit binary from PATH or dev shell. **Recovery:** confirm `simit --version` and `nix flake metadata --json . | jq '.locks.nodes.simit.locked'`.
- **Symptom:** workflow runner flips away from `atlas-nix-trusted`. **Cause:** user config was not loaded or explicit runner was removed before generator supports inference. **Recovery:** restore the explicit runner or fix simit runner inference before continuing.
- **Symptom:** docs claim binaries exist while Codeberg still has no release object. **Cause:** integration phase overreached into public release claims. **Recovery:** move those claims to Phase 04 after evidence.

## Reference

- Phase 02 fixed simit commit SHA.
- rs-modde `outputs.simitConfig` in `flake.nix`.
- Current release page evidence: `berg release list` returned `[]`.
