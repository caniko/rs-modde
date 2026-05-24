# Phase 08 — Pre-publish smoke & verification matrix

> **Recommended Codex model: GPT 5.5 high**
>
> This is the last line of defence before assets become public. A bad release that ships to Codeberg + Homebrew + COPR + winget + AUR + Flathub in one shot is *very* hard to roll back across all channels. The smoke matrix has to actually exercise each artifact (not just check it exists), interpret failure modes, and decide whether a partial pass should still publish. Worth `high` because the design judgement (which failures block the release vs warn) is non-trivial and the orchestration spans every channel landed in phases 04–07.

## Working tree
- `.forgejo/workflows/release.yml` — insert a smoke job between "Build release artifacts" and the publish steps.
- New: `scripts/smoke/` — one shell script per smoke target (`smoke-linux-tarball.sh`, `smoke-appimage.sh`, `smoke-deb.sh`, `smoke-windows-zip.sh` via wine, `smoke-flatpak.sh`, `smoke-srpm.sh`).

## Goal
Before any external publish step (Codeberg release / Homebrew / COPR / AUR / winget / Scoop / Flathub / crates.io), run a matrix of smoke checks against the *actual artifacts* in `release/`:
1. **Tarballs**: extract → run `modde --version` → match `$VERSION`. Do for linux-x86_64, linux-aarch64 (via qemu-user-static), windows-x86_64 (via wine), darwin-x86_64 (via universal binary check — execution skipped on Linux).
2. **AppImage**: `chmod +x` → run `./modde-<v>-x86_64.AppImage --version` → match.
3. **`.deb`**: `dpkg-deb -I` inspect, `lintian --pedantic` (warn-only on style), `dpkg -i` in an ephemeral Debian container, then `modde --version`.
4. **SRPM**: `rpmlint --strict` (warn-only on style); `rpmbuild --rebuild` in a Fedora container; install built RPM; `modde --version`.
5. **Flatpak manifest**: `flatpak-builder --user --install-deps-from=flathub` builds successfully; `flatpak run` (with `--no-sandbox` if needed) launches.
6. **Windows EXE**: `osslsigncode verify`, wine-based `wine modde.exe --version`.
7. **macOS tarball**: extract → `file modde` confirms Mach-O for both arches → `lipo -info` if a universal binary. No notarization check (out of scope; users accept Gatekeeper override).
8. **AppStream metainfo**: `appstreamcli validate --strict`.
9. **Signatures from phase 02**: `minisign -V`, `cosign verify-blob`, `cosign verify-attestation`.
10. **SBOM from phase 03**: `grype` and/or `osv-scanner` against `*.cdx.json` — fail on `critical`/`high` CVEs, warn on lower.

If any **blocking** check fails, the publish phase short-circuits before any external push. Document which checks are blocking vs warning.

## Why
Today the workflow assumes nix-built binaries are correct and pushes immediately to ~5 external systems. A regression that's caught by nix-build (e.g. compile error) is fine; a regression that nix-build doesn't catch (broken runtime dep, broken AppImage payload, bad desktop file, missing icon, signature against wrong file) ships everywhere simultaneously.

## Out of scope
- Integration tests that exercise actual mod installation. Those belong in CI on PRs, not in the release path.
- Long-running scenario tests. Smoke is fast (≤5min total).
- Cross-arch smoke on actual ARM hardware — qemu-user-static is fine for release-gate purposes.

## Plan
1. Add a new `smoke` job to `release.yml` that depends on `release` (or insert smoke steps into the single `release` job between build and publish — pick whichever is simpler for the atlas runner topology).
2. For each artifact category, write a small `scripts/smoke/*.sh` that:
   - Sets `set -euo pipefail`.
   - Accepts the version + artifact path as args.
   - Returns 0 on pass, 1 on fail, with a clear error message.
3. Smoke runner driver:
   ```bash
   pass=0; fail=0
   for s in scripts/smoke/smoke-*.sh; do
     name=$(basename "$s" .sh)
     if "$s" "$VERSION" release/; then
       echo "[PASS] $name"; pass=$((pass+1))
     else
       echo "[FAIL] $name"; fail=$((fail+1))
     fi
   done
   echo "smoke: $pass pass, $fail fail"
   test "$fail" -eq 0
   ```
4. Blocking-vs-warning policy: codify in a `scripts/smoke/policy.toml` or inline in the driver. Default to *all blocking* until a specific check is downgraded; lintian/rpmlint are warning-by-default.
5. Containers for smoke: use `nix shell nixpkgs#wine nixpkgs#qemu nixpkgs#podman nixpkgs#debootstrap nixpkgs#dnf5` rather than Docker-Hub images, for hermetic reproducibility.
6. Smoke results go to `release/smoke-report.txt` and are uploaded as a release asset (transparency).
7. Verify with a real tag: tag a `0.0.0-smoke-test` pre-release, run the workflow, confirm smoke runs and a synthetic failure (e.g., temporarily break `modde --version` in the AppImage) blocks publish.

## Risk profile
- **Failure mode 1**: smoke false positives block legitimate releases. Mitigation: a manual `force_publish` workflow_dispatch input that bypasses smoke (audit-logged).
- **Failure mode 2**: smoke is too slow → release blocked for hours. Budget ≤10 min wall-clock; parallelize across smoke targets if needed.
- **Failure mode 3**: smoke tests rot — `modde --version` keeps passing while the actual UI is broken. Mitigation: add at least one `modde-ui` smoke that exits cleanly after launch (use a `--smoke-test` flag or `XDG_RUNTIME_DIR=/tmp timeout 5 modde-ui --help`).

## Acceptance criteria
- [ ] `scripts/smoke/` contains one script per artifact category listed in the Goal.
- [ ] `release.yml` runs all smoke scripts after build and before any external publish step.
- [ ] A deliberate regression (e.g., corrupted AppImage) causes the workflow to fail at the smoke stage, not at publish.
- [ ] `release/smoke-report.txt` is uploaded as a release asset and shows pass/fail per check.
- [ ] `workflow_dispatch` exposes a `force_publish` boolean input that bypasses smoke when set (audit-logged in the workflow run).
- [ ] Smoke wall-clock ≤ 10 min on the atlas runner.

## Files likely touched
- `.forgejo/workflows/release.yml`
- `scripts/smoke/*.sh` (new directory)
- `scripts/smoke/policy.toml` (new, optional)

## Pitfalls
- Running `modde-ui` headless on the atlas runner: Wayland/X11 absent. Use `XDG_RUNTIME_DIR` workarounds or a `--smoke-test` no-window entry point. Avoid `xvfb` if possible — it masks real GPU init failures that users would hit.
- `rpmbuild --rebuild` requires network for Rust crates unless vendored. The COPR step already runs `cargo xtask copr vendor`; smoke can reuse that vendored tree.
- `lintian --fail-on warning` will fail on cosmetic things; default to `--fail-on error`.
- Wine startup is slow (~3s); budget accordingly.
- Smoke must run on artifacts *as they will be published*, including signatures from phase 02. Order matters: build → sign → smoke → publish. Don't run smoke on unsigned binaries.

## Reference
- Phase 02 (signing), Phase 03 (SBOM), Phases 04–07 (artifacts to smoke).
- `nixpkgs#wine` for Windows smoke, `nixpkgs#qemu-user-static` for arm64 smoke on x86.
