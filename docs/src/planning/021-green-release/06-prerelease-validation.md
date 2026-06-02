# Phase 06 — Prerelease (`-rc.N`) validation run

> **Recommended Codex model: GPT 5.5 high**
>
> Complex live-CI orchestration: push a signed prerelease tag, watch a long
> multi-step run on the atlas runner, and distinguish a clean
> gate-driven soft-skip from a real failure across ~10 steps — then decide
> fix-forward (bounce to the simit generator) vs re-tag. Orchestrator role at
> complex complexity. Not `max`: a prerelease tag/release is cheap to delete, so
> the irreversibility that earns `max` lives in Phase 07. `high` for the
> multi-step log interpretation and the fix-forward judgement.

## Working tree

`/data/nvme0/can/Projects/rs-modde`. Depends on **Phases 01, 02, 03, 04**
(build green incl. windows; all targets verified; runner stable; secrets
mapped). If **Phase 05** landed, the regenerated `release.yml` is in use;
otherwise validate via real `-rc.N` **tag pushes** (not `workflow_dispatch` on
trunk) so the checkout equals the validated tag. Tag signing is
**human-in-the-loop** (smartcard PIN) — prepare commands, the maintainer runs
them. Serializes with Phase 07 (same repo tags, same `capacity = 1` runner).

## Goal

A signed `-rc.N` prerelease tag drives a **green** Codeberg release run in which
the build + SBOM + `SHA256SUMS` + minisign + cosign + **Codeberg release create
+ asset upload** steps all execute, and every downstream channel step
**skips cleanly by its prerelease gate** with a logged reason — zero hard
failures. This proves the build/sign/upload spine and the gate logic on a real
run without spending the real version.

## Why this matters now

Everything to date is locally verified or proven only in fragments on CI. The
generated `release.yml` re-implements a previously hand-rolled pipeline; only a
live run proves the bash, the secret wiring (Phase 04), the runner stability
(Phase 03), and the now-fixed windows build (Phase 01) hold together end to end.
Prereleases exist precisely so this proof doesn't burn the real 0.2.1. The
prerelease will **not** exercise the actual downstream pushes (they gate on
`IS_PRERELEASE=false`) — that's expected, and Phase 07 is where they first fire.

## Out of scope

- Do **not** push a real `X.Y.Z` tag here — `-rc.N` only.
- Do **not** "fix" a red step by hand-editing `release.yml` — fix the simit
  generator (Phase 05 scope) and regenerate.
- Do **not** silently disable a channel; a must-publish channel that errors is a
  bug to fix, not to mute.
- Do **not** proceed to Phase 07 until this run is green and the secret mapping
  (Phase 04) is confirmed.

## Plan

1. **Pre-flight.** Confirm Phase 01/02 (windows + all targets build), Phase 03
   (runner online, mitigation in place / rule documented), Phase 04 (secret
   mapping recorded). Ensure `CHANGELOG.md` has a `## [<rc-version>] - <date>`
   section (validate-tag requires it) and the tag will be signed. Pick the rc
   version: prefer `0.2.1-rc.1` if the caret-vs-prerelease resolution allows, or
   the next version's `-rc.1` — note that semver caret deps can refuse a
   prerelease (`modde-core = "^0.2.0"` won't match `0.2.1-rc.1`); if `cargo
   update`/build chokes on the prerelease, fall back to validating via a
   throwaway **stable-looking** tag on a scratch branch, or accept that the rc
   exercises the spine only. Record the choice.
2. **Cut + push the signed prerelease tag** (maintainer, PIN). Prepare the exact
   commands and hand off, e.g.:
   ```
   git tag -s 0.2.1-rc.1 -m "rs-modde 0.2.1-rc.1" <commit-with-windows-fix>
   git push --follow-tags origin <branch>   # tag-push fires release.yml [0-9]* trigger
   ```
   The tag must point at a commit containing the Phase 01 windows fix.
3. **Watch the run.** List/inspect with the `berg` CLI / `berg-codeberg-ci`
   skill, or the API/basic-auth log commands from the plan README. Walk every
   step:
   - **Must run (not prerelease-gated):** checkout, validate-tag (worktree
     assertion + `git verify-tag`), supply-chain (`cargo deny`), SRPM, SBOM,
     **Build release artifacts** (all 8 targets), `SHA256SUMS` + minisign +
     cosign, **Codeberg release create + upload**, Attic push. Confirm these are
     green.
   - **Should skip cleanly (prerelease-gated):** apt, aur, copr, homebrew,
     scoop, chocolatey, flathub, winget, announce — each must log
     "Prerelease …; skipping …", not error.
4. **Triage red steps.** For each failure decide: real bug → fix the **simit
   generator** + regenerate (Phase 05 mechanics) → re-tag a fresh `-rc.(N+1)`;
   or environmental (runner/secret) → fix in canix (Phase 03/04) and re-run.
   Iterate to green.
5. **(Optional) exercise one downstream push** without a real release: on a
   throwaway branch+tag, temporarily flip that one channel's prerelease gate via
   config so its push mechanics run once; revert after. Only do this if you want
   pre-Phase-07 confidence in a specific channel; otherwise accept that
   downstream pushes first run in Phase 07 under close watch.
6. **Clean up** the prerelease: after green, delete the rc tag + its Codeberg
   release (rehearses the Phase 07 rollback drill):
   ```
   git tag -d 0.2.1-rc.1 && git push origin :refs/tags/0.2.1-rc.1
   # delete the Codeberg release via berg / API DELETE /repos/caniko/rs-modde/releases/{id}
   ```

## Acceptance criteria

- [ ] A `-rc.N` run on Codeberg is **green**: build (all 8 targets) + SBOM +
      `SHA256SUMS` + minisign + cosign + Codeberg-release-upload + Attic push all
      executed successfully.
- [ ] Every downstream channel step is **skipped-by-gate with a logged reason**
      (apt/aur/copr/homebrew/scoop/chocolatey/flathub/winget/announce) — zero
      hard failures.
- [ ] Any CI failure encountered was fixed in the **simit generator** (or
      canix), regenerated, and re-validated on a fresh `-rc.(N+1)` — no
      hand-edited `release.yml`.
- [ ] The rollback drill was rehearsed: the rc tag and its Codeberg release were
      deleted cleanly.
- [ ] The exact rc-version choice and the caret-vs-prerelease decision are
      recorded for Phase 07.

## Files likely touched

- `/data/nvme0/can/Projects/rs-modde` — tags only (and, if a generator bug is
  found, a Phase 05 bounce + regenerated `release.yml` committed).
- `CHANGELOG.md` — an rc section if the validate-tag check requires one for the
  chosen rc version.

## Pitfalls

- **F1 — caret excludes the prerelease.** `cargo update`/build fails to resolve
  `modde-core = "^0.2.x"` against `…-rc.1`. Symptom: build dies resolving deps.
  Recovery: don't pursue the prerelease via workspace dep bump; validate the
  spine with the tag as-is, or use a scratch stable-looking tag — see step 1.
- **F2 — validate-tag fails.** Missing `## [<rc>]` changelog section, unsigned
  tag, or `nix eval .#modde.version` ≠ tag. Recovery: add the section / sign /
  align version; re-tag a fresh rc.
- **F3 — mistaking a clean soft-skip for a failure.** A gated channel logs
  "skipping" and the step is green; don't fix-forward a non-problem. Read the
  step's conclusion, not just the presence of "skip".
- **F4 — deploying atlas mid-run.** Per Phase 03, a `canix deploy switch atlas`
  during the build SIGTERMs the job (exit 143). Don't deploy while the run is
  building.
- **F5 — cold build ~1 h.** Don't assume a hang; the first rc run cold-builds
  all 8 targets. Pre-warming (Phase 02 step 5) shortens it. It's well under the
  3 h act default.

## Reference

- Run-199/198 diagnosis (why the spine is trustworthy but windows needed
  fixing); plan README "Global constraints" for CI-inspection commands.
- Prerequisites: Phases [01](./01-fix-windows-cross-build.md),
  [02](./02-verify-all-cross-targets.md), [03](./03-diagnose-runner-teardown.md),
  [04](./04-audit-release-secrets.md).
- Generator-fix mechanics: [05-harden-simit-generator.md](./05-harden-simit-generator.md).
- Skills: `berg-codeberg-ci`, `atlas-runner`.
- Consumer: [07-real-release.md](./07-real-release.md).
