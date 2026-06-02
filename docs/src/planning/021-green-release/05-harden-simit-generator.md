# Phase 05 — Harden the simit release generator (validate/build parity + flake-config) — OPTIONAL

> **Recommended Codex model: GPT 5.5 high**
>
> A generator change in simit that affects every downstream user, touching the
> release-workflow render path with release semantics on the line (validate the
> tag but build HEAD is a latent correctness trap). Complex work in a
> sub-agent/orchestrator role: it needs the render-module design held against
> simit's config→resolve→render architecture, idempotent `--check`
> round-tripping, and tests — and a wrong change silently ships a broken
> `release.yml` to other projects. `high` effort for the design + regression
> surface. **This phase is OPTIONAL and off the critical path** (see "Why").

## Working tree

`/data/nvme0/can/Projects/simit` for the generator change, then
`/data/nvme0/can/Projects/rs-modde` to regenerate `release.yml`. Independent of
Phases 01/03/04; do **not** run concurrently with Phase 02 if you regenerate
`release.yml` while a build is reading the tree (02 is build-only and won't
touch `release.yml`, so in practice they're fine). simit's main tree carries the
maintainer's unrelated `init_flake`/`user_config` WIP — **do not touch it**.

## Goal

The simit-generated `release.yml` is hardened so that (1) when triggered by
`workflow_dispatch` on a branch it builds the **validated tag worktree** rather
than the branch HEAD — eliminating the validate-one-commit/build-another
divergence — and (2) the benign `--accept-flake-config` warning is silenced for
portability. rs-modde's `release.yml` is regenerated from the updated generator
and `simit init release --check` is clean.

## Why this matters now

`release.yml`'s `validate-tag` step builds a detached worktree of the tag and
asserts on it, but every later step (`git archive HEAD`, `nix build .#modde…`)
runs in the **job checkout** = HEAD. On a real **tag push** these coincide
(checkout == tag), so the divergence is harmless for Phase 07. But under
`workflow_dispatch` on trunk — the iteration path used for runs 196–199 — it
**validated `1e4a555` while building `69e23df`**, which is confusing and can mask
a genuine tag/HEAD mismatch. Making the build operate on the validated worktree
makes both trigger paths behave identically and removes a class of "passed
validation but shipped the wrong tree" bugs for all simit users.

Separately, the log warning
`ignoring untrusted flake configuration setting 'extra-substituters' / Pass
'--accept-flake-config'` is **benign on atlas** (the trusted container is a
client of the host nix-daemon; the SDK is GC-pinned + bind-mounted), but it is
noise and would matter on a runner lacking those system substituters; emitting
`--accept-flake-config` (or `accept-flake-config = true` in `NIX_CONFIG`) is
correct, portable, and cheap.

**Why optional:** the real release (Phase 07) is driven by a **tag push**, where
checkout == validated tag, so the divergence does not bite the actual release.
This phase improves robustness and `workflow_dispatch` ergonomics but is **not
required** to ship 0.2.1 green. Defer it freely if the critical path is the
priority — just then do Phase 06's validation via real `-rc.N` **tag pushes**
(not `workflow_dispatch` on trunk), so checkout == tag.

## Out of scope

- Do **not** hand-edit rs-modde's `release.yml` — change the generator and
  regenerate (a hand-edit reports as `ci=drift`).
- Do **not** touch simit's `init_flake`/`user_config`/main-tree WIP.
- Do **not** release simit here (no version bump, no tag) — that's a separate
  effort; this phase leaves the change committed on a branch for the maintainer.
- Do **not** change rs-modde sources or the windows build (Phase 01).

## Plan

1. **Locate the render path:** in simit, `src/render/release_workflow.rs` (the
   comprehensive `release.yml` generator) and the validate-tag/worktree + build
   step emission. Confirm how the tag worktree dir is created and where the build
   steps `cd`.
2. **Design the validate/build parity fix.** Options (pick the cleanest):
   - Have the build/SRPM/SBOM steps operate inside the validated tag worktree
     dir (export it from validate-tag, `cd` into it for the build steps), so
     both trigger paths build the validated commit; or
   - Make `validate-tag` (and the whole job) check out the resolved
     `$VERSION` tag up front when triggered by `workflow_dispatch`, so HEAD == tag
     uniformly.
   Preserve current tag-push behaviour exactly (no regression for the normal
   trigger).
3. **Add `--accept-flake-config`** to the generated `nix build` invocations (or
   set `accept-flake-config = true` in the job `NIX_CONFIG`). Make it
   configurable if simit's design prefers (a knob), else emit it unconditionally
   — it's safe on trusted and untrusted runners alike.
4. **Update tests/fixtures** so the render fixtures and `init release --check`
   idempotence reflect the new output; run simit's full test suite + clippy.
   (Note: the pre-existing `tests/projects.rs` sandbox/registry failures are
   environmental — confirm your change is regression-free against pristine HEAD,
   don't chase those.)
5. **Regenerate rs-modde's `release.yml`** with the updated simit (built from
   source, or via the flake input bumped to the branch) and review the diff:
   the build steps now target the validated tag worktree, and the `nix build`
   lines carry `--accept-flake-config`. Confirm `simit init release --check` is
   clean (idempotent) and `simit init {ci,aur,copr,apt} --check` stay clean.
6. **Commit** the simit change on a topic branch (e.g. extend/replace
   `feat-workspace-version-bump` or a fresh `feat-release-validate-build-parity`)
   and the rs-modde regenerated `release.yml` on trunk/topic. Do **not** tag or
   release simit. If rs-modde consumes a released simit pin, note that the regen
   requires either a local simit build or a simit release first — and if a simit
   release is needed to land this, that escalates it off the 0.2.1 critical path
   (defer per "Why optional").

## Acceptance criteria

- [ ] simit's generated `release.yml` builds the **validated tag worktree** (not
      branch HEAD) under `workflow_dispatch`, with tag-push behaviour unchanged —
      demonstrated by a render-fixture/unit test.
- [ ] The generated `nix build` lines carry `--accept-flake-config` (or the job
      sets `accept-flake-config = true`).
- [ ] simit test suite + clippy pass (modulo the known-environmental
      `tests/projects.rs` failures, confirmed identical on pristine HEAD).
- [ ] rs-modde `release.yml` regenerated; `simit init release --check` and
      `simit init {ci,aur,copr,apt} --check` are all clean (idempotent).
- [ ] No hand-edits to `release.yml`; simit `init_flake`/`user_config` WIP
      untouched; no simit tag/release created by this phase.

## Files likely touched

- `/data/nvme0/can/Projects/simit/src/render/release_workflow.rs` (+ any
  validate-tag/build step helpers it calls).
- `/data/nvme0/can/Projects/simit/tests/*` (render fixtures, `init release`
  idempotence).
- `/data/nvme0/can/Projects/rs-modde/.forgejo/workflows/release.yml`
  (regenerated output, not hand-edited).

## Pitfalls

- **Breaking the tag-push path while fixing the dispatch path.** The fix must be
  a no-op for the normal tag-push trigger. Symptom: tag-push runs start building
  a worktree of a tag they already are. Recovery: gate the worktree-build on the
  trigger, or make HEAD==tag the invariant both ways and confirm the fixture for
  the tag-push case is unchanged.
- **`--check` drift after regen.** If you regenerate rs-modde but the generator's
  fixture wasn't updated, `init release --check` reports drift. Recovery: update
  the fixture and the generator together; run `--check` twice.
- **Needing a simit release to land in rs-modde.** rs-modde pins simit by flake
  input; regenerating with the branch requires a local build or a temporary
  input override. If a real simit release is required, this phase has escalated
  beyond "optional hardening" — defer and use tag-push validation in Phase 06.
- **Scope-creeping into other generator features.** Touch only the validate/build
  parity and the flake-config flag; leave the multi-channel step emission alone.

## Reference

- Divergence evidence: `release.yml` validate-tag (`git worktree add --detach
  "$tag_worktree" "$VERSION"`) vs build steps (`git archive HEAD`,
  `nix build .#modde…` in the job checkout).
- simit architecture + prior work: the multichannel-packaging memory and
  `/data/nvme0/can/Projects/simit/docs/src/planning/multichannel-packaging-release/`.
- Skill: `simit-dependent-fixes` (decide generator-vs-downstream fix).
- Consumers: Phase 06 (uses the regenerated workflow if this lands) and the
  whole-set generator-not-hand-edit constraint in the plan README.
