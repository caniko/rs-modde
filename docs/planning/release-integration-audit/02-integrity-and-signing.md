# Phase 02 — Artifact integrity, signing & SLSA provenance

> **Recommended Codex model: GPT 5.5 high**
>
> Security-critical: getting key management wrong here (wrong key in secret, public key not pinned in docs, signing the wrong file, double-signing already-signed binaries) means either silently-unverifiable releases or a key compromise blast radius across every channel. Phases 04–08 all consume the signing artifacts produced here, so a flaw cascades. Worth `high` for the design judgement; mechanical edits within it are small.

## Working tree
- `.forgejo/workflows/release.yml` — add signing steps.
- `docs/site/content/docs/getting-started/installation.md` — document verification.
- New: `SECURITY.md` section on signature verification (or extend existing).
- New: `keys/minisign.pub` (or wherever the public key lives so users can pin it).

## Goal
Every release artifact has a verifiable signature and a provenance attestation:
1. `SHA256SUMS.txt` is signed with **minisign** (offline key) → `SHA256SUMS.txt.minisig`.
2. Each tarball/AppImage additionally has a **cosign keyless** signature + SLSA v1 provenance attestation pushed to a transparency log (Rekor), enabling supply-chain verification without managing PKI.
3. The git tag itself is **GPG-signed** by the maintainer before CI runs.
4. The public minisign key ships in the repo (`keys/minisign.pub`) and is documented in `SECURITY.md` with a copy-pasteable `minisign -Vm SHA256SUMS.txt -p keys/minisign.pub` command.

## Why
Today `SHA256SUMS.txt` is published but unsigned — an attacker who compromises the Codeberg release endpoint can swap binaries *and* checksums. Downstream channels (Homebrew, AUR, COPR, Flathub) pull source tarballs by URL+sha256; if those checksums aren't independently anchored to a maintainer-controlled key, every channel inherits the compromise. SLSA provenance also gives Flathub / Homebrew reviewers something machine-verifiable.

## Out of scope
- Authenticode (Windows code signing) → phase 05c.
- macOS codesign / notarization — out of scope project-wide. macOS users verify via minisign/cosign on the tarball, then override Gatekeeper once on first launch. Document this trade-off in `SECURITY.md`.
- GPG-signing release commits (just tags here).
- Rotating the existing Attic / Codeberg / COPR tokens.

## Plan
1. Generate a long-lived minisign keypair offline. Store the secret key encrypted; add it as Forgejo secret `MINISIGN_SECRET_KEY` and the password as `MINISIGN_PASSWORD`. Commit `keys/minisign.pub` to the repo.
2. Add a workflow step after "Build release artifacts" that:
   - Runs `nix shell nixpkgs#minisign -c minisign -Sm release/SHA256SUMS.txt` using the secret from the Forgejo secret, emitting `release/SHA256SUMS.txt.minisig`.
3. Add a workflow step using `cosign sign-blob --yes --bundle <file>.cosign.bundle` (keyless OIDC against Sigstore's public-good Fulcio) for every tarball, AppImage, and the SRPM. Use `nix shell nixpkgs#cosign`. Requires `id-token: write` permission on the job; if Forgejo Actions OIDC isn't wired to Sigstore, fall back to **cosign with a key in `COSIGN_PRIVATE_KEY` secret** and document the deviation in `audit-report.md`. (Check OIDC availability before committing to keyless.)
4. Generate SLSA v1 provenance with `slsa-github-generator`-equivalent. Since this is Forgejo not GitHub, use `nix shell nixpkgs#slsa-verifier` only for consumers; produce provenance ourselves with `cosign attest --predicate <provenance.json> --type slsaprovenance` per artifact. Predicate JSON is a small template: `{builder.id, buildType, invocation.configSource: <repo>@<sha>, materials: [<flake.lock>]}`.
5. Upload `*.minisig`, `*.cosign.bundle`, and `*.intoto.jsonl` to the Codeberg release alongside existing assets.
6. Update the "Validate tag" step to also enforce `git verify-tag "$CODEBERG_REF_NAME"` against a pinned set of maintainer GPG keys checked into `keys/maintainers.gpg`.
7. Write `SECURITY.md` ("Verifying releases") with three blocks: minisign quick path, cosign verify-blob, and SLSA verifier.
8. Update `docs/site/content/docs/getting-started/installation.md` "Verify your download" subsection.

## Risk profile
- **Failure mode 1: signing key leak.** Minisign secret + password are in Forgejo secrets; ensure secrets are scoped to the `release` job, not workflow-level. Document rotation procedure in SECURITY.md.
- **Failure mode 2: keyless cosign relies on Forgejo→Sigstore OIDC**, which may not be wired. Verify before depending on it; otherwise use key-based cosign and document.
- **Failure mode 3: SLSA predicate that lies.** Materials must include `flake.lock` SHA and the workflow file SHA at build time, not just the commit ref. The `caniko/canix` substituter is a material too.

## Acceptance criteria
- [ ] `keys/minisign.pub` committed and referenced from `SECURITY.md`.
- [ ] Forgejo secrets `MINISIGN_SECRET_KEY`, `MINISIGN_PASSWORD`, and (if used) `COSIGN_PRIVATE_KEY` + `COSIGN_PASSWORD` documented as required in `audit-report.md` and in a `## Required secrets` block inside `.forgejo/workflows/release.yml` (comment).
- [ ] On a dry-run tag push, `release/SHA256SUMS.txt.minisig` exists and verifies against `keys/minisign.pub` via `minisign -Vm release/SHA256SUMS.txt -p keys/minisign.pub`.
- [ ] Every tarball + AppImage in the Codeberg release has a sibling `*.cosign.bundle` and `*.intoto.jsonl` SLSA provenance asset.
- [ ] `git verify-tag` is enforced in the workflow (job fails on unsigned tag).
- [ ] `SECURITY.md` has a "Verifying releases" section with working commands; tested locally against a real artifact.
- [ ] `SECURITY.md` includes a "macOS Gatekeeper" subsection explaining that the project does not notarize, with the one-time `xattr -d com.apple.quarantine` (or right-click → Open) workaround and a pointer to minisign verification as the integrity check.

## Files likely touched
- `.forgejo/workflows/release.yml` (steps added).
- `SECURITY.md` (new section).
- `keys/minisign.pub` (new file).
- `keys/maintainers.gpg` (new file, exported GPG public keys).
- `docs/site/content/docs/getting-started/installation.md`.

## Pitfalls
- Forgejo's secret-injection on `atlas` runner: confirm `${{ secrets.MINISIGN_SECRET_KEY }}` expansion does not appear in logs (Forgejo masks secrets, but multi-line keys can leak via debug `set -x`). Pipe via stdin or write to a file with `umask 077`.
- Cosign keyless requires OIDC; `cachix/install-nix-action` does not request `id-token: write` — must be set at job-level. If the Forgejo Actions implementation doesn't support OIDC tokens, the keyless path silently degrades to anonymous; check by inspecting `cosign sign-blob` output for a Rekor entry URL.
- SLSA v1 predicates require a `builder.id` URI; pick a stable one (e.g. `https://git.tartanoglu.com/caniko/rs-modde/.forgejo/workflows/release.yml@refs/tags/$TAG`) and document it.

## Reference
- Minisign: https://jedisct1.github.io/minisign/
- Cosign blob signing: https://docs.sigstore.dev/cosign/signing/signing_with_blobs/
- SLSA v1.0 spec: https://slsa.dev/spec/v1.0/
- Phase 01 audit report — required-secrets section.
