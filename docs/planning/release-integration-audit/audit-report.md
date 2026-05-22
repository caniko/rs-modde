# Release Integration Audit Report

Audit date: 2026-05-22

Scope read: `.forgejo/workflows/release.yml`, `.forgejo/workflows/ci.yml`, `.forgejo/workflows/pages.yml`, `flake.nix`, `dist/`, `scripts/`, `justfile`, `nix/release-supporting-tools.nix`, `crates/*/Cargo.toml`, `modde.spec`, `about-template.hbs`, `deny.toml`, `CHANGELOG.md`, `README.md`, `website/content/_index.md`, and `docs/site/content/docs/getting-started/installation.md`.

`simit.toml` is absent in this tree. That is not a blocker for this audit because the active simit-facing release configuration is in `flake.nix` under `simitConfig`. If a later phase requires a standalone `simit.toml`, the upstream producer is `simit init-*` or the maintainer's simit configuration migration; validate with `test -f simit.toml` plus `nix eval .#simitConfig`.

## Emitted Artifacts

The release workflow triggers on tags matching `[0-9]*`, validates SemVer-ish tag syntax, requires `nix eval --raw .#modde.version` to equal the tag, and then builds/publishes from the current tag checkout.

| Workflow path | Format | Target platform | Built from flake output | Distribution endpoint | Signing state |
|---|---|---|---|---|---|
| `release/THIRD_PARTY_LICENSES.html` | HTML license report | All | `cargo-about`, not a flake output | Codeberg release asset | Unsigned; covered by unsigned `SHA256SUMS.txt` only |
| `release/linux-x86_64/modde` | ELF executable staging file | Linux x86_64 | `.#modde` | Not uploaded directly; staging input for tarball/raw copy | Unsigned |
| `release/linux-x86_64/modde-ui` | ELF executable staging file | Linux x86_64 | `.#modde` | Not uploaded directly; staging input for tarball/raw copy | Unsigned |
| `release/modde-<version>-x86_64-linux.tar.gz` | tar.gz archive | Linux x86_64 | `.#modde` | Codeberg release asset; Homebrew tap `linux_intel` | Unsigned |
| `release/modde-<version>-x86_64-linux` | Raw ELF executable | Linux x86_64 CLI | `.#modde` | Codeberg release asset | Unsigned |
| `release/modde-ui-<version>-x86_64-linux` | Raw ELF executable | Linux x86_64 GUI | `.#modde` | Codeberg release asset | Unsigned |
| `release/linux-aarch64/modde` | ELF executable staging file | Linux aarch64 | `.#modde-aarch64-linux` | Not uploaded directly; staging input for tarball/raw copy | Unsigned |
| `release/linux-aarch64/modde-ui` | ELF executable staging file | Linux aarch64 | `.#modde-aarch64-linux` | Not uploaded directly; staging input for tarball/raw copy | Unsigned |
| `release/modde-<version>-aarch64-linux.tar.gz` | tar.gz archive | Linux aarch64 | `.#modde-aarch64-linux` | Codeberg release asset; Homebrew tap `linux_arm` | Unsigned |
| `release/modde-<version>-aarch64-linux` | Raw ELF executable | Linux aarch64 CLI | `.#modde-aarch64-linux` | Codeberg release asset | Unsigned |
| `release/modde-ui-<version>-aarch64-linux` | Raw ELF executable | Linux aarch64 GUI | `.#modde-aarch64-linux` | Codeberg release asset | Unsigned |
| `release/windows-x86_64/modde.exe` | PE executable staging file | Windows x86_64 CLI | `.#modde-windows` | Not uploaded directly; staging input for tarball | Unsigned; no Authenticode |
| `release/windows-x86_64/modde-ui.exe` | PE executable staging file | Windows x86_64 GUI | `.#modde-windows` | Not uploaded directly; staging input for tarball | Unsigned; no Authenticode |
| `release/modde-<version>-x86_64-windows.tar.gz` | tar.gz archive containing `.exe` files | Windows x86_64 | `.#modde-windows` | Codeberg release asset | Unsigned; contained EXEs unsigned |
| `release/darwin-x86_64/modde` | Mach-O executable staging file | macOS x86_64 CLI | `.#modde-darwin-x86_64` | Not uploaded directly; staging input for tarball | Ad-hoc signed only via `codesign --sign -`; no Developer ID; no notarization |
| `release/darwin-x86_64/modde-ui` | Mach-O executable staging file | macOS x86_64 GUI | `.#modde-darwin-x86_64` | Not uploaded directly; staging input for tarball | Ad-hoc signed only; no Developer ID; no notarization |
| `release/modde-<version>-x86_64-darwin.tar.gz` | tar.gz archive | macOS x86_64 | `.#modde-darwin-x86_64` | Codeberg release asset; Homebrew tap `darwin_intel` | Archive unsigned; contained binaries ad-hoc signed only; not notarized |
| `release/darwin-aarch64/modde` | Mach-O executable staging file | macOS aarch64 CLI | `.#modde-darwin-aarch64` | Not uploaded directly; staging input for tarball | Ad-hoc signed only; no Developer ID; no notarization |
| `release/darwin-aarch64/modde-ui` | Mach-O executable staging file | macOS aarch64 GUI | `.#modde-darwin-aarch64` | Not uploaded directly; staging input for tarball | Ad-hoc signed only; no Developer ID; no notarization |
| `release/modde-<version>-aarch64-darwin.tar.gz` | tar.gz archive | macOS aarch64 | `.#modde-darwin-aarch64` | Codeberg release asset; Homebrew tap `darwin_arm` | Archive unsigned; contained binaries ad-hoc signed only; not notarized |
| `release/modde-ui-<version>-x86_64.AppImage` | AppImage | Linux x86_64 GUI | `.#appimage-ui` | Codeberg release asset | Unsigned; no AppImageUpdate signature; no `.zsync` |
| `release/modde-<version>-x86_64.AppImage` | AppImage | Linux x86_64 CLI | `.#appimage-cli` | Codeberg release asset | Unsigned; no AppImageUpdate signature; no `.zsync` |
| `release/com.tartanoglu.modde.json` | Flatpak manifest JSON | Linux desktop | `.#flatpak-manifest` | Codeberg release asset only | Unsigned manifest; no Flathub submission/build artifact |
| `release/SHA256SUMS.txt` | SHA-256 checksum manifest | All release assets matched by workflow globs | Generated after artifact copy | Codeberg release asset | Unsigned |
| `srpms/*.src.rpm` | Fedora source RPM | Fedora/COPR source build | `cargo xtask copr vendor` + `rpmbuild -bs modde.spec` | COPR project `caniko/rs-modde` when COPR secrets exist | Not in `release/`; no signature recorded in workflow; no `rpmlint` gate |
| `attic-paths.txt` | Nix closure path list | Build cache metadata | `nix path-info -r` over flake build result links | Not uploaded; used to push closures to Attic `canix` cache | Not signed or published |

Flake outputs not published as release assets: `.#docs`, `.#website`, `.#site`, `.#homebrew-formula`, `.#rs-harbor`, checks, dev shells, `apps.deploy-pages`, and `homeManagerModules.modde`. `.#site` is published separately by the `pages` workflow on `trunk`, not by the tag release workflow. `.#homebrew-formula` exists but the tag workflow uses `nix run .#rs-harbor -- brew bump` directly rather than uploading the flake's formula output.

Release assets not directly derived from pinned flake outputs: `THIRD_PARTY_LICENSES.html`, `SHA256SUMS.txt`, and the COPR SRPM/vendor tarball path. They are still created from the tag checkout, but they are not content-addressed flake package outputs.

## Channels

| Channel | Claimed by repo | Actual workflow state | Status | Severity | Phase |
|---|---|---|---|---|---|
| Nix flake package | README and docs install page recommend `nix run` / `nix profile install`; website marks Nix flake available | `.#modde` exists and is the primary Linux build source | Shipped | Low | 08 |
| Home Manager module | README and docs install page document `inputs.modde.homeManagerModules.modde` | `flake.nix` exports `homeManagerModules.modde`; CI has HM eval checks | Shipped | Low | 08 |
| Codeberg release tarballs | README documents downloaded macOS/Windows/Linux artifacts; website says staged but no tagged release yet | Tag workflow creates Codeberg release and uploads Linux, Windows, macOS tarballs plus raw Linux binaries | Shipped but unsigned and under-documented | High | 02, 08, 09 |
| AppImage | Not advertised in README/install docs; workflow emits CLI and GUI AppImages | Codeberg assets only; no `.zsync`, no AppImageUpdate signature, no install docs | Shipped but incomplete | Medium | 02, 08, 09 |
| Homebrew tap | CHANGELOG `[Unreleased]` claims tap availability; README does not include install command | Workflow updates `caniko/homebrew-modde` when `homebrew_tap_token` exists | Shipped but degraded: unsigned, non-notarized macOS archives | High | 02, 06, 08 |
| Fedora COPR | Website reserves Fedora COPR card and says public project is not discoverable from this environment | Workflow builds SRPM and submits to COPR when `copr_*` secrets exist | Partially shipped; no public install docs, no `rpmlint`/rebuild smoke | Medium | 04, 08 |
| Arch AUR | Website says no `rs-modde-bin` package is published; `dist/aur/PKGBUILD` exists | No workflow publish step; checked-in `modde-git` PKGBUILD builds only `modde`, not `modde-ui` | Descriptor present, not shipped | Medium | 04 |
| Flatpak / Flathub | Website says manifest only, no installable remote; AppStream/desktop files checked in | Workflow uploads `com.tartanoglu.modde.json` only | Manifest shipped, channel missing | High | 04, 08 |
| Cargo install / crates.io for library crates | README says `cargo install modde-cli`; README says `cargo xtask release X.Y.Z` publishes workspace crates | Crate manifests have publish metadata but no release workflow step; `modde-xtask` is `publish = false`; binary crates are not marked `publish = false` | Advertised, not proven shipped | High | 07 |
| macOS direct download | README documents quarantine workaround and explicitly says no Developer ID/notarization | Workflow emits darwin tarballs and Homebrew uses them | Shipped but broken/degraded for normal Gatekeeper UX | Critical | 06 |
| Windows direct download | README documents unsigned EXE SmartScreen workaround | Workflow emits Windows tarball only | Shipped but unsigned and not packaged for native installers | High | 05, 08 |
| Website download cards | Website currently says most artifacts are not published yet | Workflow now publishes those artifacts on tags | Site is stale relative to workflow | Medium | 09 |
| `.deb` / APT | Not claimed | No `.deb`, no APT repository | Missing expected Linux channel | Medium | 04 |
| winget | Not claimed | No manifest or publish automation | Missing expected Windows channel | Medium | 05 |
| Scoop | Not claimed | No bucket manifest or publish automation | Missing expected Windows channel | Medium | 05 |
| Authenticode | README explicitly says Windows artifacts are unsigned | No `signtool`, `osslsigncode`, Azure Trusted Signing, or certificate flow | Missing signing channel | High | 05 |
| Flathub submission | Not claimed as live; website placeholder | No Flathub repository PR/update automation | Missing expected Linux desktop channel | High | 04 |
| MacPorts | Not claimed | No Portfile or publish workflow | Missing secondary macOS package manager | Low | 06 |
| Docker / OCI | Not claimed | No image build or registry publish | Missing optional headless CLI channel | Low | 04 or 09 |
| NixOS module input registration | README/docs document Home Manager input, not a NixOS module | No `nixosModules` export; `homeManagerModules.modde` present | HM shipped; NixOS module absent by design or unclaimed | Low | 09 |

## Supply-chain Artifacts

| Artifact / control | Present? | Evidence | Gap | Severity | Phase |
|---|---:|---|---|---|---|
| `SHA256SUMS.txt` | Yes | Release workflow runs `sha256sum ... > SHA256SUMS.txt` | Checksum file is unsigned, so a release-asset compromise can replace binaries and checksums together | High | 02 |
| signed checksums | No | No `minisign`, `gpg --detach-sign`, or equivalent | Need `SHA256SUMS.txt.minisig` or equivalent and public verification docs | High | 02 |
| GPG-signed tag enforcement | No | Validate step checks tag text and package version only | Workflow accepts unsigned tags matching `[0-9]*` | High | 02 |
| minisign/cosign artifact signatures | No | Search found no `minisign` or `cosign` usage | No detached artifact signatures or transparency-log verification | High | 02 |
| SLSA provenance | No | Search found no SLSA/provenance generation | No build provenance for binaries, AppImages, manifests, or SRPM | High | 02 |
| SBOM CycloneDX/SPDX | No | Search found no `cargo-sbom`, `cyclonedx`, `spdx`, or `syft` generation | `THIRD_PARTY_LICENSES.html` is a license report, not an SBOM | High | 03 |
| Human-readable third-party license report | Yes | `cargo about generate` emits `THIRD_PARTY_LICENSES.html` | Should remain, but must not be treated as SBOM coverage | Low | 03 |
| `cargo deny` policy | Partly | CI has a `cargo deny` job; `deny.toml` exists | Release workflow does not gate on `cargo deny`; advisory policy includes an ignored RUSTSEC with comment | Medium | 03 |
| Reproducible build inputs | Partly | Nix flake pins dependencies through `flake.lock`; release uses flake outputs | No independent rebuild check compares artifacts/hashes; some generated release assets are outside flake outputs | Medium | 08 |
| Attic closure push | Yes | Release workflow pushes closures to `https://attic.candee.baby/canix` | Attic is cache acceleration, not user-facing binary distribution; no published closure receipt or attestation | Low | 03, 08 |
| Attic closure pinning | Partly | `attic-paths.txt` is generated locally for push | Path list is not uploaded or signed; consumers cannot audit which closures backed the release | Low | 03 |

## Verification Gaps

| Check | Current state | Gap | Severity | Phase |
|---|---|---|---|---|
| Tag/version validation | `Validate tag` checks SemVer-ish tag and `.#modde.version` equality | Does not check CHANGELOG section, signed tag, or release branch policy | Medium | 02, 09 |
| Artifact smoke tests | No release-path extraction/execution before publish | Tarballs, raw binaries, AppImages, Windows EXEs, and macOS artifacts are pushed without runtime smoke | High | 08 |
| AppImage integrity/update validation | AppImages are copied from flake outputs | No `appimagetool --runtime-file` integrity check, no signature, no `.zsync`/AppImageUpdate channel | Medium | 08 |
| AppStream validation | `dist/com.tartanoglu.modde.metainfo.xml` exists | No `appstreamcli validate --strict`; screenshot URLs are placeholders | Medium | 08 |
| Desktop file validation | `dist/modde-ui.desktop` exists | No `desktop-file-validate` gate | Medium | 08 |
| Flatpak builder lint | Manifest is generated and uploaded | No `flatpak-builder`, `flatpak-builder-lint`, or Flathub review smoke | High | 04, 08 |
| SRPM lint/rebuild | SRPM is built and submitted to COPR | No `rpmlint`, no local/mock rebuild smoke before COPR upload | Medium | 08 |
| Windows verification | Windows tarball is built | No Wine smoke; no Authenticode verification; no installer manifest validation | High | 05, 08 |
| macOS verification | Darwin binaries are ad-hoc signed in Nix output | No Developer ID verification, notarization log, `spctl` check, or macOS-host smoke | Critical | 06, 08 |
| SBOM/vulnerability scan | CI runs `cargo deny`; no SBOM | No `grype`/`osv-scanner` gate against generated SBOMs | Medium | 03, 08 |
| Publish ordering | Build, checksum, Attic, Codeberg, Homebrew, COPR happen in one job | No smoke/signature gate between build and external publishing | High | 08 |

## Release-ops Gaps

| Area | Current state | Gap | Severity | Phase |
|---|---|---|---|---|
| Pre-release / RC channel | Tag regex accepts generic SemVer prerelease/build metadata; Codeberg release payload hardcodes `prerelease: false` | RC/beta tags would publish as stable and update Homebrew/COPR stable | High | 09 |
| Hotfix branch policy | Not documented in inspected docs | No branch-from-tag/cherry-pick/tag policy; rollback path unclear | Medium | 09 |
| In-app update check | CLI has update-related tests/commands, but no release-ops policy documents a Codeberg latest-release check | No audited opt-out/update-notification behavior tied to releases | Low | 09 |
| Announcement automation | None in release workflow | No Mastodon/Matrix/site announcement automation | Low | 09 |
| CHANGELOG-vs-tag enforcement | Codeberg release body uses whole `CHANGELOG.md` | Workflow does not require `## [<version>] - <date>` section or extract that section | Medium | 09 |
| Rollback/yank drill | Not documented in inspected release docs | No per-channel bad-release withdrawal procedure | Medium | 09 |
| Website/download synchronization | Website install cards intentionally say many artifacts are not downloadable | Workflow now publishes those assets, so site can lag release reality | Medium | 09 |
| External channel skip semantics | Homebrew/COPR skip when secrets missing; Codeberg release fails when token missing | No single release summary that records which optional channels actually published | Medium | 09 |

## Required Secrets

Current release workflow credentials:

| Secret / credential | Required by | Current behavior when missing | Notes |
|---|---|---|---|
| `secrets.codeberg_token` -> `CODEBERG_TOKEN` | Codeberg release create/update and asset upload | Fails release (`test -n "$CODEBERG_TOKEN"`) | Also used by `pages.yml` for Codeberg Pages, outside tag release |
| Runner file `$ATTIC_TOKENS_DIR/rs-modde` | Attic closure push to `https://attic.candee.baby/canix` | Fails release (`test -r`) | Not a Forgejo secret expression, but still a required credential on the atlas runner |
| `secrets.homebrew_tap_token` -> `HOMEBREW_TAP_TOKEN` | Push to `caniko/homebrew-modde` | Skips Homebrew tap update | Token comment says Codeberg access token scoped to tap with `write:repository` |
| `secrets.copr_login` -> `COPR_LOGIN` | COPR CLI config | Skips COPR upload if any COPR credential missing | Fedora COPR |
| `secrets.copr_username` -> `COPR_USERNAME` | COPR CLI config | Skips COPR upload if any COPR credential missing | Fedora COPR |
| `secrets.copr_token` -> `COPR_TOKEN` | COPR CLI config | Skips COPR upload if any COPR credential missing | Fedora COPR |

Additional credentials phases 02-09 will need:

| Secret / credential | Channel/control | Phase |
|---|---|---|
| `MINISIGN_SECRET_KEY` | Sign `SHA256SUMS.txt` | 02 |
| `MINISIGN_PASSWORD` | Unlock minisign secret key | 02 |
| `COSIGN_PRIVATE_KEY` | Optional fallback if Forgejo keyless OIDC is unavailable | 02 |
| `COSIGN_PASSWORD` | Optional cosign key password | 02 |
| Forgejo job OIDC / `id-token: write` equivalent | Keyless cosign and SLSA provenance, if supported | 02 |
| Maintainer GPG public keys committed to repo | `git verify-tag` trust root | 02 |
| `APT_REPO_GPG_KEY` | Sign Debian/APT repository metadata and packages | 04 |
| `AUR_SSH_KEY` | Push `modde`, `modde-git`, and `modde-bin` PKGBUILDs to AUR | 04 |
| `FLATHUB_TOKEN` | Open/update Flathub app repository PR or publish branch | 04 |
| `WINGET_PAT` | Push winget manifests / PR to `microsoft/winget-pkgs` via bot fork | 05 |
| `SCOOP_BUCKET_TOKEN` | Push `caniko/scoop-modde` bucket updates | 05 |
| `AZURE_TENANT_ID` | Azure Trusted Signing option for Authenticode | 05 |
| `AZURE_CLIENT_ID` | Azure Trusted Signing option for Authenticode | 05 |
| `AZURE_CLIENT_SECRET` | Azure Trusted Signing fallback without OIDC | 05 |
| `AZURE_TRUSTED_SIGNING_ENDPOINT` | Azure Trusted Signing endpoint | 05 |
| `AZURE_TRUSTED_SIGNING_ACCOUNT` | Azure Trusted Signing account | 05 |
| `AZURE_TRUSTED_SIGNING_PROFILE` | Azure Trusted Signing certificate profile | 05 |
| `WINDOWS_SIGNING_PFX` | Alternative PKCS#12 Authenticode cert | 05 |
| `WINDOWS_SIGNING_PASS` | Alternative PKCS#12 passphrase | 05 |
| `MACOS_DEVELOPER_ID_P12_BASE64` | Developer ID Application certificate | 06 |
| `MACOS_DEVELOPER_ID_PASSWORD` | Developer ID cert passphrase | 06 |
| `MACOS_NOTARY_KEY_P8_BASE64` | App Store Connect notary API key | 06 |
| `MACOS_NOTARY_KEY_ID` | App Store Connect notary key ID | 06 |
| `MACOS_NOTARY_ISSUER_ID` | App Store Connect notary issuer ID | 06 |
| `CARGO_REGISTRY_TOKEN` | crates.io publish or post-flight verification needing auth | 07 |
| `MASTODON_TOKEN` | Release announcement automation | 09 |
| `MATRIX_TOKEN` | Release announcement automation | 09 |
| `MATRIX_ROOM` | Release announcement target room | 09 |

## Severity-ranked Gap List

| Severity | Gap | Evidence | Phase |
|---|---|---|---|
| Critical | macOS notarization is absent; macOS artifacts are not Developer ID signed or notarized | `flake.nix` ad-hoc signs with `codesign --sign -`; README documents Gatekeeper quarantine workaround; no `notarytool`/`rcodesign` release step | 06 |
| High | Unsigned checksums and unsigned release assets | `SHA256SUMS.txt` exists but no minisign/GPG/cosign signatures | 02 |
| High | No GPG-signed tag enforcement | `Validate tag` only checks tag syntax and `.#modde.version` | 02 |
| High | No SLSA provenance | No SLSA/cosign attestation generation | 02 |
| High | No SBOM despite license report | `cargo-about` emits HTML licenses; no CycloneDX/SPDX generator | 03 |
| High | Windows EXEs are unsigned | README says SmartScreen workaround; workflow has no Authenticode signing | 05 |
| High | Release publishes before smoke verification | Workflow pushes Attic, Codeberg, Homebrew, and COPR immediately after build/checksum | 08 |
| High | Flatpak channel stops at manifest upload | `.#flatpak-manifest` uploaded as JSON; no Flathub submission/build/lint | 04 |
| High | crates.io install claim is not backed by audited publish flow | README advertises `cargo install modde-cli`; release workflow has no crates.io publish or verification | 07 |
| High | RC/pre-release tags would publish as stable | Release payload hardcodes `prerelease: false`; Homebrew/COPR steps do not branch on prerelease | 09 |
| Medium | AppImages have no update/signature channel | AppImages are uploaded but no `.zsync` or AppImageUpdate signature | 02, 08 |
| Medium | AUR descriptor is checked in but unpublished | `dist/aur/PKGBUILD` exists; no release publish step | 04 |
| Medium | `.deb`/APT channel missing | No Debian package output or signed APT repository | 04 |
| Medium | winget channel missing | No winget manifests or PR automation | 05 |
| Medium | Scoop channel missing | No Scoop bucket manifest or publish automation | 05 |
| Medium | COPR lacks release-path lint/rebuild verification | SRPM is submitted without `rpmlint` or mock rebuild smoke | 08 |
| Medium | Website download claims are stale relative to workflow | Website says several artifacts are not published even though release workflow uploads them | 09 |
| Medium | CHANGELOG-vs-tag is not enforced | Release body uses full `CHANGELOG.md`; no matching section check | 09 |
| Medium | No hotfix/rollback/yank policy | No inspected release docs describe per-channel withdrawal | 09 |
| Medium | No AppStream/desktop-file validation | Metadata files exist, but no `appstreamcli` or `desktop-file-validate` gate | 08 |
| Medium | Release-generated non-flake assets are not attested | License report, checksums, and SRPM/vendor path are generated outside flake outputs | 03, 08 |
| Low | MacPorts missing | No Portfile or MacPorts workflow; Homebrew is the only package-manager path | 06 |
| Low | Docker/OCI image missing | No headless CLI container image output | 04 or 09 |
| Low | NixOS module export absent | Home Manager module is shipped, but no `nixosModules` export | 09 |
| Low | Attic closure receipt is not published | Cache push is useful acceleration but not auditable by users | 03 |
| Low | Announcement automation missing | No Mastodon/Matrix/webhook release announcement step | 09 |
