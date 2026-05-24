# Phase 04 — Missing Linux distribution channels

Three independent sub-layers, all dependent on phases 01 (gap list) and 02 (signing). They emit publish jobs / manifests / submission flows; each can be run in parallel by a separate Codex session.

| Sub-layer | Slug | Model | What it adds |
|---|---|---|---|
| 04a | `deb-and-apt.md` | 5.5 medium | `.deb` packages via `cargo-deb` (or `nfpm`) for amd64+arm64, plus a Codeberg-hosted APT repo (reprepro / aptly). |
| 04b | `aur-publish.md` | 5.5 medium | Real publish job for `dist/aur/PKGBUILD` (and a new `-bin` package), pushing to `aur.archlinux.org` via SSH key in Forgejo secret. |
| 04c | `flathub-submission.md` | 5.5 medium | Flathub submission flow: take the existing `flatpak-manifest` flake output, validate with `flatpak-builder --user --install-deps-from=flathub`, lint with `flatpak-builder-lint`, open the PR against `flathub/flathub`. |

Eligibility for sub-layer split (per `multi-phase-dispatch`):
- Each touches disjoint files (cargo-deb config / PKGBUILDs / Flathub PR) — yes, no overlap.
- Each could land independently — yes.
- Each has its own external credential (`APT_REPO_KEY`, `AUR_SSH_KEY`, `FLATHUB_TOKEN`) — yes.

Run any/all in parallel after phases 01 and 02 land.
