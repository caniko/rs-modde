# Phase 02 — Validate and publish the fixed simit

> **Recommended Codex model: GPT 5.5 medium**
>
> This is moderate release-prep work in one repository: validate the completed generator, commit it coherently, and make a fixed simit revision available for rs-modde. GPT 5.5 medium is enough because Phase 01 owns the hard design choices; this phase mainly enforces release hygiene and avoids publishing incomplete generator state.

## Working tree

`/data/nvme0/can/Projects/simit`. Requires Phase 01 acceptance. The tree may still contain unrelated uncommitted changes; separate the release generator changes from unrelated work.

## Goal

The fixed simit generator is committed and available at a stable Git revision that rs-modde can pin.

## Why this matters now

rs-modde should not depend on a dirty local simit checkout. The public release workflow is generated from simit, so rs-modde needs a reproducible simit input revision before its own `flake.lock` and workflow are regenerated.

## Out of scope

- Do not modify rs-modde.
- Do not tag a simit release unless the maintainer explicitly chooses that release workflow.
- Do not include unrelated simit work in the generator commit.

## Plan

1. Inspect simit status and split the diff:
   `git status --short`,
   `git diff --stat`,
   `git diff -- src/commands/init_release.rs src/render/release_workflow.rs src/user_config.rs tests/init_release.rs tests/user_config.rs README.md`.
2. Run the focused checks from Phase 01 again:
   `cargo test init_release user_config`.
3. Run broader checks if available and reasonable:
   `cargo test`.
4. Confirm the fixed generator renders expected rs-modde output without writing:
   `cargo run --manifest-path /data/nvme0/can/Projects/simit/Cargo.toml -- init release --print`
   from `/data/nvme0/can/Projects/rs-modde`, then inspect for `atlas-nix-trusted`, dispatch `version`, and no `install-nix-action`.
5. Commit only the generator-related simit changes. Use a message like:
   `release: generate trusted Forgejo Nix release workflows`.
6. Push the simit branch or trunk revision according to the repository's normal workflow.
7. Record the resulting commit SHA for Phase 03.

## Acceptance criteria

- [ ] Focused simit tests pass: `cargo test init_release user_config`.
- [ ] Broad simit tests pass or any blocker is recorded with exact failing test and log excerpt.
- [ ] A clean simit commit exists with only generator/release-runner related changes.
- [ ] The simit commit SHA is available for rs-modde to pin.
- [ ] No unrelated dirty simit files were reverted or accidentally included.

## Files likely touched

- `/data/nvme0/can/Projects/simit` git history.
- No rs-modde files.

## Pitfalls

- **Symptom:** `cargo test` fails outside release tests. **Cause:** unrelated dirty work in simit. **Recovery:** report exact failing target; do not hide it by weakening release tests.
- **Symptom:** rs-modde cannot pin the commit. **Cause:** fixed simit commit was not pushed or is only local. **Recovery:** push the commit or use the maintainer-approved remote branch, then record the remote revision.
- **Symptom:** commit includes unrelated README badge or upgrade changes. **Cause:** existing dirty work was staged wholesale. **Recovery:** use path-limited staging and inspect `git diff --cached --stat`.

## Reference

- Phase 01 acceptance evidence.
- `/data/nvme0/can/Projects/simit/README.md` distribution-channel documentation.
