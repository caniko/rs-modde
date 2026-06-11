# Phase 04 — Backfill and verify Codeberg release assets

> **Recommended Codex model: GPT 5.5 high**
>
> This phase is high-stakes operational verification against public release state. GPT 5.5 high is appropriate because it must distinguish missing secrets, failed runner setup, partial release assets, and real success without fabricating evidence; a smaller model is more likely to stop at a green-looking tag or branch workflow.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, plus Codeberg Actions and release state for `caniko/rs-modde`. Requires Phase 03 acceptance.

## Goal

Codeberg shows downloadable rs-modde release assets, either for a backfilled `0.2.1` release or for the next signed release tag, with concrete API evidence that assets exist.

## Why this matters now

Users cannot see downloadable binaries because the Codeberg release list is empty. Source tags alone only provide source archives; they do not provide the generated Linux, macOS, Windows, AppImage, `.deb`, checksum, SBOM, and signature assets that rs-modde documents.

## Out of scope

- Do not move or recreate signed tags without explicit maintainer approval.
- Do not bypass smoke checks unless the maintainer explicitly chooses `force_publish`.
- Do not claim downstream channels are published unless their exact evidence is checked.
- Do not upload hand-built artifacts outside the generated workflow unless the workflow path is blocked and the maintainer approves a manual recovery.

## Plan

1. Confirm repository target:
   `berg repo info`,
   `git remote -v`,
   `git ls-remote --tags origin | sort -V`.
2. Confirm current release state:
   `berg release list`,
   `curl -fsSL https://codeberg.org/api/v1/repos/caniko/rs-modde/releases | jq`.
3. Verify required secrets and runner credentials through Codeberg/runner access:
   `codeberg_token`,
   `MINISIGN_SECRET_KEY`,
   `MINISIGN_PASSWORD`,
   `ATTIC_TOKENS_DIR/rs-modde` if Attic push remains enabled,
   downstream secrets only for channels expected to publish.
4. Backfill path:
   dispatch `.forgejo/workflows/release.yml` with `version=0.2.1` and `force_publish=false`.
   If the UI/CLI cannot dispatch with inputs, record the missing dispatch mechanism and use the next-release path instead.
5. Monitor the specific `release.yml` run. Record run index, URL, status, failing step if any, and whether `Publish Codeberg release` executed.
6. Validate assets:
   `berg release list`,
   `curl -fsSL https://codeberg.org/api/v1/repos/caniko/rs-modde/releases | jq -r '.[] | [.tag_name, (.assets|length), (.assets|map(.name)|join(","))] | @tsv'`.
7. If backfill fails for a foundational input, stop and report:
   missing artifact/secret/runner, why required, upstream producer, exact regeneration workflow, and validation command.
8. If backfill succeeds, update release-state notes or docs only enough to remove stale "no assets" planning state.

## Acceptance criteria

- [ ] `berg release list` shows the target release tag.
- [ ] Codeberg release API shows nonzero assets for the target tag.
- [ ] The release run URL and final status are recorded.
- [ ] If failed, the blocker report names the exact missing input or failing step and the command/workflow that proves it fixed.
- [ ] No release evidence is fabricated from local files alone.

## Files likely touched

- Codeberg release state for `caniko/rs-modde`.
- `/data/nvme0/can/Projects/rs-modde/docs/src/planning/025-simit-release-automation/README.md` only if recording final evidence in this plan set.
- Stable docs only if public installation guidance needs correction after verified assets exist.

## Pitfalls

- **Symptom:** `0.2.1` dispatch starts but fails before build. **Cause:** workflow is still from the old tag's checked-in generated file, or dispatch checks out the old workflow source. **Recovery:** verify Forgejo uses workflow definition from the selected branch for manual dispatch; if it cannot backfill old tags with the new workflow, use the next signed release path.
- **Symptom:** release object exists with zero assets. **Cause:** release creation succeeded but upload loop failed or had no `release/` files. **Recovery:** inspect `Publish Codeberg release` logs and release directory creation steps.
- **Symptom:** branch workflows are green but release missing. **Cause:** checking the wrong workflow/event. **Recovery:** filter Actions runs to `workflow_id == "release.yml"` and the target `prettyref`.

## Reference

- `berg-codeberg-ci` workflow for Codeberg run inspection.
- rs-modde release workflow generated in Phase 03.
- Prior missing evidence: Codeberg release API returned `[]` and repository `release_counter` was `0`.
