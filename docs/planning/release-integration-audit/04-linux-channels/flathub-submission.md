# Phase 04c — Flathub submission flow

> **Recommended Codex model: GPT 5.5 medium**
>
> The `flatpak-manifest` flake output exists and emits `com.tartanoglu.modde.json` to the release, but it's never been submitted to Flathub. Flathub submission is a one-time PR to `flathub/flathub` followed by per-release manifest updates in `flathub/com.tartanoglu.modde`. Mechanical with one judgement call (which Flatpak runtime version to pin — `org.freedesktop.Platform//24.08` vs `//23.08`) and one validation step (`flatpak-builder-lint`).

## Working tree
- `flake.nix` — the `flatpak-manifest` output already exists; review whether it produces a *buildable* manifest (with sources block) or just metadata. Likely needs source URLs+sha256s injected at release time.
- `dist/com.tartanoglu.modde.metainfo.xml` — AppStream metadata; required by Flathub.
- `dist/modde-ui.desktop` — desktop file; required.
- `dist/com.tartanoglu.modde.png` — icon; verify ≥128×128 and a 256×256 variant exists.
- `.forgejo/workflows/release.yml` — new "Publish Flathub" step.

## Goal
1. `com.tartanoglu.modde.json` is a complete, buildable Flatpak manifest with pinned sources (cargo lockfile mapped via `flatpak-cargo-generator.py`).
2. Manifest passes `flatpak-builder-lint manifest com.tartanoglu.modde.json` and `--exceptions` are documented if any.
3. Initial submission PR to `flathub/flathub` exists with the manifest, screenshots, and AppStream metadata.
4. After acceptance, per-release update flow: a publish step opens a PR against `flathub/com.tartanoglu.modde` with bumped `tag` + new `cargo-sources.json`.

## Why
Flathub is the dominant cross-distro Linux desktop install channel; without it, the GUI's reach is limited to "user finds the AppImage URL on the website". COPR covers Fedora workstation, but Flathub covers everything else including Ubuntu/Mint via the GNOME Software / KDE Discover integration.

## Out of scope
- Snap Store submission (separate channel, defer).
- Migrating the AppImage flow to be Flathub-only (keep AppImage for users who can't install Flatpak).
- Wayland-only manifest restrictions — keep X11 fallback for now.

## Plan
1. Inspect `flake.nix`'s `flatpak-manifest` output. If it produces a metadata-only JSON, extend it to a full Flatpak manifest with:
   - `app-id`: `com.tartanoglu.modde`
   - `runtime`: `org.freedesktop.Platform`
   - `runtime-version`: `24.08`
   - `sdk`: `org.freedesktop.Sdk`
   - `sdk-extensions`: `["org.freedesktop.Sdk.Extension.rust-stable"]`
   - `command`: `modde-ui`
   - `finish-args`: `--socket=wayland`, `--socket=fallback-x11`, `--share=network`, `--filesystem=home`, `--device=dri`, `--talk-name=org.freedesktop.secrets`
   - `modules`: a `modde` module with sources block containing the release tarball URL + sha256, plus `cargo-sources.json` from `flatpak-cargo-generator.py`.
2. Add a release step that runs `flatpak-cargo-generator.py Cargo.lock -o release/cargo-sources.json`, and patches the sha256 of the release tarball into the manifest after `SHA256SUMS.txt` is generated.
3. Validate locally: `flatpak-builder --user --install-deps-from=flathub --force-clean build-dir release/com.tartanoglu.modde.json`. Then `flatpak-builder-lint manifest release/com.tartanoglu.modde.json` and `flatpak-builder-lint appstream dist/com.tartanoglu.modde.metainfo.xml`. Address every error; warnings noted in the audit-report.
4. Verify AppStream metadata: `appstreamcli validate --explain dist/com.tartanoglu.modde.metainfo.xml`. Fix any errors (likely: missing `<releases>` block, missing `<screenshots>`).
5. One-time submission (manual maintainer task, not in CI):
   - Fork `flathub/flathub`, create new branch `new-pr/com.tartanoglu.modde`, add `com.tartanoglu.modde.json`, open PR.
   - Document the submission PR URL in `docs/planning/release-integration-audit/audit-report.md` for traceability.
6. Once accepted, Flathub creates `flathub/com.tartanoglu.modde`. Add per-release publish step:
   ```bash
   if [ -z "${FLATHUB_TOKEN:-}" ]; then echo "skipping flathub"; exit 0; fi
   git clone "https://x-access-token:${FLATHUB_TOKEN}@github.com/flathub/com.tartanoglu.modde.git" flathub-repo
   cp release/com.tartanoglu.modde.json release/cargo-sources.json flathub-repo/
   (cd flathub-repo && git checkout -b "release/$VERSION" && \
     git add . && git commit -m "modde $VERSION" && \
     git push origin "release/$VERSION")
   # then open PR via gh CLI
   ```
7. Add `FLATHUB_TOKEN` to Forgejo secrets (GitHub PAT scoped to the Flathub app repo).

## Acceptance criteria
- [ ] `flake.nix`'s `flatpak-manifest` output produces a JSON that `flatpak-builder` consumes end-to-end (manifest → built bundle → `flatpak run com.tartanoglu.modde` launches the UI).
- [ ] `flatpak-builder-lint manifest <manifest>` passes (or `--exceptions` enumerated in `audit-report.md`).
- [ ] `appstreamcli validate dist/com.tartanoglu.modde.metainfo.xml` passes.
- [ ] Initial Flathub submission PR exists and its URL is recorded in `audit-report.md`.
- [ ] Per-release publish step is wired (skipped gracefully when `FLATHUB_TOKEN` unset) and tested with a manual dispatch.
- [ ] Install docs add a "Flathub" section with `flatpak install flathub com.tartanoglu.modde`.

## Files likely touched
- `flake.nix`
- `dist/com.tartanoglu.modde.metainfo.xml` (likely needs `<releases>` + `<screenshots>`)
- `.forgejo/workflows/release.yml`
- `docs/site/content/docs/getting-started/installation.md`
- `docs/planning/release-integration-audit/audit-report.md` (record submission PR URL)

## Pitfalls
- Flathub's reviewer checklist is strict: missing screenshots, missing `<content_rating>` tag, missing `<launchable>`, or a `finish-args` set that requests `--filesystem=host` without justification will block acceptance. Read https://docs.flathub.org/docs/for-app-authors/requirements/ end-to-end before submitting.
- `flatpak-cargo-generator.py` needs to be run with the same `Cargo.lock` as the release tarball; mismatched lockfiles produce a manifest that fails to build offline (Flathub builds offline).
- `org.freedesktop.Platform//24.08` includes a relatively new Mesa; if the UI relies on a Bevy/WGPU version with known Mesa 24 issues, fall back to `//23.08` and document.
- AppStream `<id>` must match the Flatpak app-id exactly: `com.tartanoglu.modde` everywhere.

## Reference
- Flathub submission docs: https://docs.flathub.org/docs/for-app-authors/submission/
- flatpak-cargo-generator: https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo
- Phase 01 audit — Flatpak channel currently emits the manifest but doesn't submit.
