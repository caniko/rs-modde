# Phase 01 — Complete simit release workflow generation

> **Recommended Codex model: GPT 5.5 high**
>
> This phase is complex leaf implementation in a generator with an existing dirty worktree and public release consequences. GPT 5.5 high is appropriate for preserving current uncommitted work, tightening behavior without regressions, and adding tests that encode the intended runner semantics; a smaller model may flatten the distinction between cargo CI, Nix CI, release jobs, and trusted runner requirements.

## Working tree

`/data/nvme0/can/Projects/simit`. This repository is dirty. Start by reading `git status --short`, then inspect the current diffs in `src/commands/init_release.rs`, `src/render/release_workflow.rs`, `src/user_config.rs`, `tests/init_release.rs`, and `tests/user_config.rs`.

## Goal

`simit init release` generates a Forgejo release workflow that automatically builds and uploads Codeberg release assets on the configured trusted Nix runner, supports manual dispatch by existing tag, and does not use `cachix/install-nix-action` on that trusted runner path.

## Why this matters now

rs-modde's `0.2.1` release uploads never appeared because the tag-triggered release runs failed before artifact build/upload. The latest observed manual retry failed at `https://github.com/cachix/install-nix-action@v27`, leaving `Publish Codeberg release` skipped. Installed simit `0.16.1` still generates that failing step, so the upstream generator must own the fix.

## Out of scope

- Do not edit rs-modde in this phase.
- Do not publish simit or create release tags.
- Do not change downstream package channel behavior except where required by release workflow generation.
- Do not remove existing uncommitted simit work without understanding it.

## Plan

1. Capture baseline:
   `git status --short --branch`,
   `git diff -- src/commands/init_release.rs src/render/release_workflow.rs src/user_config.rs tests/init_release.rs tests/user_config.rs`.
2. Finish runner resolution in `init_release`:
   use explicit `[release.artifacts].runner` when present; otherwise resolve Forgejo Nix release runner through `UserConfig::resolve_ci_runners(Platform::Forgejo, Runtime::Nix, None, None, false)`.
3. Preserve or complete `preinstalled_nix` plumbing so the generated workflow emits job-level `NIX_CONFIG` and `XDG_CACHE_HOME` and skips `install-nix-action` when using the trusted preinstalled Nix runner.
4. Complete `workflow_dispatch.inputs.version` handling:
   dispatch input first, then `GITHUB_REF_NAME` / `FORGE_REF_NAME` / `CODEBERG_REF_NAME`, then raw ref fallback.
5. Complete tag validation from the requested tag:
   fetch the tag, create a detached tag worktree, validate version and changelog inside that worktree, import `keys/maintainers.gpg` from it, run `git verify-tag`, then checkout the validated tag commit before building.
6. Add or finish tests in `tests/init_release.rs` and `tests/user_config.rs`:
   trusted Nix release runner path, no install-nix action on that path, dispatch `version`, tag worktree validation, and explicit-runner compatibility.
7. Run:
   `cargo test init_release user_config`.
8. Run the rendered workflow check against rs-modde without modifying it:
   `cargo run --manifest-path /data/nvme0/can/Projects/simit/Cargo.toml -- init release --check --diff`
   from `/data/nvme0/can/Projects/rs-modde`, and record whether expected drift remains for Phase 03.

## Acceptance criteria

- [ ] `cargo test init_release user_config` passes in `/data/nvme0/can/Projects/simit`.
- [ ] Generated trusted Forgejo Nix release workflows contain `runs-on: atlas-nix-trusted` in the split-runner fixture.
- [ ] Generated trusted Forgejo Nix release workflows contain job-level `NIX_CONFIG` with `experimental-features = nix-command flakes` and `accept-flake-config = true`.
- [ ] Generated trusted Forgejo Nix release workflows do not contain `cachix/install-nix-action`.
- [ ] Generated workflows include `workflow_dispatch.inputs.version` and validate/build from the fetched, verified tag commit.
- [ ] Existing explicit runner behavior is covered by tests and either remains supported or fails with a precise remediation.

## Files likely touched

- `/data/nvme0/can/Projects/simit/src/commands/init_release.rs`
- `/data/nvme0/can/Projects/simit/src/render/release_workflow.rs`
- `/data/nvme0/can/Projects/simit/src/user_config.rs`
- `/data/nvme0/can/Projects/simit/tests/init_release.rs`
- `/data/nvme0/can/Projects/simit/tests/user_config.rs`
- `/data/nvme0/can/Projects/simit/README.md` only if public command semantics change.

## Pitfalls

- **Symptom:** rs-modde still drifts back to `install-nix-action`. **Cause:** the generator only handles omitted runner, while rs-modde still has explicit `release.artifacts.runner`. **Recovery:** either make explicit trusted labels preinstalled-Nix aware or plan Phase 03 to remove the explicit runner.
- **Symptom:** tests pass locally but generated workflow validates the branch tree, not tag tree. **Cause:** version/changelog checks still run before checkout to the tag commit. **Recovery:** keep all release metadata checks inside the tag worktree before `git checkout --detach "$validated_sha"`.
- **Symptom:** user config tests reject cargo workflows. **Cause:** trusted-runner requirement applied too broadly. **Recovery:** require trust for Nix CI/release runner paths only.

## Reference

- rs-modde failing release evidence: Codeberg runs `138` and `171` for `0.2.1`.
- simit generator files named above.
- rs-modde generated release workflow expected behavior from `.forgejo/workflows/release.yml`.
