# Phase 03 — SBOM & supply-chain artifacts

> **Recommended Codex model: GPT 5.5 medium**
>
> Mostly mechanical: add `cargo-sbom` (or `cargo-cyclonedx`) to the release tool set, emit CycloneDX JSON + SPDX JSON alongside the license HTML, gate on `cargo-deny` advisories in the release path, and document the artifacts. Tier `medium` because the design choices (CycloneDX vs SPDX, what to do on a CVE advisory hit) are routine and well-precedented, but it's not pure typing — there's a judgement call on advisory-fail-vs-warn that doesn't merit `high`.

## Working tree

- `.forgejo/workflows/release.yml` — add SBOM generation steps.
- `.forgejo/workflows/ci.yml` — add `cargo deny check advisories` gate (advisory-fresh on every PR, not just release).
- `deny.toml` — review and tighten advisory + license + sources policy.
- New: `dist/sbom/` (gitignored runtime output) — or emit straight into `release/`.

## Goal

1. Every release ships **both** a CycloneDX JSON SBOM (`modde-<version>.cdx.json`) and an SPDX JSON SBOM (`modde-<version>.spdx.json`), generated from the workspace `Cargo.lock`.
2. The release job gates on `cargo deny check advisories` — a known-vulnerable dep fails the release (override requires explicit `[advisories.ignore]` with a justification comment).
3. SBOMs are signed/attested by phase 02's cosign step (treat them as release artifacts).
4. `THIRD_PARTY_LICENSES.html` continues to ship (human-readable companion).
5. CI runs `cargo deny check` on every PR (not just release) so advisories are caught before tag.

## Why

SBOMs are increasingly expected by downstream packagers (Flathub, Fedora) and required by some enterprise consumers. Generating only at release time is fine; gating on advisories only at release time is too late — a vulnerable dep should fail PR CI.

## Out of scope

- Reproducible builds (separate concern; phase 08 covers smoke-verification).
- Vendoring strategy changes — `cargo xtask copr vendor` continues unchanged.
- License policy changes — `deny.toml`'s license section reviewed but not rewritten.

## Plan

1. Add `cargo-sbom` (CycloneDX 1.5) generation step to release workflow:
   ```bash
   cargo install --locked cargo-sbom
   cargo sbom --output-format cyclone_dx_json_1_5 > release/modde-${VERSION}.cdx.json
   cargo sbom --output-format spdx_json_2_3   > release/modde-${VERSION}.spdx.json
   ```
   Wrap in `nix shell nixpkgs#cargo nixpkgs#rustc` like the existing cargo-about step. Cache `CARGO_INSTALL_ROOT` between sbom + license steps.
2. Add `cargo deny check advisories bans sources licenses` to the release workflow before build, exiting non-zero on failure. Review `deny.toml` to make sure `[advisories]` has `unmaintained = "warn"` (not deny) but `vulnerability = "deny"`.
3. Add the same `cargo deny check` step to `.forgejo/workflows/ci.yml` as a parallel job (cached cargo registry) so it runs on every PR.
4. Update the SHA256SUMS line to include the two SBOM files: `sha256sum *.cdx.json *.spdx.json` added to the existing globs.
5. Update the Codeberg release upload loop to include `release/*.cdx.json` and `release/*.spdx.json`.
6. Coordinate with phase 02: cosign-sign the SBOMs as `*.cdx.json.cosign.bundle` and attest them with `cosign attest --type cyclonedx`.
7. Document SBOM consumption in `SECURITY.md`: "Inspecting the SBOM" — `grype sbom:modde-<v>.cdx.json` example.

## Acceptance criteria

- [ ] Release workflow produces `release/modde-<version>.cdx.json` and `release/modde-<version>.spdx.json` and uploads both to the Codeberg release.
- [ ] Both SBOMs appear in `SHA256SUMS.txt`.
- [ ] Release workflow fails if `cargo deny check advisories` reports a `vulnerability = deny` hit; pass-through if only warnings.
- [ ] `.forgejo/workflows/ci.yml` runs `cargo deny check` on PRs (visible as a separate job).
- [ ] `cosign verify-attestation --type cyclonedx` succeeds against the uploaded `.cdx.json` when phase 02's signing is in place.
- [ ] `SECURITY.md` has an "Inspecting the SBOM" section with a working `grype`/`osv-scanner` example.

## Files likely touched

- `.forgejo/workflows/release.yml`
- `.forgejo/workflows/ci.yml`
- `deny.toml`
- `SECURITY.md`

## Pitfalls

- `cargo-sbom` emits the _workspace_ tree by default; if `crates/modde-ui` has feature-gated deps that flip on at release time, the SBOM may understate the dep set. Run `cargo sbom --output-format ... -p modde-cli` and `-p modde-ui` separately if the workspace-level output doesn't match what's actually linked into the release binary. Cross-check against `cargo tree -e normal -p modde --target x86_64-unknown-linux-gnu`.
- `cargo deny` with `unmaintained = deny` will fail on transitive deps that are perfectly fine; default to `warn` and only escalate per-crate.
- Don't double-install cargo-about and cargo-sbom in separate `cargo install` invocations without sharing `CARGO_INSTALL_ROOT` — they re-download the index each time.

## Reference

- cargo-sbom: https://github.com/psastras/sbom-rs
- CycloneDX 1.5: https://cyclonedx.org/specification/overview/
- Phase 01 audit report — "Supply-chain artifacts" section.
- Phase 02 — signing of SBOM artifacts.
