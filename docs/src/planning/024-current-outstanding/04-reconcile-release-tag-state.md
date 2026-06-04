# Phase 04 — Reconcile release and tag state

> **Recommended Codex model: GPT 5.5 high**
>
> This phase gates public release state. It involves tags, generated release
> workflows, changelogs, Codeberg remote evidence, and possible human signing.
> High is warranted for orchestration and failure recovery, but not max until
> the maintainer chooses to push a real public tag and supervise downstream
> publishing.

## Working tree

`/data/nvme0/can/Projects/rs-modde`. Start only after Phase 01 and Phase 03 are
accepted. If the maintainer wants the UI async cleanup in the same release, also
wait for Phase 02.

## Goal

The repository has one coherent story for the current version: manifests,
changelogs, local tags, remote Codeberg tags, and Codeberg release artifacts
either all agree on a published `0.3.0`, or the unreleased changes are moved
back under `Unreleased` with no false release claim.

## Why this matters now

Local source says version `0.3.0` and changelogs contain dated `0.3.0` entries.
Local and remote tag inspection during consolidation showed only `0.2.0` and
`0.2.1` on Codeberg. The retired release plan was written around a `0.2.1`
release and is obsolete as a literal plan, but the release consistency and
generated-workflow constraints remain active.

## Out of scope

- Do not hand-edit simit-generated release workflow files to patch release
  logic. Fix the generator and regenerate if a release workflow bug is found.
- Do not push signed tags without maintainer involvement.
- Do not fabricate release evidence. If Codeberg run or release artifacts are
  missing, report them as missing.

## Plan

1. Re-probe tags:
   `git tag --list | sort -V`,
   `git ls-remote --tags origin | sort -V`, and
   `git log --oneline --decorate -20`.
2. Verify source version and changelog:
   `nix eval --raw .#modde.version`,
   `rg -n "^## \\[0\\.3\\.0\\]|\\[0\\.3\\.0\\]" CHANGELOG.md crates/*/CHANGELOG.md`,
   and compare Cargo workspace version.
3. Decide one of two paths with the maintainer:
   - **Publish path:** create/sign/push the missing `0.3.0` tag on the intended
     commit, then supervise the release workflow.
   - **Unreleased path:** move `0.3.0` changelog content back under
     `Unreleased` and keep workspace version/tag links consistent until a later
     release.
4. For the publish path, run the prerelease/real release gates appropriate to
   the current workflow:
   `nix build .#modde .#modde-aarch64-linux .#modde-windows .#appimage-cli .#appimage-ui .#flatpak-manifest`
   plus atlas-only darwin verification when available.
5. Verify Codeberg release run and artifacts. Use `berg` if configured, or
   Codeberg UI/API. Record each downstream channel as published or intentionally
   soft-skipped because a secret is absent.
6. Run final consistency checks:
   `git ls-remote --tags origin | rg "refs/tags/0\\.3\\.0"`,
   `nix eval --raw .#modde.version`, and release artifact probes.

## Acceptance criteria

- [ ] The chosen release path is recorded in a commit or stable release note.
- [ ] Version, changelog compare links, local tags, remote tags, and Codeberg
  release artifacts are consistent for `0.3.0`, or `0.3.0` is no longer claimed
  as a released version.
- [ ] Required release build targets pass or have a recorded, current blocker
  with exact regeneration and validation commands.
- [ ] Any public release run is green; every downstream channel is published or
  intentionally soft-skipped with evidence.
- [ ] No generated release workflow drift is introduced by hand edits.

## Files likely touched

- `CHANGELOG.md`
- `crates/*/CHANGELOG.md`
- `Cargo.toml` only if the version decision changes.
- Tags and Codeberg release state.
- Generated release files only through the project generator, if needed.

## Pitfalls

- **Symptom:** changelog says `0.3.0` but no tag exists. **Cause:** local release
  preparation was committed without the public tag. **Recovery:** choose publish
  or unreleased path and make all artifacts match.
- **Symptom:** tag points at a commit that does not contain release fixes.
  **Cause:** stale tag reuse. **Recovery:** do not move it silently; delete and
  re-sign only with maintainer approval.
- **Symptom:** release run fails midway through downstream publishing. **Cause:**
  missing secret, missing asset, or runner interruption. **Recovery:** record
  exact failed channel, fix forward where possible, and avoid claiming green
  release until all channels are accounted for.

## Reference

- `.forgejo/workflows/release.yml`
- `flake.nix` release artifact definitions and `simitConfig`
- `CHANGELOG.md`
- Predecessor planning docs remain available in git history if historical
  context is needed.
