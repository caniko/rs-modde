# Phase 07 — Real release: re-cut the tag, publish the full fan-out

> **Recommended Codex model: GPT 5.5 max**
>
> Terminal, frontier-risk phase: a live multi-job Codeberg release that fans out
> to ~10 external publish targets, several **irreversible** (Chocolatey/AUR/COPR/
> Homebrew/Scoop pushes; Flathub/winget PRs). Debugging a red run means reading
> logs across many steps and distinguishing a clean soft-skip from a real failure
> mid-fan-out, then deciding fix-forward vs abort with a half-published public
> state on the line. Frontier complexity × top-level/orchestrator role, with a
> real-version-burn cost on a wrong call. This is exactly the case the routing
> matrix reserves the top tier for — `max`, with the full pre-mortem below.

## Working tree

`/data/nvme0/can/Projects/rs-modde`. Depends on a **green Phase 06** prerelease
run and a confirmed Phase 04 secret mapping. Tag signing is **human-in-the-loop**
(smartcard PIN) — agents prepare exact commands and stop; the maintainer signs
and pushes. Serializes strictly after Phase 06 (same repo tags, same
`capacity = 1` atlas runner). On success, the plan is exhausted.

## Goal

A real release tag — **re-cut onto a commit containing the Phase 01 windows fix
and all trunk fixes** — drives a **green** Codeberg release run in which every
configured channel publishes correctly or soft-skips cleanly (missing optional
secret), and the published artifacts are verified per channel.

## Why this matters now

The current tag `0.2.1` = `1e4a555` predates the windows fix and the 6 trunk
commits; a tag-push of it would check out `1e4a555` and **fail at the windows
build** (it lacks the Phase 01 fix). It must be re-cut. This is the phase where
the downstream channels — gated off during the prerelease — actually publish to
public registries for the first time, so it carries all the irreversibility the
prerelease deliberately avoided.

Decide the version first: re-cut **`0.2.1`** (delete the stale tag and re-tag the
fixed commit — acceptable since `0.2.1` never produced a published release) or
cut the **next version** if a clean re-tag of `0.2.1` is undesirable. Record the
decision and keep `CHANGELOG.md`/`.#modde.version` consistent with it.

## Out of scope

- Do **not** push the real tag before Phase 06 is green.
- Do **not** hand-edit `release.yml` to patch a CI bug — fix the generator
  (Phase 05) and regenerate, then re-tag.
- Do **not** publish a channel whose secret is absent by fabricating one — let
  it soft-skip and document it.
- Do **not** overwrite a mistaken crates.io publish — that's the separate
  `publish-crate-*` workflow; `cargo yank` if needed, never overwrite.

## Plan

1. **Re-cut decision + version.** Choose `0.2.1` re-cut vs next version (see
   Goal). Confirm `CHANGELOG.md` has the dated section for the chosen version,
   `nix eval .#modde.version` equals it, and the target commit contains the
   Phase 01 windows fix (`git show <commit>:flake.nix | grep -c <fix-marker>`).
2. **Prepare the signed-tag commands for the maintainer** (PIN). For a `0.2.1`
   re-cut:
   ```
   git tag -d 0.2.1 && git push origin :refs/tags/0.2.1     # remove stale tag (local+remote)
   git tag -s 0.2.1 -m "rs-modde 0.2.1" <commit-with-fixes>
   git push --follow-tags origin <branch>                  # fires release.yml on the [0-9]* tag
   ```
   Hand off; the maintainer runs them. **Announce an atlas maintenance freeze**
   for the run's duration (Phase 03 rule: no `canix deploy switch atlas`, atlas
   reboot, or `forgejo-runner@*.service` restart while the release builds).
3. **Watch the full run** (`berg` / basic-auth logs). Confirm in order: build
   (all 8 targets) → `SHA256SUMS` + minisign + cosign → **Codeberg release
   create + upload** → Attic push → then each downstream channel:
   apt (reprepro → Codeberg Pages), aur (3 flavors, SSH push + `.SRCINFO`), copr
   (`copr-cli` build submit), homebrew, scoop, chocolatey (if runner key
   present), flathub/winget PRs (if tokens present), announce (if tokens
   present). For each: **published** or **soft-skipped (missing secret)** —
   never silent failure.
4. **Triage mid-fan-out failures with care** (see Failure modes). If the
   Codeberg release/upload step partially failed, downstream channels that
   download from it 404 — fix the upload first. Prefer fix-forward (re-run the
   failed step / re-tag after a generator fix) over leaving a half-published
   state; abort only per the rollback drill.
5. **Post-release verification per channel:**
   - Codeberg release has all platform/deb/srpm/AppImage assets +
     `SHA256SUMS.txt(.minisig)` + cosign bundles.
   - APT repo branch updated (`dists/.../Packages`); AUR packages updated
     (`.SRCINFO` present, 3 flavors); COPR build submitted/succeeded; Homebrew/
     Scoop buckets bumped; Chocolatey package pushed (if key present);
     Flathub/winget PRs opened (if tokens present).
   - Record which channels published vs soft-skipped.

## Acceptance criteria

- [ ] The real tag points at a commit containing the Phase 01 windows fix; the
      stale `1e4a555` tag is not what shipped.
- [ ] The release run is **green**; the Codeberg release exists with all expected
      assets + signed `SHA256SUMS.txt(.minisig)` + cosign attestations.
- [ ] Each downstream channel is confirmed **published** or documented as
      **intentionally soft-skipped (missing secret)** — no channel in a silent-
      failure state.
- [ ] Post-release per-channel verification is recorded (assets present, repos/
      buckets bumped, PRs opened).
- [ ] No hand-edited `release.yml`; no fabricated secrets; any crates.io mishap
      handled by `cargo yank`, not overwrite.

## Files likely touched

- `/data/nvme0/can/Projects/rs-modde` — tags only (and, if a generator bug
  forces it, a Phase 05 bounce + regenerated `release.yml` committed, then
  re-tag).

## Risk profile

- **Irreversible publishes:** Chocolatey/AUR/COPR/Homebrew/Scoop pushes and
  Flathub/winget PRs are hard or impossible to fully retract.
- **Version burn:** a real `X.Y.Z` consumed by a failed run can't be reused;
  Phase 06 prerelease-first mitigates this, but a re-cut `0.2.1` that fails still
  costs the number.
- **Partial publish:** some channels succeed and some fail mid-run, leaving an
  inconsistent public state (Codeberg release up, AUR not).
- **Cross-channel ordering:** downstream channels download from the Codeberg
  release; if upload partially failed, downloads 404.
- **Secret exposure:** a misconfigured step could echo a token to logs.
- **Runner teardown:** atlas maintenance during the ~1 h build kills the run
  (Phase 03) — costly here because it may interrupt mid-fan-out.

## Strategy

Escalation ladder, each rung independently validated with low revert cost:

1. **Re-cut tag → spine** (build + sign + Codeberg upload). Already proven green
   on the rc in Phase 06; on the real tag it additionally flips
   `IS_PRERELEASE=false`. Revert: delete the tag + its Codeberg release (cheap).
2. **Downstream fan-out.** First real exercise of the pushes. Watch each; a
   single channel failing should not be "fixed" by muting — fix-forward or defer
   that channel explicitly. Revert: see Rollback drill (mostly forward-fix; undo
   a stray push in that channel's repo if it landed).

Never skip rung 1's green confirmation before letting rung 2 proceed. Proceed
only after Phase 06 is green and the secret mapping (Phase 04) is confirmed.

## Rollback drill

Rehearse on the Phase 06 rc artifacts first so these are muscle memory:

- Delete a bad tag (local + remote):
  `git tag -d <tag> && git push origin :refs/tags/<tag>`.
- Delete a Codeberg release a run created: `berg` release delete, or API
  `DELETE /repos/caniko/rs-modde/releases/{id}` (token), or the UI.
- Cancel an in-flight run before it reaches downstream-publish steps: Codeberg
  Actions UI / `berg` (stop the run). SLA: be able to do this in < 5 min.
- crates.io: N/A here (separate `publish-crate-*` workflow); if a wrong crate
  version ships, `cargo yank`, do not overwrite.

## Failure modes and recoveries

- **F1 — validate-tag fails** (CHANGELOG/sig/version). Symptom: run dies at
  "Validate tag". Cause: missing `## [<tag>]` section, unsigned tag, or
  `nix eval .#modde.version` ≠ tag. Recovery: fix the offending item; re-cut a
  fresh tag (you cannot move a signed tag without re-signing — PIN).
- **F2 — windows build fails again.** Symptom: `modde-windows-deps` exit 101 /
  `PowrProf.h`. Cause: the tag points at a commit *without* the Phase 01 fix.
  Recovery: re-cut the tag onto the fixed commit (this is the whole reason the
  tag is re-cut — verify the marker in step 1).
- **F3 — Codeberg upload partially fails, downstreams 404.** Symptom: apt/aur/
  copr can't download an asset. Cause: upload step errored after creating the
  release. Recovery: re-run the upload (or re-run the job); confirm all assets
  present before letting downstream channels proceed.
- **F4 — a downstream push fails mid-fan-out** (e.g. AUR SSH rejected, COPR
  submit errors). Symptom: one channel red, others green. Cause: secret/permission
  or channel-side state. Recovery: fix the secret/permission (Phase 04), re-run
  that step; if it can't be fixed promptly, explicitly defer that channel
  (document it) rather than muting silently — do not abort channels that already
  published.
- **F5 — runner torn down mid-run** (exit 143). Symptom: SIGTERM/"context
  canceled". Cause: atlas reboot/switch or runner restart during the job (Phase
  03). Recovery: ensure the atlas maintenance freeze held; re-run the release
  run (tag already exists — use
  `workflow_dispatch` with the version input, or re-trigger).
- **F6 — secret echoed to logs.** Symptom: a token visible in the run log.
  Cause: a step without masking. Recovery: rotate the exposed secret immediately
  (canix/Codeberg), fix the generator step (Phase 05), regenerate, re-tag.

## Reference

- Prerequisite: a green [06-prerelease-validation.md](./06-prerelease-validation.md)
  run + confirmed [04-audit-release-secrets.md](./04-audit-release-secrets.md)
  mapping.
- Tag hygiene rationale: plan README "Tag hygiene" (tag `0.2.1`=`1e4a555` lacks
  the windows fix; validate-tag validates the tag worktree while build uses the
  checkout).
- Runner-freeze rule: [03-diagnose-runner-teardown.md](./03-diagnose-runner-teardown.md).
- Skills: `berg-codeberg-ci`, `atlas-runner`, `canix-cli`.
- On success the plan is exhausted; prompt `verify` to audit all phases'
  acceptance criteria against the repo/CI state.
