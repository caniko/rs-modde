# Phase 04b — AUR publish (source + `-bin`)

> **Recommended Codex model: GPT 5.5 medium**
>
> The PKGBUILD already exists in `dist/aur/`; this phase wires version bumping + SSH push to `aur@aur.archlinux.org` and adds a `modde-bin` companion that consumes the prebuilt release tarball. Mechanical with one judgement call (`-bin` package depends on `glibc` ABI of the build host — pick a sensible floor and document).

## Working tree
- `dist/aur/PKGBUILD` — existing source PKGBUILD; refresh.
- New: `dist/aur-bin/PKGBUILD` — `-bin` flavour pointing at release tarball.
- `.forgejo/workflows/release.yml` — new "Publish AUR" step.

## Goal
1. `modde-git` (existing, source-from-git) stays as-is for tracking trunk.
2. `modde` (new, source PKGBUILD) — tagged releases, builds from the release source tarball.
3. `modde-bin` (new) — pulls the prebuilt `modde-<v>-x86_64-linux.tar.gz` from the Codeberg release.
4. All three packages are pushed to AUR on every tag.

## Why
The PKGBUILD has lived in `dist/aur/` since before the project had releases — it's never been published. Arch users currently can't `yay -S modde`. Adding `-bin` reduces install time from ~15min (compile) to ~10s.

## Out of scope
- Submitting to official `[extra]` Arch repository — separate process, defer.
- ALPM hooks for game-detection — out of scope; this is just packaging.

## Plan
1. Refresh `dist/aur/PKGBUILD` so `modde-git` is correctly named (it currently builds only the CLI; either add `modde-ui` or rename to `modde-cli-git`).
2. Create `dist/aur/modde/PKGBUILD` (source release):
   - `pkgname=modde`, `pkgver=<TAG>`, `source=("https://codeberg.org/caniko/rs-modde/archive/${pkgver}.tar.gz")`.
   - `sha256sums` populated by the release job at publish time.
   - Builds both `modde` and `modde-ui` binaries.
3. Create `dist/aur/modde-bin/PKGBUILD`:
   - `pkgname=modde-bin`, `source=("https://codeberg.org/caniko/rs-modde/releases/download/${pkgver}/modde-${pkgver}-x86_64-linux.tar.gz")`.
   - `package()` just `install -Dm755` the binaries.
   - Depends on `glibc>=2.38` (or whatever the build container ships); document floor.
4. Generate `.SRCINFO` for each via `makepkg --printsrcinfo > .SRCINFO`.
5. Add Forgejo secret `AUR_SSH_KEY` (Ed25519 private key whose pub key is registered on aur.archlinux.org).
6. New release-workflow step:
   ```bash
   mkdir -p ~/.ssh
   echo "$AUR_SSH_KEY" > ~/.ssh/aur && chmod 600 ~/.ssh/aur
   ssh-keyscan aur.archlinux.org >> ~/.ssh/known_hosts

   for pkg in modde modde-bin; do
     git clone "ssh://aur@aur.archlinux.org/${pkg}.git" "aur-$pkg" || true
     cp dist/aur/$pkg/PKGBUILD "aur-$pkg/"
     (cd "aur-$pkg" && makepkg --printsrcinfo > .SRCINFO && \
       git add PKGBUILD .SRCINFO && \
       git -c user.email=ci@modde.tartanoglu.com -c user.name='modde release bot' \
         commit -m "${VERSION}" && \
       GIT_SSH_COMMAND='ssh -i ~/.ssh/aur' git push)
   done
   ```
7. Populate `sha256sums` from `release/SHA256SUMS.txt` before committing.
8. Document AUR install in `docs/site/content/docs/getting-started/installation.md` and link to AUR pages.

## Acceptance criteria
- [ ] `dist/aur/{modde,modde-bin,modde-git}/PKGBUILD` exist; each `makepkg --printsrcinfo` produces a valid `.SRCINFO`.
- [ ] `namcap dist/aur/modde/PKGBUILD` and `namcap dist/aur/modde-bin/PKGBUILD` produce no errors (warnings acceptable, documented).
- [ ] Release workflow pushes to `ssh://aur@aur.archlinux.org/modde.git` and `.../modde-bin.git` on every tag and does not regress `modde-git`.
- [ ] On a fresh Arch container, `yay -S modde-bin` installs and `modde --version` matches the tag.
- [ ] AUR maintainer-comment field references the maintainer GPG key for cross-channel identity.

## Files likely touched
- `dist/aur/modde/PKGBUILD` (new)
- `dist/aur/modde-bin/PKGBUILD` (new)
- `dist/aur/modde-git/PKGBUILD` (renamed from `dist/aur/PKGBUILD`, refreshed)
- `.forgejo/workflows/release.yml`
- `docs/site/content/docs/getting-started/installation.md`

## Pitfalls
- AUR rejects pushes that change `pkgver` without a `pkgrel` reset to 1 — handle explicitly.
- `modde-bin` linked against a too-new `glibc` will fail to install on older Arch installs. The atlas runner's `cachix/install-nix-action` build host may have a newer glibc than typical Arch installs; pin the build environment's glibc floor or document it.
- Don't commit the AUR SSH key to `keys/` (different trust domain from minisign/maintainer keys).

## Reference
- AUR submission guidelines: https://wiki.archlinux.org/title/AUR_submission_guidelines
- Phase 02 — `sha256sums` should be cross-checked against the signed `SHA256SUMS.txt`.
