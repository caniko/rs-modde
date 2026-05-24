# Phase 01 — Release-surface audit & gap inventory

> **Recommended Codex model: GPT 5.5 high**
>
> Synthesis-heavy planner step: read every release-touching file (workflows, flake, simit.toml, dist/, scripts/), classify every emitted artifact, cross-check against the distribution channels the project *should* be on for a cross-platform mod manager, and produce a prioritized severity-tagged gap list that downstream phases consume. Cheaper tiers will skim and miss the cross-cutting issues (e.g. SBOM ≠ THIRD_PARTY_LICENSES.html; AppImage shipped but unsigned and no zsync; AUR PKGBUILD checked in but unpublished). Worth the spend because every later phase reads this report.

## Working tree
- New file: `docs/planning/release-integration-audit/audit-report.md` (the deliverable).
- Read-only sweep of: `.forgejo/workflows/release.yml`, `flake.nix`, `simit.toml`, `dist/`, `scripts/`, `justfile`, `nix/release-supporting-tools.nix`, `crates/*/Cargo.toml`, `modde.spec`, `about-template.hbs`, `deny.toml`, `CHANGELOG.md`, `website/content/_index.md` (download links).

## Goal
A single committed report that enumerates:
1. Every artifact emitted by the current release workflow, with file path, format, target platform, distribution endpoint, and signing state.
2. Every distribution channel the project *claims* to support (README, website, install docs) vs. what the workflow actually publishes — list mismatches.
3. Every missing channel a cross-platform mod manager would reasonably want (winget, scoop, .deb/APT, Flathub, MacPorts, crates.io for libraries, Docker for headless CLI, NixOS module input registration). **Note**: macOS notarization is out of scope project-wide — record Gatekeeper UX as a known trade-off, not a gap to remediate.
4. Supply-chain artifacts present/absent: SHA256SUMS, GPG-signed tag, minisign/cosign, SLSA provenance, SBOM (CycloneDX/SPDX), reproducibility, attic closure pinning.
5. Pre-publish verification: smoke tests, AppStream/desktop-file validation, AppImage `appimagetool --runtime-file` integrity, Flatpak builder lint, rpmlint on SRPM.
6. Release-ops gaps: pre-release/RC channel, hotfix branch policy, in-app update check, announcement automation, CHANGELOG-vs-tag enforcement.

Each finding tagged severity `critical` / `high` / `medium` / `low` and mapped to the phase that addresses it (02–09).

## Why
Every downstream phase references this report's findings. Without it, the remediation phases would each re-discover the same context and risk diverging on what "the current state" is. The report is also the input for the verify pass — phase 08 and the post-verify check both compare repo state against the inventory here.

## Out of scope
- Any code changes. This phase is read-only and writes one markdown file.
- Designing the fixes. Each later phase owns its own design.
- Re-litigating simit's release flow itself — only document what it does, do not propose changes to simit.

## Plan
1. Parse `.forgejo/workflows/release.yml` step-by-step and list every artifact name produced, every external system pushed to, and every secret consumed.
2. Cross-reference `flake.nix` package outputs against artifacts uploaded to Codeberg releases — flag any flake output not in the release and any release asset not built from a pinned flake output.
3. Read `website/content/_index.md`, `README.md`, `docs/site/content/docs/getting-started/installation.md` for advertised install methods; mark each as "shipped", "shipped-but-broken", or "advertised-not-shipped".
4. Inspect `dist/aur/PKGBUILD`, `dist/com.tartanoglu.modde.metainfo.xml`, `dist/modde-ui.desktop`, `modde.spec` — for each, identify the channel it targets and whether the workflow publishes it.
5. Check signing state: search workflow for `gpg`, `cosign`, `minisign`, `signtool`, `codesign`, `notarytool` — record absence.
6. Check SBOM state: search for `cargo-sbom`, `cyclonedx`, `spdx`, `syft` — record absence; note that `cargo-about` produces a license report, not an SBOM.
7. List every Forgejo secret referenced (`codeberg_token`, `homebrew_tap_token`, `copr_*`) and any secret a future phase will need (`MACOS_NOTARY_*`, `WINDOWS_CERT_*`, `MINISIGN_KEY`, `CRATES_IO_API_TOKEN`, `WINGET_PAT`, `FLATHUB_TOKEN`).
8. Write `audit-report.md` with sections: **Emitted artifacts**, **Channels (shipped/broken/missing)**, **Supply-chain artifacts**, **Verification gaps**, **Release-ops gaps**, **Required secrets**, **Severity-ranked gap list**.

## Acceptance criteria
- [ ] `docs/planning/release-integration-audit/audit-report.md` exists and is committed.
- [ ] Report enumerates every artifact in `release/` produced by the current workflow (one row per artifact: name, format, platform, signed?).
- [ ] Report names at least: winget, scoop, Authenticode, .deb/APT, Flathub submission, MacPorts, crates.io (for library crates), Docker/OCI, SBOM, SLSA provenance, signed checksums, signed tag — each marked present/absent with severity. macOS notarization is explicitly listed as out-of-scope with a one-paragraph rationale (users accept Gatekeeper override; project does not maintain an Apple Developer account).
- [ ] Report's "Severity-ranked gap list" maps every gap to a phase number 02–05, 07–09.
- [ ] Every Forgejo secret consumed by the *current* workflow is listed, and every additional secret that phases 02–09 will need is listed with the channel that requires it.

## Files likely touched
- `docs/planning/release-integration-audit/audit-report.md` (created).

## Pitfalls
- Confusing `cargo-about`'s license report with an SBOM. They are different artifacts; both should ship.
- Treating Attic closure push as "binary distribution". Attic is build-cache acceleration, not user-facing distribution.
- Missing that `appimage-{cli,ui}` are emitted but lack zsync update channel and AppImageUpdate signature — a partial channel, not a complete one.
- Listing the homebrew tap as "macOS distribution" — it ships unsigned, non-notarized binaries; on modern macOS (gatekeeper) this is a degraded UX, not a complete channel.

## Reference
- `.forgejo/workflows/release.yml` — current pipeline.
- `flake.nix` packages set — source of all binary artifacts.
- `simit.toml` — only configures Homebrew section currently.
- `dist/aur/PKGBUILD`, `modde.spec`, `dist/com.tartanoglu.modde.metainfo.xml` — checked-in distribution descriptors.
