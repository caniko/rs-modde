# Phase 05 — Wire `simit` into CI for drift detection and release-path smoke testing

> **Recommended Codex model: gpt-5.4, effort high**
>
> Two policy decisions to land in YAML + docs: (1) we deliberately do NOT run
> `simit init-ci --check` because rs-modde's CI is bespoke and would always
> diverge, and (2) we add a small dry-run smoke job that exercises the simit
> release path on every PR. The first is a "don't do X, document why" call
> that's easy to fumble into "let's just disable the check". The second is
> mechanical YAML. Sub-agent at `high` because the judgment density on (1)
> warrants the extra effort budget; the mechanical YAML alone wouldn't.

## Working tree

`/data/nvme0/can/Projects/rs-modde`. Depends on **Phase 04** — the smoke job
exercises `cargo xtask release patch --dry-run`, which only works once xtask is
backed by simit.

## Goal

Two CI surfaces gain simit awareness, and one explicit non-decision is documented:

1. A new `release-dry-run` job in [.forgejo/workflows/ci.yml](../../.forgejo/workflows/ci.yml)
   runs `cargo xtask release patch --dry-run -m "ci smoke"` on every PR and push.
   It fails if simit's dry-run fails — that catches CHANGELOG drift, broken
   workspace `cargo metadata`, missing simit binary in the devShell, or any
   regression in the xtask wrapper.
2. CONTRIBUTING.md gains a "Release tooling" section documenting:
   - simit owns version bumps, changelog edits, and tag creation.
   - The xtask wrapper at `cargo xtask release` delegates to simit.
   - rs-modde does **not** run `simit init-ci --check` or `simit init-flake --check`,
     and why (see decision section in this doc; copy the reasoning into
     CONTRIBUTING).
3. The decision against `simit init-ci --check` and `simit init-flake --check` is
   recorded in [docs/planning/simit-integration/DECISION.md](./DECISION.md) (this
   phase creates that file) so future contributors don't re-litigate it from
   scratch.

## Why this matters now

Without the smoke job: the next contributor reshapes CHANGELOG.md, the next
`simit release` call from a maintainer bails, and we discover the regression at
release time. The dry-run is fast (no cargo build, no nix build — just
`cargo metadata` + parser checks; ~5–10s in the existing `runs-on: atlas`
shell), so the cost is negligible.

Without the documented non-decision: a future contributor inevitably tries to
"finish the simit integration" by running `simit init-ci` against rs-modde,
which would emit a workflow that:

- Doesn't build `.#flatpak-manifest`, `.#appimage-cli`, `.#appimage-ui`,
  `.#modde-windows`, or `.#docs`.
- Doesn't push to Attic.
- Doesn't validate against `nix flake check`.
- Uses simit's default runner labels (e.g. `codeberg-small`), not the `atlas`
  self-hosted runner this project requires.
- Doesn't call `cargo xtask coverage --ci`, replacing it with a plain
  `cargo test` instead.

So `simit init-ci --check` would *always* fail against rs-modde's bespoke
workflows, which means either we constantly suppress it (drift becomes noise)
or we constantly fight it (re-running init-ci destroys the bespoke CI). Neither
is a good outcome. The right call is to write down "we're not using it" once.

## Out of scope

- Replacing any bespoke job (lint, test, build, flake-check) in `ci.yml` with
  simit-generated equivalents. The bespoke jobs stay.
- Adding `simit init-flake --check`. The flake is heavily customised
  ([flake.nix](../../flake.nix) is 700+ lines with rs-harbor + cross-compile +
  flatpak + appimage). `simit init-flake` would clobber it; `--check` would
  always diverge. Same reasoning as above; same decision.
- Upstreaming a simit feature to make `init-ci --check` configurable enough for
  rs-modde to consume. That belongs in the simit repo, not here.
- Touching `release.yml` (the tag-triggered release pipeline). This phase only
  edits the PR-and-push CI (`ci.yml`).

## Plan

1. **Add the `release-dry-run` job** to [.forgejo/workflows/ci.yml](../../.forgejo/workflows/ci.yml).
   Append after the existing `flake-check` job:
   ```yaml
   release-dry-run:
     runs-on: atlas
     steps:
       - uses: https://code.forgejo.org/actions/checkout@v4
         with:
           fetch-depth: 0
       - uses: https://github.com/cachix/install-nix-action@v27
         with:
           extra_nix_config: |
             experimental-features = nix-command flakes
             substituters = https://attic.candee.baby/canix https://cache.nixos.org
             trusted-public-keys = canix:uqr0nD3I0mfj9BYfZgTZHMaDKfI2yCTtSA5JGTWKUeg= cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=
       - name: Verify simit availability
         run: nix develop --command simit --version
       - name: Dry-run a patch release through xtask
         run: nix develop --command cargo xtask release patch --dry-run -m "ci smoke"
       - name: Verify dry-run left the working tree clean
         run: |
           if [ -n "$(git status --porcelain)" ]; then
             echo "::error::simit release --dry-run modified the working tree"
             git status --porcelain
             exit 1
           fi
   ```
   `fetch-depth: 0` is required because simit's git preflight reads the full
   history to check tag-existence and HEAD signing state; the default shallow
   clone breaks that.

2. **Write [docs/planning/simit-integration/DECISION.md](./DECISION.md)** with
   sections:
   - `# simit integration decisions`
   - `## simit owns versioning; rs-modde keeps bespoke CI`
   - `## We do not run `simit init-ci --check`` — paste the bullet list from the
     "Why this matters now" section explaining what bespoke pieces simit's
     generator does not produce.
   - `## We do not run `simit init-flake --check`` — explain that flake.nix is
     rs-harbor-driven and customised, that simit's init-flake assumes a vanilla
     crane setup, and that the two are intentionally incompatible.
   - `## Revisit triggers` — name the conditions that would change the calculus:
     simit gains configurable extra jobs / extra packages / runner override;
     rs-harbor publishes a `mkSimitCi` helper; or rs-modde's CI becomes
     vanilla. Until one of those, the decision stands.

3. **Edit [CONTRIBUTING.md](../../CONTRIBUTING.md)** to add a "Release tooling"
   section under whatever release/contribution documentation already exists.
   Keep it short — 6-10 lines:
   - `cargo xtask release {patch|minor|major|prerelease} -m "<message>"` is the
     entry point.
   - It delegates to `simit release` from the devShell.
   - Tags are bare-semver (no `v` prefix); the release workflow triggers on
     `[0-9]*` tags.
   - CHANGELOG.md must keep its `## [Unreleased]` heading exactly as-is.
   - Link to `docs/planning/simit-integration/DECISION.md` for the rationale on
     why we don't auto-generate CI from simit.

4. **Lint the workflow**:
   ```
   nix shell nixpkgs#yamllint -c yamllint .forgejo/workflows/ci.yml
   ```

5. **Local rehearsal**: run the smoke job's commands by hand inside `nix
   develop` to make sure they pass:
   ```
   nix develop --command simit --version
   nix develop --command cargo xtask release patch --dry-run -m "ci smoke"
   git status --porcelain  # must be empty
   ```

6. **Commit**:
   ```
   git add .forgejo/workflows/ci.yml CONTRIBUTING.md docs/planning/simit-integration/DECISION.md
   git commit -m 'ci: add simit release-dry-run smoke job and document scope'
   ```

## Acceptance criteria

- [ ] `.forgejo/workflows/ci.yml` contains a `release-dry-run` job that runs `cargo xtask release patch --dry-run -m "ci smoke"`.
- [ ] The job uses `fetch-depth: 0` on the checkout step (simit needs full history).
- [ ] The job has an explicit `git status --porcelain` post-check that fails CI if simit modified files.
- [ ] `docs/planning/simit-integration/DECISION.md` exists and explicitly documents non-use of `simit init-ci --check` and `simit init-flake --check`, with reasons.
- [ ] CONTRIBUTING.md has a "Release tooling" section naming the simit-backed `cargo xtask release` entry point, bare-semver tag convention, and the `## [Unreleased]` invariant.
- [ ] `yamllint .forgejo/workflows/ci.yml` exits 0 (with the existing rule disables intact).
- [ ] Running the smoke job's commands locally in `nix develop` exits 0 and leaves a clean working tree.
- [ ] Existing CI jobs (`lint`, `test`, `build`, `flake-check`) are unchanged — verify with `git diff HEAD~1 .forgejo/workflows/ci.yml` showing only additions.
- [ ] Commit message is `ci: add simit release-dry-run smoke job and document scope`.

## Files likely touched

- [.forgejo/workflows/ci.yml](../../.forgejo/workflows/ci.yml) — append `release-dry-run` job.
- [CONTRIBUTING.md](../../CONTRIBUTING.md) — append "Release tooling" section.
- [docs/planning/simit-integration/DECISION.md](./DECISION.md) — new file.

## Pitfalls

- **Smoke job runs on every push** including release tags. That's fine — the dry-run is read-only and the `release.yml` runs on tag pushes too; both can coexist. If duplicate runs become noisy, gate with `if: github.ref_type != 'tag'` on the release-dry-run job.
- **`cargo xtask release patch --dry-run` mutates `Cargo.lock`**: confirm against the Phase 03 / Phase 04 dry-run behavior. If simit's lockfile update is part of the *plan-and-print* (not the dry-run), this won't happen. If it does, two options: (a) check out `Cargo.lock` after the smoke job with `git checkout -- Cargo.lock` (and remove the porcelain check, which would fail); (b) report it upstream as a simit dry-run bug. Default to (a) only if simit confirms the behavior is intentional.
- **Atlas runner may not have `cargo` on PATH outside `nix develop`**. Every command in the new job must be prefixed with `nix develop --command`. Forgetting this on the `git status` post-check is fine — `git` is available outside the dev shell.
- **The smoke job depends on Phase 02's devShell change being on `trunk`**. If you merge Phase 05 before Phase 02 propagates, the `simit --version` step fails. Order matters; verify Phase 02 + Phase 04 are merged before opening this PR.
- **The DECISION.md file lives under `docs/planning/`** which is intentionally outside the published mdBook (rs-modde uses `docs/site/` for Zola content, separate from `docs/planning/` which is plan-set internal). Don't link to it from public-facing README sections; CONTRIBUTING.md is the right cross-reference target.
- **Re-litigation defence**: if a future contributor opens a PR to add `simit init-ci --check`, the DECISION.md file gives them the prior reasoning. Make sure the "Revisit triggers" section is specific enough to refute "let's just suppress the failing checks" — name the *positive* conditions (simit gains configurability, rs-harbor publishes mkSimitCi, etc.) that would actually change the calculus.

## Reference

- Current CI: [.forgejo/workflows/ci.yml](../../.forgejo/workflows/ci.yml)
- Current release pipeline (out of scope for this phase): [.forgejo/workflows/release.yml](../../.forgejo/workflows/release.yml)
- simit init-ci renderer (read this to confirm what it does and doesn't emit): [simit/src/render/ci.rs](../../../../simit/src/render/ci.rs)
- simit init-flake renderer: [simit/src/commands/init_flake.rs](../../../../simit/src/commands/init_flake.rs)
- Phase 04 (xtask backend swap; prerequisite): [04-swap-xtask-release-backend.md](./04-swap-xtask-release-backend.md)
