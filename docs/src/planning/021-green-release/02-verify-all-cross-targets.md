# Phase 02 — Verify all 8 release cross-targets build green

> **Recommended Codex model: GPT 5.5 low**
>
> Mechanical verification: run each flake build target, read pass/fail, record.
> Leaf/sub-agent role at low-moderate complexity — no design decisions, the only
> judgement is interpreting a build failure enough to route it back to the right
> phase. `low` effort suffices; bump to `medium` only if darwin/appimage throw a
> non-obvious failure that needs real diagnosis (then it likely belongs back in
> Phase 01's strategy, not here).

## Working tree

`/data/nvme0/can/Projects/rs-modde`. Depends on **Phase 01** (the windows fix
must be committed). Build-only — this phase does not edit `release.yml` or any
source, so it never conflicts with Phase 05's regeneration. darwin targets
require the macOS SDK and so can only be proven on the **atlas** runner (or a
host with the GC-pinned SDK store path); locally they will fail with a missing
SDK and that is expected, not a regression.

## Goal

Every release artifact target the workflow builds is confirmed green: the seven
Linux-buildable targets locally, and the two darwin targets on atlas (or via a
build-only CI dispatch). No target is left in an unknown/untested state going
into the prerelease run, so Phase 06 fails (if at all) on publish wiring, not on
a surprise compile error 40 minutes into a cold build.

## Why this matters now

Run 199 only ever exercised `.#modde` (x86_64) and started `.#modde-aarch64-linux`
before being killed; **no run has built the full target set end to end**. With
Phase 01 fixing windows, the remaining risk is an untested target (a darwin SDK
wiring gap, an AppImage runtime regression, a flatpak manifest eval error)
surfacing only during the expensive ~1 h cold release build. Proving each target
in isolation first is far cheaper than discovering it mid-release.

## Out of scope

- Do **not** fix the windows build here — that's Phase 01. If windows still
  fails, bounce back.
- Do **not** push tags or trigger a release run (Phases 06/07).
- Do **not** modify `release.yml`, `flake.nix`, or sources. This is read/build
  only; the only writes are nix `result*` symlinks.
- Pre-warming the Attic cache is **optional** (see step 5) — not required for
  acceptance.

## Plan

1. **Confirm Phase 01 landed:** `git log --oneline -3` shows the windows fix
   commit; `nix build .#modde-windows -L` is green.
2. **Build every Linux-buildable target locally**, capturing each result:
   ```
   for t in modde modde-aarch64-linux modde-windows appimage-cli appimage-ui flatpak-manifest; do
     echo "=== $t ==="; nix build ".#$t" -L --no-link 2>&1 | tail -5
   done
   ```
   All six must succeed. (`flatpak-manifest` produces a JSON manifest, not a
   binary — success = the derivation builds.)
3. **darwin targets:** attempt `nix build .#modde-darwin-x86_64 -L` and
   `.#modde-darwin-aarch64 -L` locally. Expect a missing-macOS-SDK failure on a
   machine without the GC-pinned SDK — that is **not** a code defect. To prove
   them for real, use a **build-only** check on atlas: either the `forgejo-ci`
   path, a throwaway minimal workflow that only runs `nix build .#modde-darwin-*`
   on `runs-on: atlas-nix-trusted`, or have the maintainer run the two builds on
   atlas directly. Record the atlas result (BUILT / failed-with-reason).
4. **Cross-check against the workflow's asset list.** Open `release.yml` "Build
   release artifacts" step and confirm the targets you verified match exactly
   what it builds and uploads (x86_64-linux, aarch64-linux, windows .exe + dll,
   2× darwin tarballs, 2× AppImage, flatpak manifest, srpm, debs). Any target in
   the workflow not on your green list is a gap.
5. **(Optional) pre-warm the Attic cache** to shorten the cold release build:
   push the realized Linux/appimage/flatpak store paths to the canix cache so
   the release run substitutes instead of recompiling. Use the `canix-cli` /
   `update-canix` Attic-push path or `attic push canix <paths>`. darwin paths
   can only be pushed from atlas (the producer). Log exactly which paths were
   warmed; if skipped, note the release run will cold-build (~1 h, under the 3 h
   timeout).

## Verification outcome — 2026-06-02

Run after the Phase 01 blocker was resolved (the actual blocker hit was **not**
`PowrProf.h` — that workaround holds — but an incomplete `db.rs` → `db/` refactor
left untracked, so Nix's git-filtered flake source omitted `db/`, `config.rs`, and
`postgres_parity.rs`; staged them and the windows build went green).

| Target | Result | Notes |
|---|---|---|
| `.#modde` | BUILT | native x86_64-linux |
| `.#modde-aarch64-linux` | BUILT | cross ELF binaries |
| `.#modde-windows` | BUILT | `modde.exe`, `modde-ui.exe`, `modde-xtask.exe`, `libmcfgthread-2.dll` |
| `.#appimage-cli` | BUILT | wraps `${modde}/bin/modde` |
| `.#appimage-ui` | BUILT | wraps `${modde}/bin/modde-ui` |
| `.#flatpak-manifest` | BUILT | JSON manifest (writeText) |
| `.#modde-darwin-x86_64` | atlas-only | local fail expected (see below) |
| `.#modde-darwin-aarch64` | atlas-only | local fail expected (see below) |

All six Linux-buildable targets are green locally. The two darwin targets fail
locally for **two pre-existing reasons, neither caused by this branch** (verified
both fail in *dependency* compilation, upstream of any modde code):

1. **SDK not in the build sandbox.** The SDK is pinned as a context-free literal
   in `OSXCROSS_SDK`/`OSXCROSS_SDKROOT` (not an input-closure member), and the
   *daemon's* `/etc/nix/nix.conf` substituters lack `harbor-macos-sdk`, so under
   `sandbox = true` the valid `…-macosx-sdk-26.1/MacOSX26.1.sdk` is never
   bind-mounted. One-off local workaround (trusted user):
   `nix build .#modde-darwin-x86_64 --option extra-sandbox-paths /nix/store/<hash>-macosx-sdk-26.1`.
2. **`openssl-sys` (pulled by `git2`, pre-existing — not from the new sqlx/postgres)
   has no OpenSSL for darwin.** `darwinArgs.buildInputs = []` (flake.nix ~270),
   unlike windows/aarch64. Even with the SDK in the sandbox, deps fail with
   "system library `openssl` … not found."

Per this phase's own design, a local darwin failure is documented-expected, not a
defect. Prove darwin on atlas (SDK bind-mounted + GC-pinned). A real *local* darwin
fix would need openssl provisioning (vendored, or cross-openssl + `OPENSSL_DIR`).

## Acceptance criteria

- [ ] `nix build` is green for all of: `.#modde`, `.#modde-aarch64-linux`,
      `.#modde-windows`, `.#appimage-cli`, `.#appimage-ui`,
      `.#flatpak-manifest` (locally).
- [ ] `.#modde-darwin-x86_64` and `.#modde-darwin-aarch64` are proven green on
      atlas (or by the maintainer on a SDK-equipped host); a local-only SDK
      failure is documented as expected, not counted as a pass or a defect.
- [ ] Every target the `release.yml` "Build release artifacts" step builds is
      on the green list — no untested target remains.
- [ ] A one-line per-target status table is recorded (BUILT / atlas-only /
      reason) for handoff to Phase 06.

## Files likely touched

- None persistent. Only nix `result*` symlinks (gitignored) and, if step 3 uses
  a throwaway workflow, a temporary file the maintainer removes after (do not
  commit it). Optional Attic pushes mutate the cache, not the repo.

## Pitfalls

- **Treating a local darwin failure as a blocker.** Without the GC-pinned SDK,
  local darwin builds fail by design. Symptom: `SDK not found` / missing
  `MACOS_SDK`. Recovery: prove darwin on atlas, where the SDK store path is
  bind-mounted and GC-pinned; don't try to "fix" it locally.
- **AppImage needs FUSE/runtime quirks.** `.#appimage-*` may need
  `APPIMAGE_EXTRACT_AND_RUN` or fail under a sandbox. Build the derivation (not
  run the AppImage); derivation success is the bar here.
- **Stale cache hiding a real failure.** If you pre-warmed or have local store
  hits, a "success" might be substituted, not freshly built. For the targets
  Phase 01 touched, build with `--rebuild` once to be sure the source actually
  compiles.
- **Asset-list drift.** The workflow may upload individual binaries *and*
  tarballs; verifying only the tarball target can miss a per-binary copy step.
  Reconcile against the actual step text.

## Reference

- Build step: `release.yml` "Build release artifacts" (the `nix build .#…`
  sequence).
- Prerequisite: [01-fix-windows-cross-build.md](./01-fix-windows-cross-build.md).
- Consumer: [06-prerelease-validation.md](./06-prerelease-validation.md).
- atlas runner + Attic: `atlas-runner`, `canix-cli` skills.
