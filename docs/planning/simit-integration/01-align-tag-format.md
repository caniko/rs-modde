# Phase 01 — Switch release tags from `vX.Y.Z` to bare `X.Y.Z`

> **Recommended Codex model: gpt-5.4-mini, effort medium**
>
> Mechanical edit across three small files (release.toml, .forgejo/workflows/release.yml,
> docs) with one regex rewrite and one trigger-pattern change. Leaf node — no design
> decisions. Routing a frontier-tier model here would be waste; a mini sub-agent at
> medium effort has enough headroom for the YAML/regex care needed.

## Working tree

`/data/nvme0/can/Projects/rs-modde`. No external repos. This is the first phase in the
`simit-integration` plan set — no prerequisite phases.

## Goal

rs-modde produces and consumes bare-semver release tags (`0.2.0`, `1.0.0-rc.1`) instead
of the current `v`-prefixed form (`v0.2.0`). All in-repo tag references — the
`cargo-release` config, the Forgejo release workflow's trigger and validator, and the
CONTRIBUTING/README release docs — are updated in one commit so the next release can be
cut with simit (which only emits bare-semver tags; see
[simit src/git.rs:88-117](../../../../simit/src/git.rs#L88-L117)).

## Why this matters now

simit's `git::tag` writes `refs/tags/{version}` literally — no prefix, no template (see
[simit/src/git.rs:116](../../../../simit/src/git.rs#L116) and the simit publish-crate
workflow at [simit/.forgejo/workflows/publish-crate.yaml:7](../../../../simit/.forgejo/workflows/publish-crate.yaml#L7)
which triggers on `*.*.*`). rs-modde's release path expects `v*`:

- [release.toml:5](../../release.toml#L5): `tag-name = "v{{version}}"`
- [.forgejo/workflows/release.yml:8](../../.forgejo/workflows/release.yml#L8): `tags: ['v*']`
- [.forgejo/workflows/release.yml:28-31](../../.forgejo/workflows/release.yml#L28-L31): validator strips `v` from `GITHUB_REF_NAME` and asserts the strip changed the value

If we adopt simit before reconciling, the first simit-driven tag (e.g. `0.2.0`) will not
fire the release workflow, and even if forced it will fail the validator's
`test "$GITHUB_REF_NAME" != "$VERSION"` check. Reconciling first keeps every subsequent
phase a no-op for the release pipeline.

Deferring this is cheap *until* Phase 04 lands — then it becomes a release-blocking bug.
Better to land it first, behind no other changes, so a `git bisect` for "next release
broke" lands cleanly.

## Out of scope

- Touching `harbor_xtask::run_release` or any `rs-harbor` code. Tag-format coupling is in
  `release.toml`; the harbor helper is config-driven.
- Removing `release.toml` entirely (Phase 04 does that after simit is wired in).
- Renaming any existing git tags (`v0.1.0` stays as-is; this only changes future
  tagging).
- Codeberg release UI re-pointing for the existing `v0.1.0` release.

## Plan

1. **Edit [release.toml](../../release.toml)**: change `tag-name = "v{{version}}"` to
   `tag-name = "{{version}}"` and `tag-message = "v{{version}}"` to
   `tag-message = "{{version}}"`. Leave every other key unchanged — the cargo-release
   backend is still in use until Phase 04.
2. **Edit [.forgejo/workflows/release.yml](../../.forgejo/workflows/release.yml)**:
   - Line 8: change the trigger pattern from `'v*'` to `'[0-9]*'`. The leading
     digit-class avoids matching arbitrary tags while accepting `0.x` and `1.x` series.
     (Forgejo Actions tag filters use glob syntax, not full regex, so `[0-9]*` is the
     correct narrowing.)
   - Lines 28-31: replace the four-line "Validate tag" block with:
     ```yaml
     - name: Validate tag
       run: |
         VERSION="$GITHUB_REF_NAME"
         echo "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$'
         test "$(nix eval --raw .#modde.version)" = "$VERSION"
     ```
     The strip-and-compare guard is replaced by a direct semver match. The
     `nix eval` assertion against `.#modde.version` stays — it's the only thing that
     actually proves the tag matches the workspace version.
3. **Search for stale `v{{version}}` references**:
   ```
   rg -n 'v\{\{version\}\}|"v\*"|tags/v|ref_name.*v' \
     --glob '!target' --glob '!docs/planning'
   ```
   Update CONTRIBUTING.md, README.md, and any docs that show the old format. Do
   not touch `docs/planning/` — past plans are historical record.
4. **Sanity-check the workflow YAML**:
   ```
   nix shell nixpkgs#yamllint -c yamllint .forgejo/workflows/release.yml
   ```
5. **Commit**:
   ```
   git add release.toml .forgejo/workflows/release.yml CONTRIBUTING.md README.md
   git commit -m 'chore(release): switch to bare-semver tags'
   ```
   Do not tag this commit — no version bump yet.

## Acceptance criteria

- [ ] `release.toml` contains `tag-name = "{{version}}"` and `tag-message = "{{version}}"` with no leading `v`.
- [ ] `.forgejo/workflows/release.yml` trigger is `tags: ['[0-9]*']`.
- [ ] `.forgejo/workflows/release.yml` validator uses `$GITHUB_REF_NAME` directly with no `${...#v}` substring stripping.
- [ ] `rg 'v\{\{version\}\}' release.toml .forgejo CONTRIBUTING.md README.md` returns no matches.
- [ ] `nix shell nixpkgs#yamllint -c yamllint .forgejo/workflows/release.yml` exits 0 (warnings about `line-length` are tolerated since the file already disables that rule).
- [ ] `nix flake check --keep-going --print-build-logs` passes — this phase touches no Nix-evaluated files but the gate confirms no accidental regressions.
- [ ] The new validator block, copied into a scratch shell with `GITHUB_REF_NAME=0.2.0` and `GITHUB_REF_NAME=0.2.0-rc.1`, exits 0 for both; with `GITHUB_REF_NAME=v0.2.0` it exits non-zero.
- [ ] Commit message is `chore(release): switch to bare-semver tags` — picked so a future bisect can identify this phase by message.

## Files likely touched

- [release.toml](../../release.toml) — tag-name + tag-message keys.
- [.forgejo/workflows/release.yml](../../.forgejo/workflows/release.yml) — trigger filter + validator.
- [CONTRIBUTING.md](../../CONTRIBUTING.md) — any "tags look like vX.Y.Z" docs (grep first).
- [README.md](../../README.md) — installation/release-notes sections only if they reference tag format.

## Pitfalls

- **Forgejo glob `'[0-9]*'` vs full regex**: Forgejo Actions inherits GitHub Actions' tag-filter syntax — only `*`, `?`, `[set]`, `!` are supported. Do *not* try `'^[0-9]+\..*'` — it will match literally and never fire. If the simpler glob feels too loose, also keep the validator's strict semver regex; the workflow won't proceed without it.
- **Old `v0.1.0` tag still exists**: cosmetic only — historical releases stay as-is. Do *not* rewrite history or delete the tag.
- **cargo-release is still the release backend until Phase 04**: that means if someone cuts a release between Phase 01 and Phase 04, cargo-release will read the new `release.toml` and produce bare-semver tags — that's the desired behavior. The Forgejo release workflow has already been updated in this same commit to accept them.
- **Codeberg release publish payload**: `release.yml:121` posts `tag_name: $tag` — Codeberg accepts arbitrary tag strings, no change needed.

## Reference

- simit's tag emitter: [simit/src/git.rs:88-117](../../../../simit/src/git.rs#L88-L117)
- simit's publish-crate trigger (proof of bare-semver convention): [simit/.forgejo/workflows/publish-crate.yaml:7](../../../../simit/.forgejo/workflows/publish-crate.yaml#L7)
- Current rs-modde release pipeline: [.forgejo/workflows/release.yml](../../.forgejo/workflows/release.yml)
- Prior cargo-release decision (now being reversed): [docs/planning/xtask-tooling-cli-uplift/DECISION.md](../xtask-tooling-cli-uplift/DECISION.md)
- Forgejo Actions tag-filter syntax: matches GitHub Actions glob semantics (https://docs.github.com/en/actions/using-workflows/workflow-syntax-for-github-actions#filter-pattern-cheat-sheet — Forgejo mirrors this).
