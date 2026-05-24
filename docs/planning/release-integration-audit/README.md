# Release Integration Audit & Remediation

Wide-coverage plan to close gaps in rs-modde's release surface: signing, supply-chain artifacts, missing distribution channels (Linux/Windows/macOS), library publishing, pre-publish verification, and release ops.

Current pipeline (baseline, as of 2026-05-22):
- Forgejo `release.yml` triggered on `[0-9]*` tag push, runs on `atlas`.
- Cross-platform Nix builds: `modde`, `modde-aarch64-linux`, `modde-windows`, `modde-darwin-{x86_64,aarch64}`, `appimage-{cli,ui}`, `flatpak-manifest`.
- Codeberg release upload + SHA256SUMS (unsigned).
- Attic closure push to `https://attic.candee.baby/canix`.
- Homebrew tap bump via `rs-harbor brew bump` to `caniko/homebrew-modde`.
- COPR SRPM upload (Fedora) via `cargo xtask copr vendor`.
- `THIRD_PARTY_LICENSES.html` via `cargo-about`.

## Phase order & dependencies

| Phase | Layout | Sub-layers | Models | Blocks on |
|---|---|---|---|---|
| 01 audit | flat | — | 5.5 high | — |
| 02 signing | flat | — | 5.5 high | 01 |
| 03 sbom | flat | — | 5.5 medium | 01 |
| 04 linux channels | dir | 3 (deb, aur, flathub) | medium ×3 | 01, 02 |
| 05 windows channels | dir | 3 (winget, scoop, authenticode) | medium ×2, high ×1 | 01, 02 |
| 07 crates.io publish | flat | — | 5.5 medium | 01 |
| 08 pre-publish smoke matrix | flat | — | 5.5 high | 02–05, 07 |
| 09 release ops | flat | — | 5.5 medium | 08 |

macOS notarization is **explicitly out of scope** — unsigned macOS binaries remain installable (with one-time Gatekeeper override) and the project does not pursue Apple Developer ID. Phase 02's minisign / cosign signatures remain the only macOS verification path; this is documented in `SECURITY.md` rather than papered over with Authenticode-equivalent flow.

Parallelism after 01+02 land: 03, 04/*, 05/*, 07 can all run in parallel. 08 must serialize after them. 09 last.

Run each phase yourself in a fresh Codex session. Prompt `verify` when done to audit acceptance criteria.
