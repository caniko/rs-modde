# Plan 025 — simit release automation

> **Recommended Codex model: GPT 5.5 medium**
>
> This is a moderate orchestration plan across two local repositories with clear rollback boundaries: first complete simit's generator, then integrate rs-modde, then supervise public release state. GPT 5.5 medium is enough because the plan is well-scoped and source evidence is local; a smaller model is more likely to miss the dirty-worktree coordination and generated-file constraints.

## Scope and current state

rs-modde's Codeberg page has no downloadable binaries because Codeberg has zero release objects for `caniko/rs-modde`, even though remote tags `0.2.0` and `0.2.1` exist. The `0.2.1` `release.yml` runs failed before build/upload, and the installed simit `0.16.1` still generates release workflows that use `cachix/install-nix-action@v27`, which failed on the Forgejo runner. The local simit checkout already contains uncommitted work in this area: trusted runner resolution, `workflow_dispatch.version`, tag worktree validation, and release workflow tests. Treat those changes as existing work to audit and complete, not disposable scratch.

## Phase table

| Phase | File | Depends on | Touches | Status |
|---|---|---|---|---|
| 01 | [Complete simit release workflow generation](./01-complete-simit-release-generator.md) | none | `/data/nvme0/can/Projects/simit` | blocking |
| 02 | [Validate and publish the fixed simit](./02-validate-and-publish-simit.md) | 01 | `/data/nvme0/can/Projects/simit` | blocked |
| 03 | [Adopt fixed simit in rs-modde](./03-adopt-fixed-simit-in-rs-modde.md) | 02 | `/data/nvme0/can/Projects/rs-modde` | blocked |
| 04 | [Backfill and verify Codeberg release assets](./04-backfill-and-verify-release-assets.md) | 03 | Codeberg release state, `/data/nvme0/can/Projects/rs-modde` | blocked |

## Parallelism layer

Wave 0: Phase 01 only. It owns the generator behavior and tests. Do not start rs-modde integration from the dirty local simit tree until Phase 01 acceptance proves the generator is coherent.

Wave 1: Phase 02 only. It validates, commits, and publishes the fixed simit so rs-modde can pin a stable commit rather than a worktree path.

Wave 2: Phase 03 only. It updates rs-modde's simit input and regenerated workflow. This must serialize after Phase 02 to avoid pinning unpublished local state.

Wave 3: Phase 04 only. It is a release operation against public Codeberg state and must wait for rs-modde's generated workflow to be clean.

## Whole-set acceptance criteria

- [ ] `simit init release --check --diff` is clean in rs-modde after adopting the fixed simit.
- [ ] rs-modde's generated `.forgejo/workflows/release.yml` runs on `atlas-nix-trusted`, has a dispatch `version` input, and does not use `cachix/install-nix-action`.
- [ ] A signed semver tag release path creates or updates a Codeberg release with nonzero uploaded assets.
- [ ] Optional `0.2.1` backfill either succeeds with uploaded assets or records the exact current blocker with the run URL and failing step.

## Global constraints

- Do not hand-edit generated rs-modde release workflow logic except as output from `simit init release`.
- Do not revert unrelated dirty changes in either checkout.
- Do not fabricate release evidence. If a token, runner credential, signed tag, or workflow run is missing, report it with the command or UI workflow that regenerates it.
- Do not move or recreate signed tags without maintainer approval.

## Reference

- Originating diagnosis: Codeberg release API returned `[]`; `berg release list` returned `[]`; `release.yml` runs `138`, `146`, `152`, `165`, and `171` for `0.2.1` failed.
- Current rs-modde release config: `/data/nvme0/can/Projects/rs-modde/flake.nix` `outputs.simitConfig`.
- Current simit generator: `/data/nvme0/can/Projects/simit/src/commands/init_release.rs` and `/data/nvme0/can/Projects/simit/src/render/release_workflow.rs`.
