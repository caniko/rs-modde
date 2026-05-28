# Phase 04a — Debian `.deb` packages + APT repo

> **Recommended Codex model: GPT 5.5 medium**
>
> Routine packaging work: a `cargo-deb` manifest in each binary crate, two `.deb` builds in the release job, and a reprepro/aptly script that publishes a Codeberg-hosted APT repo. The judgement calls (split modde vs modde-ui into two packages? what to do about `libssl3` ABI across Debian/Ubuntu?) are well-precedented; `medium` is enough.

## Working tree

- `crates/modde-cli/Cargo.toml` — add `[package.metadata.deb]`.
- `crates/modde-ui/Cargo.toml` — add `[package.metadata.deb]` with X11/Wayland depends.
- `.forgejo/workflows/release.yml` — add deb build + APT publish steps.
- New: `scripts/publish-apt.sh` — reprepro-based.
- New: `dist/apt/conf/distributions` — reprepro config.

## Goal

1. Two `.deb` packages per architecture: `modde_<v>_amd64.deb` (CLI) and `modde-ui_<v>_amd64.deb` (GUI). Same for `arm64`.
2. Packages uploaded as Codeberg release assets, signed (Phase 02 cosign + minisign).
3. APT repo at `https://modde.rs/apt/` (served via Codeberg Pages or the existing website host) with `stable` suite, signed `Release` file using a dedicated **repo GPG key** (separate from maintainer key).
4. Install docs add `apt`-based install (curl key → add source → `apt install modde`).

## Why

`.deb` is the dominant Linux desktop install method (Ubuntu, Mint, Debian, Pop!\_OS); shipping only Fedora SRPM via COPR leaves the largest segment without a native package. AUR covers Arch users only. Flatpak (phase 04c) covers desktop GUI but not the CLI.

## Out of scope

- Submitting to official Debian/Ubuntu repos — high bar (DD sponsorship, multi-year cycle); explicitly defer.
- `.snap` packaging — separate channel; document as deferred in audit report, not in this phase.
- Mirroring the APT repo to a CDN — single-origin is fine for now.

## Plan

1. Add `[package.metadata.deb]` to `crates/modde-cli/Cargo.toml`:
   ```toml
   [package.metadata.deb]
   name = "modde"
   maintainer = "Can H. Tartanoglu <canhtart@gmail.com>"
   depends = "libssl3, libsqlite3-0, libdbus-1-3"
   section = "utils"
   priority = "optional"
   assets = [
     ["target/release/modde", "usr/bin/", "755"],
     ["LICENSE", "usr/share/doc/modde/copyright", "644"],
   ]
   ```
2. Add similar for `crates/modde-ui/Cargo.toml` with `depends = "libssl3, libsqlite3-0, libdbus-1-3, libwayland-client0, libxkbcommon0, libvulkan1"` (matches the AUR PKGBUILD's runtime deps).
3. In `release.yml`, after the Linux nix-build step, add:
   ```bash
   nix shell nixpkgs#cargo-deb nixpkgs#cargo nixpkgs#rustc -c bash <<'SCRIPT'
   cargo deb -p modde-cli --no-build --no-strip --output release/
   cargo deb -p modde-ui  --no-build --no-strip --output release/
   SCRIPT
   ```
   Note: `--no-build` requires the binaries to already exist at `target/release/`. Since the workflow uses nix-built binaries, copy them to `target/release/` first or set `[package.metadata.deb] assets` paths to the nix-result symlinks.
4. Cross-build for `arm64`: either run `cargo deb --target aarch64-unknown-linux-gnu -p modde-cli` against the nix-cross-built binaries, or fabricate the `.deb` manually with `dpkg-deb -b` from a staged tree. Pick whichever requires less invention; prefer fabricating because nix already has the binaries.
5. Sign packages with `dpkg-sig` using the **repo GPG key** stored as Forgejo secret `APT_REPO_GPG_KEY` (separate key, lower-trust than maintainer key).
6. Publish APT repo:
   ```bash
   nix shell nixpkgs#reprepro -c bash <<'SCRIPT'
   reprepro -b dist/apt includedeb stable release/*.deb
   SCRIPT
   ```
   Then rsync `dist/apt/{dists,pool}` to the website-hosting target. If the website is served from Codeberg Pages (separate repo), push the apt tree into that repo on a new branch.
7. Add install docs section: `curl -fsSL https://modde.rs/apt/key.gpg | sudo gpg --dearmor -o /etc/apt/keyrings/modde.gpg && echo "deb [signed-by=/etc/apt/keyrings/modde.gpg] https://modde.rs/apt stable main" | sudo tee /etc/apt/sources.list.d/modde.list && sudo apt update && sudo apt install modde`.

## Acceptance criteria

- [ ] `cargo deb -p modde-cli` and `cargo deb -p modde-ui` produce valid `.deb` artifacts locally that pass `lintian --fail-on warning` (or `--info` for warnings allowed; document the chosen bar).
- [ ] Release workflow uploads `modde_<v>_amd64.deb`, `modde_<v>_arm64.deb`, `modde-ui_<v>_amd64.deb`, `modde-ui_<v>_arm64.deb` to the Codeberg release.
- [ ] `https://modde.rs/apt/dists/stable/Release` and `Release.gpg` resolve, and `apt update && apt install modde` works on a Debian 12 + Ubuntu 24.04 container.
- [ ] APT repo GPG key fingerprint is documented in `SECURITY.md` separately from the maintainer/minisign keys.
- [ ] `docs/site/content/docs/getting-started/installation.md` has a working "Debian/Ubuntu (apt)" section.

## Files likely touched

- `crates/modde-cli/Cargo.toml`, `crates/modde-ui/Cargo.toml`
- `.forgejo/workflows/release.yml`
- `scripts/publish-apt.sh` (new)
- `dist/apt/conf/distributions` (new)
- `docs/site/content/docs/getting-started/installation.md`
- `SECURITY.md`

## Pitfalls

- `cargo-deb`'s `depends` autodetection misses runtime deps loaded via `dlopen` (Vulkan loader). Hand-list them.
- Cross-arch `.deb`: `cargo deb --target aarch64-...` requires `dpkg-buildpackage` to know about the foreign arch; safer to use `dpkg-deb -b` on a staged tree built from the nix `modde-aarch64-linux` output.
- `libssl3` ABI: Debian 11 ships libssl1.1, Debian 12+ ships libssl3. Either vendor OpenSSL via `openssl-sys`'s `vendored` feature for the deb build, or skip Debian 11 explicitly. Recommend: vendored OpenSSL for deb build to avoid the ABI matrix.
- The APT `Release` file must be signed _with_ the same key whose fingerprint is in `key.gpg`; mismatched keys produce `NO_PUBKEY` errors that are confusing to debug.

## Reference

- `cargo-deb`: https://github.com/kornelski/cargo-deb
- reprepro: https://salsa.debian.org/debian/reprepro
- Phase 01 audit report — Linux gap list.
- Phase 02 — repo key vs maintainer key separation.
