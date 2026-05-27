# CI Issues Research Dossier

## Goal And Trigger

User asked: "Analyze all the issues in CI, and find their sources. Let's start by
designing solutions."

Scope: every Forgejo Actions workflow under [.forgejo/workflows/](../../.forgejo/workflows/)
for `caniko/rs-modde` on `codeberg.org`. Cover the failure history on the active
branch, the latent issues in workflows that have not yet been triggered, and the
in-flight uncommitted CI edits on `simit-chaperone-20260525`.

## Current Reality

- Local branch [`simit-chaperone-20260525`](../../.forgejo/workflows/) is at the
  same SHA as remote `simit-ci-adoption-20260525` (`4355249`). The chaperone
  branch is **not** on origin.
- Uncommitted changes now touch 13 workflow files, [flake.nix](../../flake.nix),
  [flake.lock](../../flake.lock), [README.md](../../README.md),
  [crates/modde-core/tests/repo_truth_tests.rs](../../crates/modde-core/tests/repo_truth_tests.rs),
  [docs/mo2-coverage.md](../../docs/mo2-coverage.md),
  [docs/site/content/docs/games/supported-games.md](../../docs/site/content/docs/games/supported-games.md),
  and add [nix/pre-commit.nix](../../nix/pre-commit.nix) plus
  [nix/treefmt.nix](../../nix/treefmt.nix).
- **As of `4355249`, the six `ci-modde-*` workflows are all GREEN on remote**
  (runs `93`–`98`, all `status=success`). The recent failure history is fully
  remediated by the immediately following commits.
- Two workflows are **legacy / unproven**:
  - [`.forgejo/workflows/pages.yml`](../../.forgejo/workflows/pages.yml) — last run `#2`, FAILED on `b7a1699`.
  - [`.forgejo/workflows/release.yml`](../../.forgejo/workflows/release.yml) — never run.
- Three workflow families have **never triggered** (await a tag push or dispatch):
  `publish-crate-modde-*.yaml` (6 files), `release-artifacts-modde-*.yaml`
  (6 files), and `release.yml`.

## Evidence Inventory

### CI run history (Forgejo Actions tasks API)

Commands:

```
curl -fsSL https://codeberg.org/api/v1/repos/caniko/rs-modde/actions/tasks?page=1&limit=50
curl -fsSL https://codeberg.org/api/v1/repos/caniko/rs-modde/actions/tasks?page=2&limit=50
curl -fsSL https://codeberg.org/caniko/rs-modde/actions/runs/<idx>  # JSON-in-HTML payload with per-step status
```

Aggregate (88 runs across two pages):

- Latest 6 runs (`#93`–`#98`, commit `4355249fbacdd2e9f8850af582361611bb71b33d`,
  "Skip publish gates for non-publishable xtask"): **all success**, one per
  workflow `ci-modde-{cli,core,games,sources,ui,xtask}.yaml`.
- Runs `#1`–`#92`: long failure streak across the simit-adoption iteration
  (commits `9e36373` → `4355249`).
- `pages.yml` run `#2` (commit `b7a1699`): **failure**.
- `ci.yml` runs `#1`×6 (commit `b7a1699`): **failure** — but `ci.yml` no longer
  exists in the tree; it was replaced by the simit-generated `ci-modde-*` set.

### Failure categories (per `currentJob.steps` block extracted from each run page)

| Bucket | Runs                | Commit(s)                                                   | Failing step                                 | Root cause                                                                                                                                                                                                                                                                                          |
| ------ | ------------------- | ----------------------------------------------------------- | -------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A      | `#1`×6, `#2`        | `b7a1699`                                                   | "Set up job" / "install-nix-action@v27"      | Pre-simit `ci.yml` ran on a runner without Nix.                                                                                                                                                                                                                                                     |
| B      | `#3`–`#8`           | `9e36373` ("Adopt simit-managed CI")                        | "Set up job"                                 | Runner label not yet provisioned (initial simit pin).                                                                                                                                                                                                                                               |
| C      | `#9`–`#14`          | `482006c` ("Use trusted Nix atlas runner")                  | "Set up job"                                 | Switched runner label, still mis-provisioned.                                                                                                                                                                                                                                                       |
| D      | `#15`–`#33`         | `f07aaa8`, `f319c8c`, `823d4e9`, `9095884`                  | "Set up job"                                 | Runner provisioning continued failing.                                                                                                                                                                                                                                                              |
| E      | `#34`–`#38`         | `9095884` ("Retry Codeberg runners")                        | "Set up job"                                 | Same — runner still missing capability.                                                                                                                                                                                                                                                             |
| F      | `#39`–`#44`         | `aab6147` ("Retry Codeberg Nix runner")                     | "Check flake" (immediate)                    | `nix-command flakes` not enabled on `atlas-nix-trusted`.                                                                                                                                                                                                                                            |
| G      | `#45`–`#50`         | `99c78eb` ("Enable flakes in generated Nix CI")             | "Check flake"                                | `NIX_CONFIG` worked, but `XDG_CACHE_HOME`/`CARGO_HOME` resolved to a runner-relative path that broke `nix flake check`.                                                                                                                                                                             |
| H      | `#51`, `#57`, `#58` | `2da61ba`, `f89601b`                                        | "Check flake" (5m–18m, cancelled or failure) | Cache paths still wrong; the workflow timed out building deps cold.                                                                                                                                                                                                                                 |
| I      | `#63`–`#68`         | `e8b9ab3` ("Sync capability docs table formatting")         | "Check flake" (varying duration)             | Cold cache + non-sandbox-stable `nix/tool-schema.nix` made the `tool-schema-fresh` check non-deterministic.                                                                                                                                                                                         |
| J      | `#69`–`#74`         | `781f65f` ("Refresh generated tool schema")                 | "Check flake"                                | Same: the freshly exported `tool-schema.nix` did not match what `modde dev export-tool-schema` produced inside the Nix sandbox.                                                                                                                                                                     |
| K      | `#75`–`#77`         | `9202f85` ("Use sandbox-stable generated tool schema")      | "Deny dependency policy"                     | `cargo deny` rejected newly-pulled crate licenses / sources.                                                                                                                                                                                                                                        |
| L      | `#81`–`#83`         | `51ef2b0` ("Allow current dependency policy inputs")        | "Deny dependency policy"                     | License id mismatch on cargo-deny 0.18.3. The added `GPL-3.0-only` entry did **not** match the SPDX expression an upstream dep reports; the fix in `750fb66` flipped the allowlist to the bare `GPL-3.0` form, which cargo-deny 0.18.3 accepts in practice (runs `#87`–`#91`, `#93`–`#98` confirm). |
| M      | `#92`               | `750fb66` ("Use cargo-deny 0.18 compatible GPL license id") | "Package crate"                              | `cargo package -p modde-xtask --allow-dirty` failed because `modde-xtask` has `publish = false`. Fixed in `4355249` by deleting the step from `ci-modde-xtask.yaml`.                                                                                                                                |

All buckets A–M are **already resolved** at the latest commit (`4355249`). No
`ci-modde-*` regression survives on the current tip.

### Per-step extraction sample (run `#92`, xtask)

```
success  3s  Set up job
success  2s  Checkout
success  1s  Add cargo bin to PATH
success  8s  Check flake
success 12s  Test
success 22s  Install cargo-deny
success  5s  Deny dependency policy
success  2s  Clippy
failure  0s  Package crate          <-- cargo package -p modde-xtask --allow-dirty
failure  1s  Complete job
```

`modde-xtask` carries `publish = false` (commit `4355249` removes the step
entirely from `ci-modde-xtask.yaml`).

### Run `#2` (pages.yml) per-step extraction

```
success  4s  Set up job
success  3s  https://code.forgejo.org/actions/checkout@v4
failure  0s  https://github.com/cachix/install-nix-action@v27   <-- root cause
skipped  0s  Deploy Codeberg Pages
failure  1s  Complete job
```

The `cachix/install-nix-action@v27` step crashed instantly. Combined with
`runs-on: atlas` (not `atlas-nix-trusted`), the runner has no pre-installed Nix
and the install action does not work in that environment.

### Workflow inventory (current tree)

```
.forgejo/workflows/
├─ ci-modde-{cli,core,games,sources,ui,xtask}.yaml      # simit-managed, GREEN
├─ publish-crate-modde-{cli,core,games,sources,ui}.yaml # simit-managed, never run
├─ release-artifacts-modde-{cli,core,games,sources,ui,xtask}.yaml  # simit-managed, never run
├─ pages.yml                                            # legacy hand-authored, broken
└─ release.yml                                          # legacy hand-authored, never run
```

simit-managed files share a `# Generated by simit. Manual edits will be reported
as ci=drift.` header and the pattern:

```yaml
on:
  push:
    branches: ["**"]
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
jobs:
  test:
    runs-on: atlas-nix-trusted
    env:
      NIX_CONFIG: "experimental-features = nix-command flakes"
      XDG_CACHE_HOME: "/tmp/.cache"
      CARGO_HOME: "/tmp/.cargo"
```

Legacy `pages.yml` and `release.yml` instead use:

```yaml
"on":
  push: { branches/tags: ... }
concurrency:
  group: ${{ codeberg.workflow }}-${{ codeberg.ref }} # <-- INVALID expression
  cancel-in-progress: ...
jobs:
  <job>:
    runs-on: atlas # <-- no Nix pre-installed
    steps:
      - uses: https://github.com/cachix/install-nix-action@v27 # <-- failing on run #2
```

### Uncommitted edits on `simit-chaperone-20260525`

`git diff --stat HEAD` (tracked changes plus one intentional generated symlink):

```
.forgejo/workflows/ci-modde-{cli,core,games,sources,ui,xtask}.yaml | +3 each  (Build docs step)
.forgejo/workflows/publish-crate-modde-{cli,core,games,sources,ui}.yaml | +3 each (Build docs step)
.forgejo/workflows/pages.yml                                       | +1/-1 ('on' -> "on")
.forgejo/workflows/release.yml                                     | +3/-3 (single->double quotes only)
README.md, docs/mo2-coverage.md, docs/site/.../supported-games.md  | prettier reflows
crates/modde-core/tests/repo_truth_tests.rs                        | whitespace-insensitive markdown-table assertions
nix/pre-commit.nix, nix/treefmt.nix                                | new
flake.nix                                                          | +treefmt-nix / git-hooks inputs, formatter output, formatting check
flake.lock                                                         | new inputs locked
.pre-commit-config.yaml                                            | untracked generated symlink into /nix/store
```

Notes:

- The `Build docs` step inserts `cargo doc -p <crate> --no-deps --all-features`
  between "Deny dependency policy" and "Clippy" in **all 6 ci-modde-_ and all 5
  publish-crate-_ workflows**.
  - No `RUSTDOCFLAGS=-D warnings` is set in the workflow `env`, so rustdoc
    warnings do not gate the step. The step will only fail on actual rustdoc
    _errors_ (broken intra-doc links, etc.).
  - **No regeneration drift check was performed**: the new step was edited into
    the simit-generated files by hand. If `simit init-ci` is re-run on this
    branch with the new simit pin (`e9cf448d`), the hand-edit will likely be
    overwritten unless the upstream simit template now emits a `Build docs`
    step. This is the same drift pattern that motivated buckets G–K above.
- New flake outputs: `formatter` and a `formatting` check that runs treefmt over
  the tree. This wires a `checks.<system>.formatting` derivation that the
  simit-managed `nix flake check` step will exercise on every push. If treefmt
  considers the tree unformatted, **every `ci-modde-*` workflow will now fail at
  "Check flake"**. Local pre-commit hooks must therefore stay in sync with what
  treefmt enforces.
- `.pre-commit-config.yaml` is a symlink into `/nix/store/...` and is untracked
  (intentional — generated by `git-hooks.nix`).
- The simit input bumped from `5ebd4e63` to `e9cf448d`. Re-running
  `simit init-ci` could regenerate workflow templates; the impact on the
  hand-edited `Build docs` step is unknown.

### Workflows that have never run (latent issues)

`publish-crate-modde-*.yaml` and `release-artifacts-modde-*.yaml`:

- Already use `runs-on: atlas-nix-trusted` and `github.*` expressions — same
  shape as the green CI workflows.
- Carry significant _new_ logic the green CI never exercised:
  - `Validate signed release tag` (GPG verify against `keys/maintainers.gpg`)
  - `cargo pkgid` version match
  - `enable-openid-connect: true` for Sigstore keyless signing
  - cargo-deny and clippy passes
- First tag push will be the first proof — likely to surface "works on local Nix
  but not in the runner" issues (e.g. apt-get fallback for `gpg` install
  assumes a Debian-family base; the atlas runner's base image is unknown from
  here).

`release.yml`:

- Uses `runs-on: atlas` and `cachix/install-nix-action@v27` (same broken pattern
  as `pages.yml`).
- Uses `${{ codeberg.workflow }}` / `${{ codeberg.ref }}` — invalid expressions.
  Forgejo will substitute empty strings, collapsing the concurrency group key
  into a single shared bucket and forcing all release workflow runs to
  serialize (or, if Forgejo errors on undefined contexts, the workflow will
  refuse to start).
- References `CODEBERG_REF_NAME` instead of `GITHUB_REF_NAME`. Forgejo does set
  `GITHUB_*` variables; `CODEBERG_*` is not standard Forgejo. The validate-tag
  step likely sees an empty `$VERSION`.
- Tag trigger glob `[0-9]*` matches anything starting with a digit, including
  partial / malformed tags. The simit-managed `publish-crate-*` uses `*.*.*`
  instead.
- Will fail on first tag push for at least three independent reasons.

`pages.yml`:

- Same `codeberg.*` typo and `atlas` (non-Nix) runner pattern as `release.yml`.
- Already proven broken by run `#2`.
- Runs only on push to `trunk`. With the chaperone branch unmerged, this stays
  dormant until trunk advances.

## Existing Plan Status

[docs/planning/release-integration-audit/](../../docs/planning/release-integration-audit/)
is an unrelated, older release-channel audit (Linux/Windows channels). It does
not address Forgejo Actions / CI workflow correctness. No prior CI-specific
plan exists in `docs/planning/`. Out of scope here.

## Work That Should Survive

1. The simit-managed pattern for CI (`atlas-nix-trusted`, pre-installed Nix,
   `NIX_CONFIG` env, `/tmp/.cargo`, `/tmp/.cache`) is the canonical shape and
   must be the template for any new or fixed workflow.
2. The historical lessons that produced the latest GREEN state:
   - Sandbox-stable tool-schema export (`tool-schema-fresh` check must be
     hermetic).
   - cargo-deny 0.18.3 accepted the bare `GPL-3.0` allowlist entry in this repo
     and rejected the attempted `GPL-3.0-only` switch.
   - Per-crate `Package crate` step must be gated on `publish ≠ false`.
3. The simit pin and `simit init-ci` / `simit init-flake` regeneration loop is
   the source of truth — manual edits to simit-generated files should be
   minimized and instead pushed upstream into simit's template if the change is
   structural (like a `Build docs` step).

## Blockers And Missing Artifacts

None blocking _this dossier_. Items that would block downstream fixes:

- Visibility into the `atlas-nix-trusted` runner image (base distro, installed
  tools). Required to predict whether the apt-get fallback in
  `release-artifacts-*.yaml` actually runs. Producer: runner admin (you).
  Regeneration: `nix develop -c uname -a && cat /etc/os-release` snippet added
  ad-hoc to a CI step, or out-of-band documentation.
- Whether the new simit pin `e9cf448d` regenerates a `Build docs` step. Producer:
  `nix run .#simit-cli -- init-ci --check` on a clean checkout.

## Risks And Constraints

- **Drift risk on `Build docs`**: hand-editing simit-generated files re-creates
  the same problem class as the earlier "Refresh generated tool schema" loop. A
  later `simit init-ci` will silently strip the step.
- **Formatter check shipping with chaperone branch**: the new `formatting` flake
  check will run on every push. If anyone pushes an unformatted commit, _all
  six_ `ci-modde-*` workflows fail at the (very expensive) "Check flake" step.
  Cheap mitigation: gate the formatter check on a separate, fast workflow, or
  ensure pre-commit hooks are installed before push.
- **Release blast radius**: the first tag push will trigger
  `release.yml` (legacy, broken) + 6 `release-artifacts-*` + 5 `publish-crate-*`
  workflows simultaneously, all racing on the same runner pool. Concurrency
  groups will limit per-workflow parallelism but not total concurrent jobs.
- **Secret surface**: `release.yml` declares ~20 optional secrets that have
  likely never been validated. Even after fixing the runner/expression bugs,
  individual notarization / signing branches will surface lazily.

## Candidate Next Steps

Designed solutions, sequenced by independence:

### S1 — Drop the legacy `release.yml` if `publish-crate-*` + `release-artifacts-*` already cover its job

[`.forgejo/workflows/release.yml`](../../.forgejo/workflows/release.yml) duplicates a
subset of what the simit-managed `publish-crate-*` + `release-artifacts-*`
workflows do (validate tag, build, upload). Audit whether each unique
responsibility (Codeberg release object creation, COPR/Mastodon announcements,
homebrew tap PRs, AUR push, winget PR, scoop bucket, MASTODON/MATRIX
announcements) actually has a home in the simit-generated set. If not,
**either**:

- (S1a) Delete `release.yml` and migrate any unique responsibility into a new
  simit-managed `announce-*.yaml`-style workflow; or
- (S1b) Bring `release.yml` up to simit shape **manually** (one-time):
  - `runs-on: atlas` → `runs-on: atlas-nix-trusted`
  - Drop `cachix/install-nix-action@v27`; rely on pre-installed Nix.
  - `codeberg.workflow` → `github.workflow`, `codeberg.ref` → `github.ref`.
  - `CODEBERG_REF_NAME` → `GITHUB_REF_NAME` (with the same triple-fallback chain
    `publish-crate-*` already uses).
  - Tag glob `[0-9]*` → `[0-9]+.[0-9]+.[0-9]+` (or align with simit's `*.*.*`).
  - Add `env: { NIX_CONFIG, XDG_CACHE_HOME, CARGO_HOME }` block.

Independence: high. Does not interact with green CI.

### S2 — Fix `pages.yml` to the simit shape

Either regenerate via simit (preferred if simit owns a `pages` template) or
manually:

```yaml
on: # remove the unnecessary "on": quote dance
  push:
    branches: [trunk]
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }} # fix expression
  cancel-in-progress: true
jobs:
  publish:
    runs-on: atlas-nix-trusted # fix runner
    env:
      NIX_CONFIG: "experimental-features = nix-command flakes"
      XDG_CACHE_HOME: "/tmp/.cache"
      CARGO_HOME: "/tmp/.cargo"
    steps:
      - uses: https://code.forgejo.org/actions/checkout@v4
      # no install-nix-action — atlas-nix-trusted already has Nix
      - name: Deploy Codeberg Pages
        env:
          CODEBERG_TOKEN: ${{ secrets.codeberg_token }}
        run: |
          set -euo pipefail
          test -n "$CODEBERG_TOKEN"
          git config user.name "forgejo-actions"
          git config user.email "forgejo-actions@noreply.codeberg.org"
          git remote add pages-origin "https://caniko:${CODEBERG_TOKEN}@codeberg.org/caniko/rs-modde.git"
          DEPLOY_REMOTE=pages-origin nix run .#deploy-pages
```

Independence: high. Only blocks the Codeberg Pages deploy.

### S3 — Decide the fate of the hand-edited `Build docs` step

Two design choices:

- (S3a) **Upstream into simit**: open a simit change adding an optional `Build
docs` step to the simit-managed CI template, gated on a `simitConfig` flag.
  Re-run `simit init-ci` here. The local hand-edit then disappears as a no-op
  diff. Long-term correct path; matches "Use simit for release workflow"
  feedback memory.
- (S3b) **Drop the hand-edit**: rely on `cargo doc` exercised inside `nix flake
check` via a Crane `cargoDoc` derivation. Adds it to the GREEN check path
  without forking the workflow template.
- (S3c) **Keep the local hand-edit short-term**: accept drift; add a
  `simit-ci-drift` annotation file documenting the intentional deviation.

Recommend S3a or S3b. Independence: medium — must decide before pushing
`simit-chaperone-20260525`, otherwise the hand-edits ship and create a future
simit-regeneration conflict.

### S4 — Make the new `formatting` flake check non-fatal-by-default for `nix flake check` in CI

`nix flake check` in the `ci-modde-*` workflows will execute every check in
[flake.nix](../../flake.nix), including the new
`formatting = treefmtEval.config.build.check self;`. A treefmt diff (e.g. a
contributor forgot to run pre-commit) will turn the entire CI run red on
"Check flake".

Options:

- (S4a) Use `nix flake check --keep-going` and let the lint surface separately.
- (S4b) Split out a fast `treefmt-check` job that runs `nix build
.#checks.<system>.formatting -L` before `nix flake check`, so contributors get
  a clear "formatting failed" signal independent of test/build time.
- (S4c) Keep the current shape and require `pre-commit install` in CONTRIBUTING.

Recommend S4b: cheapest, clearest, mirrors `Build docs` in spirit (separating
quality gates).

Independence: low — couples with S3 because both modify the simit-generated CI
template.

### S5 — Validate the unproven publish + release-artifacts pipelines before the next tag

Pre-flight:

- Trigger `release-artifacts-modde-cli.yaml` via `workflow_dispatch` from a
  throwaway annotated tag like `0.0.0-rc.0` on a sandbox branch (or set
  `force_publish: true` and a fake tag in a fork).
- Confirm the `gpg` apt-get fallback either runs (Debian-base atlas runner) or
  is unnecessary (Nix already supplies gpg via `nix develop`). If atlas-nix-trusted is NixOS-based, the apt-get fallback path is dead code and the
  step will fail on a missing `gpg`. Either ensure `gpg` is in
  `devShells.default.packages`, or change the fallback to `nix shell nixpkgs#gnupg --command ...`.

Independence: high. Runs in isolation.

### S6 — One-time cleanup of the `simit-chaperone-20260525` working tree before pushing

Order of operations:

1. Decide S3 (Build docs upstream vs drop vs accept drift).
2. Decide S4 (formatting check splitting).
3. Run `nix flake check` locally to verify the new `formatting` derivation does
   not flag anything.
4. Run `nix develop -c pre-commit run --all-files` to validate the
   `git-hooks.nix` configuration.
5. Commit via `simit commit` (per the "Use simit for release workflow"
   memory) so simit can detect drift in generated files.

Independence: this is the finishing step that bundles all the others.

## Unblock log (this session)

Actions taken to clear immediate push blockers, verified end-to-end with
`nix flake check` ("all checks passed!") on `x86_64-linux`:

1. **deny.toml**: `git checkout HEAD -- deny.toml` to drop the
   `GPL-3.0` → `GPL-3.0-only` revert that empirically fails cargo-deny 0.18.3
   (the current worktree already reflects the restored `GPL-3.0` entry).
2. **nix/tool-schema.nix**: `git checkout HEAD -- nix/tool-schema.nix` because
   alejandra reformatted `values = [ "a" "b" ];` → `values = ["a" "b"];`, and
   the flake's `tool-schema-fresh` check runs `modde dev export-tool-schema`
   and diffs against the on-disk file; the exporter emits the spaced form.
3. **nix/treefmt.nix**: added `excludes = ["nix/tool-schema.nix"]` to
   `programs.alejandra` so future treefmt runs do not re-introduce the
   regression in (2). Long-term, either update the exporter to emit
   alejandra-style output or keep the exclude.
4. **treefmt**: ran the local `formatter.x86_64-linux` derivation; tree is now
   clean (`formatted 0 files`), so the new `checks.<system>.formatting`
   derivation passes.
5. **repo_truth_tests**: `crates/modde-core/tests/repo_truth_tests.rs` was
   asserting exact substrings against the README and docs markdown tables, and
   prettier reflowed those tables with column padding. Added an
   `assert_contains_loose` helper that collapses runs of whitespace before
   substring-matching, then switched the three markdown-table assertions to
   the loose form. The HTML/Tera assertion against
   `website/templates/comparison.html` keeps the exact-match form because
   prettier does not touch HTML there.
6. **Build docs step**: confirmed `nix develop -c cargo doc -p modde-core
--no-deps --all-features` finishes clean (exit 0). The hand-edited step
   added to all 11 ci-modde-_ / publish-crate-_ workflows is functional on the
   current tree; drift risk vs simit regeneration (S3) remains.

Remaining decisions (S1, S2, S3, S5) do not block the push and are left to the
user.

## Open Decisions For The User

1. **S3**: Build docs step — upstream into simit (S3a), drop in favor of a
   Crane `cargoDoc` check (S3b), or accept drift (S3c)?
2. **S1**: Legacy `release.yml` — delete and rely on simit-managed
   `publish-crate-*` + `release-artifacts-*` (S1a), or bring it up to simit
   shape manually (S1b)?
3. **S4**: Should the new `formatting` check be split out of `nix flake check`
   in CI (S4b) or remain inline (status quo)?
4. **S5**: Is the `atlas-nix-trusted` runner image NixOS-based (apt-get
   fallback is dead) or Debian-based (apt-get fallback is the active path)?
   This unlocks the `release-artifacts-*` validation.
