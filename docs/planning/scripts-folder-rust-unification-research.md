# Scripts Folder Rust Unification Research Dossier

## Goal And Trigger

The maintainer wants to delete the `scripts/` folder and replace every shell
script under it with Rust CLI tooling. The end state is a single unified Rust
CLI that absorbs every operation that today lives as bash. The trigger is the
desire to eliminate bash from rs-modde so that all repo-side tooling is built,
linted, tested, and shipped through the same Cargo/Nix/CI pipeline that the
product itself uses.

This research underwrites the implementation. It captures what the existing
scripts do, who invokes them, what runtime tooling they orchestrate, where
the wrapping happens (flake.nix, Forgejo workflows, justfile), and which
seams a Rust replacement must preserve to avoid regressing a tagged release.

## Current Reality

`scripts/` is the only `*.sh` location in the working tree
(`find . -name "*.sh" -not -path "./target/*" -not -path "./.direnv/*"
-not -path "./.git/*"`), and it contains exactly five top-level scripts plus
one `smoke/` subdirectory:

| Path | LOC | Purpose |
| ---- | --- | ------- |
| [scripts/deploy-pages.sh](../../scripts/deploy-pages.sh) | 60 | Build `.#site` via Nix and force-push it to the `pages` branch on `origin` (or `DEPLOY_REMOTE`) for Codeberg Pages. |
| [scripts/publish-apt.sh](../../scripts/publish-apt.sh) | 116 | Stage all `release/*.deb` files into a reprepro tree, sign with the apt-repo GPG key, then commit + push the rendered `dists/` and `pool/` to `caniko/modde-apt` on Codeberg. |
| [scripts/update-3077-fixture.sh](../../scripts/update-3077-fixture.sh) | 4 | One-line wrapper around `cargo run -p modde-sources --bin update-wabbajack-fixture -- 3077`. |
| [scripts/update-lotf-fixture.sh](../../scripts/update-lotf-fixture.sh) | 4 | One-line wrapper around `cargo run -p modde-sources --bin update-wabbajack-fixture -- lotf`. |
| [scripts/smoke/run-smoke.sh](../../scripts/smoke/run-smoke.sh) | 64 | Discovery driver: iterates `scripts/smoke/smoke-*.sh`, captures stdout/stderr per script, writes `release/smoke-report.txt`. |
| [scripts/smoke/common.sh](../../scripts/smoke/common.sh) | 104 | Shared bash helpers: `die`, `warn`, `need`, `require_args`, `find_one`, `find_many`, `collect_many`, `assert_version_output`, `run_version_check`, `root_run`. |
| [scripts/smoke/policy.toml](../../scripts/smoke/policy.toml) | 12 | Declarative policy: "lintian and rpmlint are warnings; everything else is blocking." Currently read by humans only — `run-smoke.sh` doesn't parse it. |
| [scripts/smoke/smoke-linux-tarball.sh](../../scripts/smoke/smoke-linux-tarball.sh) | 35 | Extract `modde-<v>-{x86_64,aarch64}-linux.tar.gz`, run `modde --version` (qemu-aarch64/binfmt for aarch64). |
| [scripts/smoke/smoke-appimage.sh](../../scripts/smoke/smoke-appimage.sh) | 17 | `chmod +x` the AppImage, `timeout 30 ./modde-<v>.AppImage --version`, assert version match. |
| [scripts/smoke/smoke-darwin-tarball.sh](../../scripts/smoke/smoke-darwin-tarball.sh) | 45 | Extract Darwin tarballs, `file modde` to confirm Mach-O + arch, optional `lipo -info` for universal binaries. |
| [scripts/smoke/smoke-windows-zip.sh](../../scripts/smoke/smoke-windows-zip.sh) | 43 | `unzip` + `tar xzf` the Windows artifacts, `osslsigncode verify`, run `modde.exe --version` under `wine` with `timeout 60`. |
| [scripts/smoke/smoke-deb.sh](../../scripts/smoke/smoke-deb.sh) | 41 | `debootstrap` a bookworm chroot, install the `.deb`s, run `lintian --pedantic` (warning-only), chroot-run `modde --version`. Requires root. |
| [scripts/smoke/smoke-srpm.sh](../../scripts/smoke/smoke-srpm.sh) | 42 | `podman run` a Fedora container, `rpmbuild --rebuild` the SRPM, `rpmlint --strict` (warning-only), `modde --version`. |
| [scripts/smoke/smoke-flatpak.sh](../../scripts/smoke/smoke-flatpak.sh) | 49 | `jq`-patch the manifest's archive URL to a local `file://`, `flatpak-builder --install`, `flatpak run` with `timeout 10` (124 = pass). |
| [scripts/smoke/smoke-appstream.sh](../../scripts/smoke/smoke-appstream.sh) | 13 | `appstreamcli validate --strict dist/com.tartanoglu.modde.metainfo.xml`. |
| [scripts/smoke/smoke-signatures.sh](../../scripts/smoke/smoke-signatures.sh) | 61 | `minisign -V SHA256SUMS.txt[.minisig]`, then for every artifact: `cosign verify-blob` + `cosign verify-blob-attestation` (either keyed via `COSIGN_PUBLIC_KEY`/`keys/cosign.pub`, or keyless via OIDC identity). |
| [scripts/smoke/smoke-sbom.sh](../../scripts/smoke/smoke-sbom.sh) | 18 | `grype sbom:<file> --fail-on high` for every `release/*.cdx.json`. |

Total: 728 LOC of bash.

## Evidence Inventory

### Invocation seams

These are the only entry points; replacing them is the externally-visible
contract:

- [.forgejo/workflows/release.yml:535](../../.forgejo/workflows/release.yml#L535)
  invokes `nix run .#release-smoke -- "$CODEBERG_REF_NAME" release` between
  the build job and the publish steps. Five `release-artifacts-modde-*.yaml`
  workflows do the same (e.g. `release-artifacts-modde-cli.yaml:183`,
  `release-artifacts-modde-core.yaml:183`, etc.).
- [.forgejo/workflows/release.yml:1108](../../.forgejo/workflows/release.yml#L1108)
  shells out directly to `bash scripts/publish-apt.sh` inside a
  `nix shell nixpkgs#reprepro nixpkgs#gnupg nixpkgs#git` block.
- [.forgejo/workflows/pages.yml:34](../../.forgejo/workflows/pages.yml#L34)
  runs `DEPLOY_REMOTE=pages-origin nix run .#deploy-pages` after building
  `.#site` on `trunk` pushes.
- [flake.nix:1292](../../flake.nix#L1292) defines `apps.deploy-pages` as a
  `pkgs.writeShellApplication` that runtime-inputs `git nix coreutils
  findutils` and inlines `builtins.readFile ./scripts/deploy-pages.sh`.
- [flake.nix:1303](../../flake.nix#L1303) defines `apps.release-smoke` as a
  `pkgs.writeShellApplication` whose `runtimeInputs` is the full external
  toolset: `appstream coreutils cosign debootstrap dnf5 dpkg file findutils
  flatpak flatpak-builder git gnugrep gnutar grype gzip jq minisign
  osslsigncode podman qemu rpm unzip wineWow64Packages.stable`. The script
  body just `exec`s `scripts/smoke/run-smoke.sh`.
- [flake.nix:1354](../../flake.nix#L1354) sets `simitConfig.release.smoke.command
  = "nix run .#release-smoke --"`, so simit-driven releases re-enter via the
  same Nix app.
- [crates/modde-sources/tests/fixtures/README.md:16-28](../../crates/modde-sources/tests/fixtures/README.md)
  documents the two `update-*-fixture.sh` wrappers; these are the only
  documented callers.

The `justfile` does not reference scripts/. `cargo xtask` does not call
scripts/. No `Makefile` exists.

### Existing Rust-side surfaces

- [crates/modde-xtask/src/main.rs](../../crates/modde-xtask/src/main.rs) is
  already a `clap` CLI with subcommands: `check`, `test`, `lint`, `fmt`,
  `fmt-check`, `gui`, `run`, `coverage`, `release`, `copr {vendor,
  vendor-check, srpm}`, `nix {build, develop}`, `docs`. It delegates to
  `harbor-xtask` (`git = "https://codeberg.org/caniko/rs-harbor.git"`) for
  shared logic like `run_copr_srpm`. It is `publish = false` and is
  workspace-internal tooling — exactly the right home for repo-ops
  subcommands that aren't part of the product CLI.
- [crates/modde-cli/src/main.rs:40](../../crates/modde-cli/src/main.rs#L40)
  has a hidden top-level `Dev` subcommand with one action,
  `ExportToolSchema`. This is the only product-CLI hook that exists for
  internal repo tasks, and the justfile uses it via `cargo run -p modde-cli
  --quiet -- dev export-tool-schema`. The `Dev` group is `#[command(hide =
  true)]`.
- [crates/modde-sources/src/bin/update-wabbajack-fixture.rs](../../crates/modde-sources/src/bin/update-wabbajack-fixture.rs)
  already implements the fixture refresh logic in Rust; the two
  `update-*-fixture.sh` files are pure shell-trampolines that pass `"$@"`
  through. There is no `Cargo.toml` `[[bin]]` entry — the bin lives under
  `src/bin/` and Cargo auto-discovers it. The crate has `publish = true`,
  which means the bin currently ships with crates.io publishes of
  `modde-sources`.

### External tools that must remain reachable

Even after the rewrite, the underlying smoke checks are external-tool
orchestration: `cosign`, `minisign`, `grype`, `appstreamcli`, `osslsigncode`,
`wine`, `flatpak`, `flatpak-builder`, `podman`, `debootstrap`,
`dpkg-deb`/`lintian`, `rpmlint`/`rpmbuild`, `tar`, `unzip`, `file`, `lipo`,
`qemu-aarch64`, `timeout`, `jq`, `reprepro`, `gpg`, `git`. A Rust rewrite
mostly replaces `bash` with `std::process::Command` plus structured error
reporting. There is no value in re-implementing reprepro, cosign, etc. in
Rust — they are the source of truth.

### Existing planning context

[docs/planning/release-integration-audit/08-pre-publish-smoke-matrix.md](release-integration-audit/08-pre-publish-smoke-matrix.md)
specified the original smoke matrix and explicitly chose `scripts/smoke/*.sh`
as the implementation shape. Its Acceptance criteria are already satisfied;
the rewrite must preserve them at the workflow level (smoke runs before
publish, regressions fail the workflow, `release/smoke-report.txt` is still
uploaded, `force_publish` bypass still works).

[docs/planning/release-integration-audit/04-linux-channels/deb-and-apt.md:12,79](release-integration-audit/04-linux-channels/deb-and-apt.md)
treats `scripts/publish-apt.sh` as the canonical implementation surface for
the apt channel. Rewriting it does not change the channel's external
behavior; it changes the in-repo file the workflow shells into.

The audit-report at
[docs/planning/release-integration-audit/audit-report.md:133](release-integration-audit/audit-report.md#L133)
references `scripts/publish-apt.sh` as the documented publish path. After
the rewrite the link needs to point at the new Rust command instead.

## Existing Plan Status

| Plan item | Status | Evidence |
| --------- | ------ | -------- |
| `scripts/smoke/` exists with one script per category | done | All 10 smoke targets present. |
| `release.yml` runs smoke after build, before any external publish | done | `.forgejo/workflows/release.yml:535`. |
| `release/smoke-report.txt` uploaded as release asset | done — verify after rewrite | `run-smoke.sh:62` writes the report. |
| `workflow_dispatch.force_publish` bypass | unknown — out of scope here | The current dossier doesn't depend on it. |
| `scripts/publish-apt.sh` published artifacts on stable tags only | done | `release.yml:1108` is gated on `IS_PRERELEASE=false`. |
| `apps.deploy-pages` deploys `.#site` to Codeberg Pages branch | done | `flake.nix:1292`, `pages.yml:34`. |
| Wabbajack fixture updater is Rust | done | `update-wabbajack-fixture.rs` is the implementation; shell scripts are 4-line trampolines. |

No plan items become obsolete from the rewrite. The plan documents
themselves require pointer updates (file paths change) but their decisions
stand.

## Work That Should Survive

These are non-negotiable durable behaviors the rewrite must keep:

1. **Smoke runs against the `release/` directory** populated by the build
   step. The current contract is `(VERSION, RELEASE_DIR)` positional args;
   `RELEASE_DIR` defaults to `release` and is always called with `release`
   from CI.
2. **Per-target failure isolation.** A linux-tarball failure must not stop
   the windows-zip check from running. The report shows all results
   together with `pass=N fail=M` summary, then exits non-zero if any
   blocked.
3. **Policy distinction**: `lintian` and `rpmlint` outputs are
   non-blocking warnings; everything else is blocking. Today this is
   enforced inside `smoke-deb.sh` and `smoke-srpm.sh` themselves (they
   `warn` instead of `die`), not by the driver reading `policy.toml`.
4. **Report file**: `release/smoke-report.txt` must contain the per-target
   stdout/stderr plus a final tally. CI uploads it.
5. **The Nix app boundary**: `nix run .#release-smoke -- "$VERSION" release`
   is what every CI workflow references. The downstream rename can be
   `release-smoke` → invoking the unified Rust tool, but the
   `nix run .#release-smoke -- $VERSION release` invocation must stay
   compatible until every workflow file is updated atomically. (Or commit
   a workflow-side update in the same PR — see Risks.)
6. **APT publish gate semantics**: when `APT_REPO_GPG_KEY`,
   `APT_REPO_GPG_KEY_ID`, or `APT_REPO_PUSH_TOKEN` is unset, the publish
   step must `exit 0` with a `::warning::` annotation, not fail. This
   matches Homebrew/Scoop/Flathub gate semantics and keeps tag pushes
   green before the repo is bootstrapped.
7. **Pages deploy idempotence**: empty diff against `pages` branch must
   exit 0 with no commit/push (see `deploy-pages.sh:51-54`).
8. **Reprepro tree shape**: `conf/distributions` comes from
   `dist/apt/conf/distributions` (checked in). After build the tree is
   `dists/` + `pool/` + `key.gpg.asc` + a `.codebergpages.toml`, with
   `README.md` preserved.
9. **Fixture refresher CLI shape**: `--url <authored-files-url>` overrides
   the default "latest authored file" selection. Currently passed through
   `$@`.

## Blockers And Missing Artifacts

None. All foundational inputs are available:

- Source scripts are in-tree.
- CI workflows are readable.
- `harbor-xtask` is already a dependency, so the rewrite can extend it or
  follow its established `run_*` pattern.
- The Nix flake exposes the runtime-inputs list that bounds the external
  tool surface.

## Risks And Constraints

### R1. Release CI is the hot path

The smoke driver runs on every tagged release. A regression here doesn't
just delay a feature — it blocks every package channel (Codeberg release,
Homebrew tap, COPR, AppImage, AUR, eventually winget/Scoop/Flathub). The
rewrite **must** be functionally equivalent before any CI cutover.
Mitigation: keep `scripts/smoke/` and the new Rust tool side-by-side for
one tag cycle, run both, compare reports.

### R2. The Nix flake `apps.release-smoke` knows the entire toolchain

`flake.nix:1303-1340` enumerates every binary the smoke checks need. The
Rust tool inherits the same dependency surface — it doesn't shrink. The
flake wrapper still needs to either (a) inject those into `PATH` before
`exec`ing the Rust binary, or (b) the Rust binary's package derivation
needs `propagatedBuildInputs = [appstream cosign debootstrap ...]`. Option
(a) matches the current model and is lower risk.

### R3. `root_run` semantics

`smoke-deb.sh` does `debootstrap` and chroot operations that need root.
`common.sh:96-103` falls back to `sudo` and `die`s if neither root nor
sudo is available. A Rust rewrite needs the same fallback. The Forgejo
runner topology is the atlas self-hosted runner — confirm the new tool
runs with the same uid/sudo posture. There is no plan to elevate Rust to
running as root by default.

### R4. GPG and key material handling in `publish-apt.sh`

`publish-apt.sh` does `gpg --import` into a per-run `GNUPGHOME` and uses
the imported key id with reprepro's `SignWith`. The Rust port must keep
the throwaway-`GNUPGHOME` discipline; secret key material must never be
written to a path that survives the run. `tempfile::tempdir()` with
`Drop` cleanup matches the current `trap 'rm -rf "$work"' EXIT`. The
embedded passphrase file path must be `chmod 600`. The remote URL
rewriter `printf '%s' ... | sed "s#https://#https://caniko:${TOKEN}@#"`
must be replaced with a structured URL rebuild (don't shell-interpolate
the token).

### R5. Force-push to `pages` branch is destructive by design

`deploy-pages.sh:57` does `git push origin "HEAD:pages" --force`. This is
intentional (the branch is generated content). The Rust port must keep
this contract but use a structured remote URL and never log the URL after
inserting a token (no token is used in `deploy-pages.sh` today — it
relies on the runner's `git credential` config). Keep that.

### R6. Workflow file sprawl

`nix run .#release-smoke` is referenced in **six** workflow files (one
`release.yml` + five `release-artifacts-modde-*.yaml`). Cutover must
update all six in the same commit, or keep the Nix app name
`release-smoke` pointing at the new Rust tool so workflows stay
unchanged. Recommend keeping the Nix app names stable and only changing
the script body inside `flake.nix`.

### R7. The fixture trampolines are zero-risk

They are 4 lines of shell that already shell out to a Rust binary. The
"replacement" is just deletion of the `.sh` files and a README pointer
change. Optionally add `cargo xtask fixture {3077, lotf}` as a
convenience wrapper, but this is not load-bearing.

### R8. Choosing the CLI home: `modde-xtask` vs `modde-cli` vs a new bin

- `modde-cli` is the **product** CLI shipped to end users. Adding
  release-ops commands to it (apt publishing, smoke validation,
  Pages deploy) bloats the binary, makes `modde --help` longer for end
  users, and forces a publish-impact analysis for every internal-only
  change. The existing `Dev { ExportToolSchema }` hook is fine for
  developer-affecting subcommands the *product* needs (schema export
  feeds Home Manager); it should not become the home for release-ops.
- `modde-xtask` is `publish = false`, already aliased to `cargo xtask`,
  already has subcommand groups (`copr`, `nix`), and already orchestrates
  release-adjacent commands via `harbor-xtask`. **This is the right home.**
- A brand-new `modde-ops` crate is unnecessary overhead — same audience
  as xtask, same lifecycle, same publish-false posture.

Recommendation: extend `modde-xtask` with `smoke`, `deploy-pages`,
`publish-apt`, and (optionally) `fixture` subcommands. Keep the existing
public surface (`check`, `test`, `lint`, `fmt`, etc.) untouched.

### R9. `harbor-xtask` upstreaming

Some of these operations (apt repo publishing, Codeberg Pages deploy,
release smoke driver) are not modde-specific — they are reusable Rust
release-ops. `harbor-xtask` already lives upstream at
`codeberg.org/caniko/rs-harbor`. Decide per-subcommand whether the logic
belongs in `harbor-xtask` (and re-used by other rs-* projects) or stays
in `modde-xtask` (modde-specific reprepro `distributions` file, modde's
Codeberg `pages` branch shape, modde's smoke matrix). Default to "start
modde-local, lift to harbor when a second consumer needs it" to avoid
upstream churn for a single user.

### R10. Wine, podman, debootstrap require Linux + capabilities

The smoke runner already only runs on the atlas Linux runner. A Rust
rewrite doesn't change that. Document the assumption in the new
subcommand's `--help` and fail fast with a clear error if invoked on
macOS/Windows or without `sudo`/`podman`/`flatpak`.

### R11. Crates.io publish exposure of `update-wabbajack-fixture`

`modde-sources` is `publish = true` and `src/bin/update-wabbajack-fixture.rs`
is auto-discovered by Cargo. The fixture-updater bin currently ships
with every crates.io publish of `modde-sources`. That's not necessarily
wrong (some downstream consumers may want it), but if the unification
moves the entrypoint into `modde-xtask`, the bin in `modde-sources` can
be deleted, shrinking the crates.io footprint. Decide whether to move
or duplicate.

## Candidate Next Steps

These are sequenced by risk, not by file count. Each step is independently
shippable.

### Step 1 — Delete trivial trampolines, update fixture docs

- Delete `scripts/update-3077-fixture.sh` and `scripts/update-lotf-fixture.sh`.
- Update `crates/modde-sources/tests/fixtures/README.md` to point at
  `cargo run -p modde-sources --bin update-wabbajack-fixture -- 3077`
  (and `lotf`). Optionally add `cargo xtask fixture {3077,lotf}` as a
  wrapper around the same bin for symmetry with the rest of `xtask`.
- No CI impact; these scripts are not in any workflow.

### Step 2 — Port `publish-apt.sh` to `cargo xtask publish-apt`

- Implement `crates/modde-xtask/src/commands/publish_apt.rs` (or fold
  into `harbor-xtask` if reusable).
- Inputs: env vars `VERSION`, `APT_REPO_GPG_KEY`, `APT_REPO_GPG_KEY_ID`,
  `APT_REPO_PUSH_TOKEN`, optional `APT_REPO_GPG_PASSPHRASE`,
  optional `APT_REPO_REMOTE`. Same gate semantics: missing key or token
  → log warning, exit 0.
- External tools called via `Command`: `gpg --batch --import`,
  `reprepro -b $work/apt includedeb stable $deb`, `git clone`,
  `git add -A`, `git commit`, `git push`.
- Sensitive material handling: `tempfile::tempdir()` for `GNUPGHOME`,
  `0700` perms, drop-on-exit cleanup; `Url::parse` + structured username
  injection for the push URL; never log the URL after token insertion.
- CI change: `release.yml:1108` becomes
  `nix shell nixpkgs#reprepro nixpkgs#gnupg nixpkgs#git -c cargo xtask publish-apt`
  (or `nix run .#xtask -- publish-apt`). Update `audit-report.md:133`
  link.
- Delete `scripts/publish-apt.sh` only after the new code is exercised
  once on a real (or `--dry-run`) tag.

### Step 3 — Port `deploy-pages.sh` to `cargo xtask deploy-pages`

- Implement `crates/modde-xtask/src/commands/deploy_pages.rs`.
- Inputs: optional env `DEPLOY_REMOTE` (default `origin`); the script
  reads `git rev-parse --show-toplevel` for the repo root.
- External tools: `nix build .#site --no-link --print-out-paths`,
  `git ls-remote`, `git clone --depth 1 --branch pages`, `cp -rL` (use
  `walkdir` + `fs::copy` and dereference symlinks since the Nix store
  output is symlinked), `git add -A`, `git commit`, `git push --force`.
- Empty-diff short-circuit: parse `git diff --cached --quiet` exit code.
- Replace `flake.nix:1292-1300` so `apps.deploy-pages` invokes the new
  Rust binary with `git`, `nix`, `coreutils` still in `runtimeInputs`.
  `.forgejo/workflows/pages.yml:34` continues to call
  `nix run .#deploy-pages`; no workflow change needed.
- Delete `scripts/deploy-pages.sh` last.

### Step 4 — Port the smoke matrix to `cargo xtask smoke`

This is the largest piece. Design it as a single Rust binary with one
internal "check" per existing smoke script. Suggested shape:

```text
cargo xtask smoke run <VERSION> [--release-dir release] [--checks ...] [--only ...] [--skip ...]
```

- The driver iterates the registered checks, runs them in sequence (today
  they are sequential; parallelism is a follow-up), captures their
  per-check output, writes `release/smoke-report.txt`, exits non-zero
  if any blocking check failed.
- Each check is a Rust function with the signature
  `fn run(ctx: &SmokeCtx) -> Result<CheckOutcome>`, returning
  `Pass | Warn { msg } | Fail { msg }`. Policy enforcement (lintian and
  rpmlint downgrade to Warn) is centralized in the driver, not buried in
  each check. This finally makes `scripts/smoke/policy.toml` load-bearing
  instead of decorative — but the simpler path is to encode the policy
  in code as a `const WARNING_TOOLS: &[&str] = &["lintian", "rpmlint"];`
  and delete the toml.
- External tools stay external; the Rust code just wraps `Command`.
- Replace `flake.nix:1303-1338` so `apps.release-smoke` invokes
  `cargo xtask smoke run` (the `runtimeInputs` toolset stays identical).
  All six CI workflow files that call `nix run .#release-smoke -- ...`
  continue to work unchanged.
- Delete `scripts/smoke/` last, after one full tagged release runs through
  the Rust driver cleanly.

### Step 5 — Final cleanup

- Remove `scripts/` directory entirely (it should be empty by this
  point).
- Update `docs/planning/release-integration-audit/audit-report.md` and
  `docs/planning/release-integration-audit/08-pre-publish-smoke-matrix.md`
  to point at the new `cargo xtask {smoke,publish-apt,deploy-pages}`
  commands. The accepted plan stays; only the implementation pointer
  changes.
- Update `CONTRIBUTING.md` if it mentions any of these scripts (current
  grep shows only `cargo xtask` references for the dev loop; nothing for
  the deleted scripts).
- Add `cargo xtask smoke`, `cargo xtask publish-apt`, `cargo xtask
  deploy-pages` to the `xtask` help block in CONTRIBUTING.md.

### Dependencies and parallelism

- Steps 1, 2, 3, 4 are independent of each other and can land in any
  order. Step 1 is risk-free; ship it first to clear noise.
- Each port should land as a separate PR so the diff is reviewable.
- Step 5 depends on 1–4 being complete.

## Open Decisions For The User

1. **Home for the unified CLI.** Recommendation: `modde-xtask`. Reject
   if the user prefers a new `modde-ops` crate, `modde-cli dev *`
   surface, or pushing everything into upstream `harbor-xtask` from day
   one. The recommendation is grounded in: xtask is `publish = false`,
   already has subcommand groups, and is the convention `cargo xtask`
   uses across this maintainer's projects (see `CONTRIBUTING.md:22`).

2. **`harbor-xtask` upstreaming policy.** Move logic to harbor-xtask
   eagerly (so the next rs-* project gets it free), or lazily (when a
   second consumer materializes). Recommend lazy — modde is the only
   confirmed consumer right now, and harbor-xtask churn affects every
   downstream.

3. **Keep `scripts/smoke/policy.toml`?** It is human-documentation only
   today. Recommend deleting it and encoding the warning/blocking policy
   in Rust constants in the smoke driver. The user may want to keep it
   if they expect external tooling (or a future Forgejo annotation
   parser) to read it.

4. **`modde-sources` keeps or drops the `update-wabbajack-fixture`
   bin.** Keeping it means external consumers of the published
   `modde-sources` crate can refresh fixtures. Dropping it means the
   bin lives only in `modde-xtask` (which is `publish = false`).
   Recommend dropping after `modde-xtask` absorbs the entrypoint —
   the bin is dev tooling, not library surface.

5. **Cutover timing for smoke.** Run the new Rust smoke driver in
   parallel with the bash driver for one tag cycle, or hard cutover.
   Recommend parallel for one cycle — the cost is two reports, and the
   safety is high since the bash driver is already battle-tested. After
   one green parallel run, delete `scripts/smoke/`.

6. **Fixture `cargo xtask` wrapper.** Add `cargo xtask fixture {3077,
   lotf} [--url <u>]` for symmetry, or skip and let users call the
   modde-sources bin directly. Recommend adding the wrapper — costs
   ~20 LOC and matches the "one unified CLI" goal.
