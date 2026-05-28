# Phase 05 — End-to-end smoke + parent-plan verify flip

> **Recommended Codex model: GPT 5.5 high**
>
> Orchestrator step across three external systems (Codeberg release CI,
> Codeberg Pages serving, a Debian container's apt stack) plus the
> parent plan's verify-mode re-run. Each surface can fail in subtly
> different ways and the right diagnosis often needs careful log
> reading. High-tier is the right cost; max would be overkill.

## Working tree

`/data/nvme0/can/Projects/rs-modde`. This phase produces no committed code changes in steady state — it exercises the system end-to-end and may add a single line to `docs/planning/release-integration-audit/audit-report.md` (or its Phase 04a state subsection) capturing the smoke-pass evidence and the apt repo URL.

A throwaway Debian 12 (bookworm) container is needed for the user-side smoke. Use `nix shell nixpkgs#podman -c podman run --rm -it debian:bookworm bash` (or `docker run --rm -it debian:bookworm bash`), whichever is available locally.

## Goal

Prove the apt channel works from end to end:

1. A prerelease tag (`X.Y.Z-rc.N`) skips apt publish cleanly (the workflow's prerelease gate).
2. A stable tag (`X.Y.Z`) pushes a signed `dists/stable/Release` + `pool/` tree to `caniko/rs-modde-apt`.
3. The published tree resolves over HTTPS from the URL chosen in Phase 01.
4. A fresh Debian 12 container can `apt update && apt install modde modde-ui` after pinning the apt-key fingerprint.
5. The parent plan's verify run (`/multi-phase-plan-codex verify docs/planning/release-integration-audit`) reports Phase 04a as **passed**.

## Why this matters now

This is the only phase that touches live release CI and a live user-installable apt mirror. Without it, the previous four phases produce a plausible-looking system whose first real failure mode would land in a user's `apt update` log. Catching it here lets the maintainer iterate without breaking the public install path.

## Out of scope

- Backporting hotfixes through the apt channel (a separate process; covered in CONTRIBUTING.md's hotfix release section).
- Cross-arch testing on `arm64`. The plan supports it (the script and reprepro config already cover `arm64`); the smoke runs against `amd64` only.
- Replacing the throwaway tags with real ones. The smoke uses cheap, throwaway version numbers that get yanked at the end.

## Plan

1. **Prerelease tag smoke.** Push a throwaway prerelease tag (e.g. `0.0.0-rc.1`) from a clean rs-modde branch. Watch the release workflow run:
   - Confirm the `Publish APT repository` step logs `Prerelease 0.0.0-rc.1; skipping APT repository publish.` and exits 0.
   - Confirm no commits land on `caniko/rs-modde-apt` (the `pages` branch tip is unchanged from Phase 01).
   - Delete the throwaway tag locally and on the remote afterwards: `git push origin :refs/tags/0.0.0-rc.1`.
2. **Stable tag smoke.** Push a throwaway stable tag (e.g. `0.0.0`) with a matching `## [0.0.0] - YYYY-MM-DD` heading temporarily added to `CHANGELOG.md` (revert the changelog entry after the smoke). Watch the release workflow:
   - Confirm the `Build Debian packages` step produces at least `release/modde_<version>_amd64.deb` and `release/modde-ui_<version>_amd64.deb`.
   - Confirm `Publish APT repository` exits 0 with reprepro output and a `git push --force-with-lease origin HEAD:refs/heads/pages` line in its log.
   - Confirm a new commit on `caniko/rs-modde-apt`'s `pages` branch with the message `apt: publish modde 0.0.0`.
3. **Pages serving smoke.** Resolve the chosen public URL (record from Phase 01) and confirm the tree is served:
   ```sh
   base="$(cat docs/planning/apt-channel-bootstrap/01-codeberg-pages-bootstrap.md | grep -F 'URL chosen:' | awk '{print $NF}')"
   curl -fsSI "${base}/dists/stable/Release" | head -3
   curl -fsSI "${base}/dists/stable/Release.gpg" | head -3
   curl -fsSI "${base}/key.gpg.asc" | head -3
   ```
   All three must return HTTP 200.
4. **User-side install smoke** inside a Debian 12 container:
   ```sh
   container=$(podman run --rm -d debian:bookworm sleep 3600)
   podman exec "$container" bash -lc '
     set -euo pipefail
     apt-get update
     apt-get install -y curl gnupg ca-certificates
     install -d -m 0755 /etc/apt/keyrings
     curl -fsSL "'"${base}"'/key.gpg.asc" | gpg --dearmor > /etc/apt/keyrings/modde.gpg
     fp=$(gpg --show-keys --with-colons /etc/apt/keyrings/modde.gpg | awk -F: "/^fpr:/ {print \$10; exit}")
     test "$fp" = "CCFE4A8461DF8778F5227684B6DB8F177A951E1B"
     echo "deb [signed-by=/etc/apt/keyrings/modde.gpg] '"${base}"' stable main" \
       > /etc/apt/sources.list.d/modde.list
     apt-get update
     apt-get install -y modde modde-ui
     modde --version
     modde-ui --version
   '
   podman stop "$container"
   ```
   All asserts in the script must succeed.
5. **Yank the throwaway release.** From rs-modde:
   ```sh
   # Codeberg release: delete from the web UI.
   git push origin :refs/tags/0.0.0
   git tag -d 0.0.0
   # CHANGELOG.md: revert the temporary 0.0.0 heading.
   git checkout CHANGELOG.md
   ```
   The apt repo's published 0.0.0 packages can be left in place — the next real release rewrites the entire tree.
6. **Parent-plan verify.** Run the parent plan's verify mode:
   ```
   /multi-phase-plan-codex verify docs/planning/release-integration-audit
   ```
   Inspect the per-phase report for Phase 04a. Expected outcome: Phase 04a flips from `partial` to `passed`. If it does not, the report names the specific acceptance criterion that still fails; loop back into the relevant earlier phase of *this* plan to fix it, not the parent plan.
7. **Record evidence.** Append a brief evidence block to `docs/planning/release-integration-audit/audit-report.md`'s "Phase 04a APT Repository State" subsection: smoke date, throwaway tag used, container image hash, and confirmation that the parent verify passed Phase 04a. Keep it to four lines.

## Acceptance criteria

- [ ] The prerelease-tag smoke produced a workflow run with a visible `skipping APT repository publish` log line and no new commit on `caniko/rs-modde-apt`.
- [ ] The stable-tag smoke produced a workflow run with a `git push --force-with-lease origin HEAD:refs/heads/pages` line in the `Publish APT repository` step's log and a new commit on `caniko/rs-modde-apt`'s `pages` branch whose message is `apt: publish modde 0.0.0`.
- [ ] `curl -fsSI <base>/dists/stable/Release` returns HTTP 200 with `<base>` set to the Phase 01 URL choice.
- [ ] The Debian container script in Plan step 4 ran to completion and printed `modde --version` and `modde-ui --version` output.
- [ ] Plan step 4's fingerprint check (`test "$fp" = "CCFE..."`) passed inside the container.
- [ ] The throwaway tag `0.0.0` no longer exists on the remote.
- [ ] `CHANGELOG.md` no longer contains the temporary `## [0.0.0]` heading.
- [ ] Running `/multi-phase-plan-codex verify docs/planning/release-integration-audit` reports Phase 04a as `passed` with no unresolved gaps.
- [ ] `docs/planning/release-integration-audit/audit-report.md` has a four-line evidence note appended to its "Phase 04a APT Repository State" subsection.

## Files likely touched

- `docs/planning/release-integration-audit/audit-report.md` (one four-line evidence note appended).
- No code changes in steady state.

Transient (added then reverted within this phase):

- `CHANGELOG.md` (temporary `## [0.0.0]` heading, reverted at Plan step 5).
- A throwaway git tag `0.0.0` (deleted at Plan step 5).
- One throwaway commit on `caniko/rs-modde-apt`'s `pages` branch (left in place; superseded by the next real release).

## Pitfalls

- **CHANGELOG enforcement bites the smoke**: the release workflow refuses to build without a matching CHANGELOG heading for the tag. Forgetting Plan step 2's temporary heading means the smoke fails at `Validate tag`, not at the apt publish, and the resulting log noise is misleading. Add the heading; revert it after.
- **Tag-already-exists**: if `0.0.0` was used in a previous smoke and not deleted, the workflow refuses to re-tag. Use a fresh suffix (`0.0.1`, `0.0.2`) and update the smoke script accordingly.
- **Codeberg Pages cache**: the served tree may lag a `git push` by 30-90 seconds. Plan step 3's `curl` should retry with a small backoff if the first call returns 404, before declaring a failure.
- **Container apt cache pollution**: re-running Plan step 4 in the same container without `apt-get clean && rm -rf /var/lib/apt/lists/*` can show stale `Release` files. Use `--rm -d` semantics (fresh container each run).
- **Force-with-lease vs deploy-key race**: if a maintainer hand-edited the `pages` branch between Phase 01 and Plan step 2, the runner's push fails with `! [rejected] pages -> pages (stale info)`. Investigate the manual edit — do not blindly rebase or `--force` from CI. The publish script intentionally refuses to clobber unexpected history.
- **Parent-verify still failing**: if the verify in Plan step 6 reports Phase 04a still `partial` after a green smoke, the most likely cause is a doc string in Phase 04 that didn't get updated (e.g. a stale `modde_apt_repo_ssh_key` mention in `audit-report.md`). Loop back to Phase 04, not Phase 03.
- **Throwaway-version yank scope**: do *not* yank the throwaway tag's crates.io publish if any per-crate `publish-crate-*.yaml` workflow ran. Reverting a smoke that accidentally pushed `0.0.0` to crates.io requires `cargo yank` per crate; better to use a clearly throwaway version number that's outside the project's real release line (e.g. `0.0.0` is unambiguous because the project's current version is `0.2.0+`).

## Risk profile

This phase is a candidate for a `5.5 max` route only when the maintainer wants a careful audit of every log line. At `5.5 high` the agent should:

- Refuse to call the phase complete on a partial Pages-cache miss.
- Refuse to call the phase complete if the Debian container smoke shows `apt update` warnings about the `signed-by` field (those warnings indicate a fingerprint mismatch the loop above is meant to catch).
- Refuse to mark the parent plan's Phase 04a as passed without re-running the verify; do not paper over a partial verify with chat-level reasoning.

## Reference

- Phase 01-04 of this plan (predecessors).
- Parent plan's verify mode: `/multi-phase-plan-codex verify docs/planning/release-integration-audit`.
- Codeberg Pages caching behaviour: <https://docs.codeberg.org/codeberg-pages/>.
- Project versioning policy memory: bare semver, major-anytime; throwaway `0.0.0` is unambiguously outside the real release line.
