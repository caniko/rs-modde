# Phase 05a — winget manifest + publish

> **Recommended Codex model: GPT 5.5 medium**
>
> Generate winget 1.6 multi-file manifests (`installer`, `defaultLocale`, `version`), validate with `winget validate`, and open a PR against `microsoft/winget-pkgs` on each tag using `wingetcreate update`. Mechanical with one wrinkle: winget prefers MSI/MSIX installers and accepts portable `.zip`/`.exe`; we ship `.tar.gz` which winget does _not_ support, so we'll need to also emit a `.zip` (or repackage as portable EXE). Bumps it from `low` to `medium`.

## Working tree

- `.forgejo/workflows/release.yml` — emit `.zip` alongside `.tar.gz` for Windows, add winget publish step.
- `dist/winget/` (new) — template manifest files.

## Goal

1. Release emits `modde-<v>-x86_64-windows.zip` (in addition to `.tar.gz`) — winget accepts `.zip` as a portable installer.
2. On each tag, `wingetcreate update Caniko.Modde --version <v> --urls <zip-url>` runs in CI, generates a manifest PR, and submits to `microsoft/winget-pkgs`.
3. Manifest includes correct SHA256 (cross-checked against `SHA256SUMS.txt`), `InstallerType: zip`, `NestedInstallerType: portable`, `NestedInstallerFiles` listing `modde.exe` and `modde-ui.exe`.
4. Once accepted, users can `winget install Caniko.Modde`.

## Why

winget is the default Windows package manager on Win11; for a cross-platform tool, it's table stakes. Currently Windows users must download a tarball, which is unusual on Windows (Windows users expect `.zip` or installer).

## Out of scope

- MSI installer (separate enhancement; defer).
- MSIX packaging (separate; defer).
- Submitting `Caniko.ModdeUI` as a separate winget package — bundle both binaries in one for now.

## Plan

1. Add `.zip` emission alongside the existing `.tar.gz` for windows in the release workflow:
   ```bash
   nix shell nixpkgs#zip -c bash -c \
     "cd release/windows-x86_64 && zip ../modde-${VERSION}-x86_64-windows.zip modde.exe modde-ui.exe"
   ```
2. Add `.zip` to `SHA256SUMS.txt` and to the Codeberg release upload loop.
3. Create `dist/winget/` with template manifests for `Caniko.Modde`:
   - `Caniko.Modde.installer.yaml` — installer type, URLs, SHA256, scope, MinimumOSVersion.
   - `Caniko.Modde.locale.en-US.yaml` — name, publisher, description, license, tags.
   - `Caniko.Modde.yaml` — PackageVersion + manifest type version.
4. Add Forgejo secret `WINGET_PAT` (GitHub PAT with `repo` scope on the user's `winget-pkgs` fork).
5. Add release-workflow step:
   ```bash
   if [ -z "${WINGET_PAT:-}" ]; then echo "skipping winget"; exit 0; fi
   nix shell nixpkgs#wingetcreate -c bash <<'SCRIPT'
   wingetcreate update Caniko.Modde \
     --version "$VERSION" \
     --urls "https://codeberg.org/caniko/rs-modde/releases/download/${VERSION}/modde-${VERSION}-x86_64-windows.zip" \
     --token "$WINGET_PAT" \
     --submit
   SCRIPT
   ```
   (Note: `wingetcreate` is .NET-based — if not in nixpkgs, install via `dotnet tool install -g wingetcreate`.)
6. First-time submission is manual: open the initial PR with `wingetcreate new Caniko.Modde` from a workstation; record PR URL in `audit-report.md`. CI handles updates only.
7. Install docs: add `winget install Caniko.Modde` to the Windows section.

## Acceptance criteria

- [ ] Release emits `modde-<v>-x86_64-windows.zip` and it appears in `SHA256SUMS.txt`.
- [ ] `winget validate dist/winget/` (or whatever path holds the template) passes locally.
- [ ] On a real tag, a PR appears at `microsoft/winget-pkgs` from the configured fork, with correct version + SHA256.
- [ ] After acceptance, `winget install Caniko.Modde` on a fresh Win11 installs both `modde.exe` and `modde-ui.exe` to PATH.
- [ ] Step is no-op (with warning) when `WINGET_PAT` is missing.

## Files likely touched

- `.forgejo/workflows/release.yml`
- `dist/winget/Caniko.Modde.installer.yaml`, `.locale.en-US.yaml`, `.yaml` (new)
- `docs/site/content/docs/getting-started/installation.md`

## Pitfalls

- winget's `winget-pkgs` repo has strict naming: `Caniko.Modde` must match the Publisher.Identifier convention exactly; pick once and document, since renaming requires a new package submission.
- `InstallerType: zip` requires `NestedInstallerFiles` enumerating every exe winget should add to PATH — don't forget `modde-ui.exe`.
- PR review can take days; CI submission must be idempotent (re-submitting the same version should no-op, not error).
- The PAT must be on a fork the bot owns, not on `microsoft/winget-pkgs` directly.

## Reference

- winget manifest spec: https://github.com/microsoft/winget-pkgs/tree/master/doc
- wingetcreate: https://github.com/microsoft/winget-create
- Phase 05c — signed binaries before zipping.
