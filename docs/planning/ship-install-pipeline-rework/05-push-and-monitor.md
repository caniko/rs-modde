# Phase 5 — Push to `origin/trunk` and monitor Woodpecker

> **Recommended Codex model: GPT 5.5 medium**
>
> Most of the work is mechanical (one `git push`, one CI-status
> check), but the failure-mode branches need judgement: if CI goes
> red on a step that Phase 4 just validated locally, the agent has
> to reason about the environment delta (runner network, container
> image, sibling-repo absence, secret env vars) rather than the
> code. A `low` model tends to repeat the push without diagnosis;
> `medium` has the headroom to read the failing CI log and decide
> whether to fix-forward or revert.
>
> This phase performs a **shared-state action** (`git push` to a
> public-ish origin). The user's CLAUDE.md explicitly calls out
> push as something to confirm before. **Agent should ask the user
> for go-ahead before step 2.** That confirmation step is the
> reason this isn't routed to `low`.
>
> Routed as **leaf × moderate** in the `gpt-plan-routing` matrix.

## Working tree

`/data/nvme0/can/Projects/rs-modde`

**Depends on Phase 4** — local CI parity green is a precondition.
Do not push without it.

## Goal

Two commits are visible on `origin/trunk` and the Woodpecker
pipeline reports all four steps green. After this phase, the
install pipeline rework is in canonical history; future work
proceeds from a clean baseline.

## Why this matters now

This is the actual upstream landing. Everything before this phase
is preparatory. After this phase, the rework is rollback-able only
by `git revert`, not by amending. The phase also catches
local-vs-CI environment drifts that Phase 4 couldn't (different
runner image, missing secrets, network egress rules).

## Out of scope

- Force-pushing. Trunk is the user's main branch. Force-push is
  explicitly forbidden by CLAUDE.md.
- Tagging or cutting a release. Releases happen separately.
- Notifying any downstream consumers (e.g., a `canix`-side update).
  If a downstream depends on `rs-modde`'s `memory-admission` git
  pin, that's a follow-up task, not part of this phase.
- Opening a PR — the user pushes directly to `trunk`; there's no
  PR workflow for this branch.
- Backfilling release notes. Defer to a separate task if wanted.

## Plan

1. **Pre-push sanity:**
   ```
   git fetch origin
   git log --oneline origin/trunk..HEAD
   git diff origin/trunk..HEAD --stat | tail -1
   ```
   Expect: two commits (Phase 3's output) and the ~92-file delta.
   If `origin/trunk` has advanced (someone else pushed), stop here.
   Pull, rebase, re-run Phase 4, then return to this step.

2. **Confirm with the user.** Push is a shared-state action. Show
   the user:
   - `git log -2 --oneline`
   - `git diff origin/trunk..HEAD --stat | tail -1`

   Ask for explicit go-ahead before step 3. Do not push speculatively.

3. **Push:**
   ```
   git push origin trunk
   ```
   Output should be a fast-forward.

4. **Locate the pipeline.** The runner is at atlas
   (host of `https://attic.candee.baby/canix` per the canix-cli
   skill); the Forgejo / Woodpecker UI is the canonical viewer.
   Open the project's pipeline list — `gh`-style API access may
   not be available for Woodpecker, but the web UI is.

5. **Monitor.** Watch for each of the four steps in order:
   - `nix develop --command cargo fmt --all -- --check` (fast,
     seconds to minutes).
   - `nix develop --command cargo clippy --workspace -- -D warnings`
     (minutes; full re-clippy in a fresh container).
   - `nix develop --command just coverage-ci` (slowest — llvm-cov
     instrumented build; ~10–30 minutes depending on runner CPU).
   - `nix develop --command cargo build --workspace --release`
     (also slow — separate release compile, ~10–20 minutes).

6. **If a step fails:**
   - Read the step's full log.
   - Compare to Phase 4's local run. Likely deltas:
     - **Network egress.** Did the runner fail to fetch a crate
       (memory-admission included)? If so, Phase 1's git-pin URL
       may be SSH not HTTPS, or the runner image lacks `git`.
       Recovery: fix the URL or runner image; land a follow-up.
     - **Missing env var or secret.** Unlikely for this CI config
       but possible if any of the new modules read `MODDE_*` env
       vars at compile time (none should).
     - **Runner image mismatch.** Local runs in `nixos/nix:latest`,
       check.yml says the same. Should match.
     - **Coverage threshold.** If Phase 2 raised `FAIL_UNDER`
       optimistically, the runner's coverage % may differ from the
       local one. Recovery: lower `FAIL_UNDER` in a follow-up.
   - **Never** `git push --force` to repair. The user's CLAUDE.md
     bans this. **Never** `git revert` without telling the user;
     show the failing log first and decide together.

7. **Once green:**
   ```
   git rev-parse origin/trunk     # must match git rev-parse HEAD
   ```
   Report success to the user with the two commit subjects and the
   pipeline URL.

## Acceptance criteria

- [ ] `git push origin trunk` succeeded as fast-forward (no
      `--force`).
- [ ] `git rev-parse origin/trunk` matches `git rev-parse HEAD`.
- [ ] Woodpecker pipeline for the just-pushed commit reports
      success on all four steps.
- [ ] No `--force-with-lease` or `--force` was used.
- [ ] No commit was amended after Phase 3 closed.

## Files likely touched

None on the local machine. Possible follow-up commits if CI fails,
authored as small targeted fixups (not amendments).

## Pitfalls

- **`origin/trunk` advanced.** Rare on this user's setup (sole
  pusher), but the canix automation or another agent could push.
  Symptom: `git push` rejected with non-fast-forward. Recovery:
  `git fetch && git log origin/trunk..HEAD`; if the divergence is
  trivial, `git pull --rebase` and re-validate (back to Phase 4).
  If the divergence is non-trivial (other agent's work to
  reconcile), pause and surface to the user.
- **CI step times out.** Woodpecker step timeout defaults vary;
  `coverage-ci` is the most likely victim. Symptom: step kills at
  e.g. 30 minutes. Recovery: split the coverage step into two CI
  steps (instrumented compile + run) in a follow-up, or raise the
  step timeout. Don't disable coverage.
- **Pipeline shows green but a downstream consumer breaks.** If
  any user of `rs-modde` (the canix flake input is the most likely
  surface) reads the source by SHA, that downstream needs a bump.
  Recovery: surface to the user as a separate follow-up — this
  phase doesn't fix it.
- **Forgetting that `trunk` is the user's main branch.** Some
  conventions name the main branch `main`. Here the project
  uses `trunk` per `git status`. Don't push to `main` (doesn't
  exist).

## Reference

- Origin: `git@codeberg.org:caniko/rs-modde.git` (verify with
  `git remote -v` before push).
- CI viewer: Woodpecker UI on the atlas-hosted Forgejo / canix.
- Companion phases: [04](./04-ci-parity-validation.md) (must be
  green locally before this phase runs).
- CLAUDE.md push policy: confirm before push; never `--no-verify`;
  never `--force` to main/trunk.
