# Phase 03 — Rewrite `scripts/publish-apt.sh` for SSH push

> **Recommended Codex model: GPT 5.5 medium**
>
> Bash plumbing for an SSH agent + `known_hosts` pin + force-pushing a
> regenerated tree to a Pages branch. The pieces are individually
> routine, but the SSH agent lifecycle (start, add key, trap-cleanup on
> early exit), the `known_hosts` pin (so an MITM cannot quietly accept a
> new host key on first run), and the force-push semantics need a model
> that won't paper over edge cases. A low-tier rewrite would skip the
> trap or use `StrictHostKeyChecking=no`.

## Working tree

`/data/nvme0/can/Projects/rs-modde`. This phase modifies `scripts/publish-apt.sh` only. It must not edit `.forgejo/workflows/release.yml` — that is Phase 04's surface — because doing both in one phase creates a window where the script expects new env vars that the workflow has not yet set, blocking re-test from `main` if the user lands the two diffs out of order.

## Goal

`scripts/publish-apt.sh` builds the same signed reprepro tree it builds today, but pushes via SSH to `ssh://git@codeberg.org/caniko/rs-modde-apt.git`. Behavior parity:

- Same input (`release/*.deb`).
- Same gating semantics (skip + warn when required env vars are unset, so non-bootstrap tags stay green).
- Same git author identity (`modde release bot <release-bot@modde.tartanoglu.com>`).
- Same idempotency: re-running for the same `VERSION` against an already-published tree is a no-op.

What changes:

- Input env vars: drop `APT_REPO_GPG_KEY` plain naming? **No** — keep the local env-var spelling uppercase; only the workflow's `secrets.X` reference changes. This script consumes:
  - `APT_REPO_GPG_KEY` (unchanged): ascii-armored secret key. Required.
  - `APT_REPO_GPG_KEY_ID` (unchanged): fingerprint. Required.
  - `APT_REPO_GPG_PASSPHRASE` (unchanged): optional.
  - **New**: `APT_REPO_SSH_KEY` (replaces `APT_REPO_SSH_KEY`): ascii ed25519 private key body. Required.
- Push transport: `git push` over SSH to the remote above.
- Branch target: `pages` (Phase 01 set this as the default-and-served branch on `caniko/rs-modde-apt`). The push is a `--force-with-lease` to keep the served tree exactly equal to the freshly built tree.
- `known_hosts` pinning for `codeberg.org` (so the SSH connection cannot be MITM'd; the runner has no maintainer in the loop to accept a TOFU prompt).

## Why this matters now

The workflow's `Publish APT repository` step skips silently today because every apt-related secret is unset. Once Phase 02 lands the SSH key, Phase 04 will switch the workflow to feed `APT_REPO_SSH_KEY` into the script's env. If the script still expects `APT_REPO_SSH_KEY`, the first real tag push will fail in a noisy way (`token undefined`, then HTTP 401 via the old HTTPS URL). Better to ship the script change first and let it skip cleanly while Phase 04 catches up.

## Out of scope

- Workflow changes. Phase 04.
- Docs changes. Phase 04.
- Adding new artifact types (e.g. `arm64` debs not already in `release/`). Out of scope project-wide for this plan; the reprepro `Architectures` line in `dist/apt/conf/distributions` already covers `amd64 arm64` and the script globs `release/*.deb` indiscriminately.
- Splitting `pages` into multiple repos or branches per release channel (stable vs testing).
- Mirroring to a CDN.

## Plan

1. Rewrite `scripts/publish-apt.sh` end-to-end. Required structure (verbatim, modulo whitespace):
   ```sh
   #!/usr/bin/env bash
   # ... (header comment block, see step 2)
   set -euo pipefail

   VERSION="${VERSION:?VERSION must be set to the release tag}"
   APT_REPO_REMOTE="${APT_REPO_REMOTE:-ssh://git@codeberg.org/caniko/rs-modde-apt.git}"
   APT_REPO_BRANCH="${APT_REPO_BRANCH:-pages}"

   # --- soft gates: any missing piece is a clean skip, not a failure ---
   if [ -z "${APT_REPO_GPG_KEY:-}" ] || [ -z "${APT_REPO_GPG_KEY_ID:-}" ]; then
     echo "::warning::APT_REPO_GPG_KEY / APT_REPO_GPG_KEY_ID unset; skipping apt publish."
     exit 0
   fi
   if [ -z "${APT_REPO_SSH_KEY:-}" ]; then
     echo "::warning::APT_REPO_SSH_KEY unset; skipping apt publish."
     exit 0
   fi
   debs=(release/*.deb)
   if [ ! -e "${debs[0]}" ]; then
     echo "::warning::No .deb files in release/; nothing to publish to apt."
     exit 0
   fi

   work="$(mktemp -d)"
   trap 'rm -rf "$work"; [ -n "${SSH_AGENT_PID:-}" ] && ssh-agent -k >/dev/null 2>&1 || true' EXIT
   chmod 700 "$work"

   # --- gpg setup, exactly as the previous version ---
   GNUPGHOME="$work/gpg"; mkdir -p "$GNUPGHOME"; chmod 700 "$GNUPGHOME"
   export GNUPGHOME
   printf '%s' "$APT_REPO_GPG_KEY" | gpg --batch --import 2>&1 | sed 's/^/gpg: /'
   echo "${APT_REPO_GPG_KEY_ID}:6:" | gpg --batch --import-ownertrust

   # --- ssh setup: agent + key + pinned known_hosts ---
   eval "$(ssh-agent -s)"
   ssh_key="$work/id_ed25519"
   printf '%s\n' "$APT_REPO_SSH_KEY" > "$ssh_key"
   chmod 600 "$ssh_key"
   ssh-add "$ssh_key" >/dev/null

   ssh_known="$work/known_hosts"
   cat > "$ssh_known" <<'KNOWN'
   # codeberg.org host keys (Ed25519). Source: https://docs.codeberg.org/security/ssh-fingerprint/
   codeberg.org ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIIVIC02vnjFyL+I4RHfvIGNtOgJMe769VTF1VR4EB3ZB
   KNOWN
   chmod 600 "$ssh_known"

   export GIT_SSH_COMMAND="ssh -i $ssh_key -o IdentitiesOnly=yes -o UserKnownHostsFile=$ssh_known -o StrictHostKeyChecking=yes"

   # --- reprepro tree assembly, as in the previous version ---
   mkdir -p "$work/apt/conf"
   cp dist/apt/conf/distributions "$work/apt/conf/distributions"
   for deb in "${debs[@]}"; do
     echo "reprepro: includedeb stable $deb"
     reprepro -b "$work/apt" includedeb stable "$deb"
   done

   # --- push the rebuilt tree ---
   git_checkout="$work/checkout"
   git clone --depth 1 --branch "$APT_REPO_BRANCH" "$APT_REPO_REMOTE" "$git_checkout"

   # Wipe the tree but keep .git and a single human-edited README the pages
   # repo owns (Phase 01 seeded it).
   find "$git_checkout" -mindepth 1 -maxdepth 1 \
     -not -name .git \
     -not -name README.md \
     -exec rm -rf {} +
   cp -r "$work/apt/dists" "$git_checkout/dists"
   cp -r "$work/apt/pool"  "$git_checkout/pool"
   cp dist/apt/key.gpg.asc "$git_checkout/key.gpg.asc"

   cat > "$git_checkout/.codebergpages.toml" <<'TOML'
   # Codeberg Pages serves this branch as the rs-modde apt repository.
   # The rs-modde release workflow rewrites this tree on every stable tag
   # via scripts/publish-apt.sh.
   TOML

   cd "$git_checkout"
   git -c user.name='modde release bot' \
       -c user.email='release-bot@modde.tartanoglu.com' \
       add -A
   if git diff --cached --quiet; then
     echo "apt: no changes for ${VERSION}; skipping push."
     exit 0
   fi
   git -c user.name='modde release bot' \
       -c user.email='release-bot@modde.tartanoglu.com' \
       commit -m "apt: publish modde ${VERSION}"
   git push --force-with-lease origin "HEAD:refs/heads/${APT_REPO_BRANCH}"
   ```
2. Rewrite the script's leading comment block to match the new env-var surface:
   - List `APT_REPO_GPG_KEY`, `APT_REPO_GPG_KEY_ID`, `APT_REPO_GPG_PASSPHRASE` (optional), `APT_REPO_SSH_KEY`.
   - Remove every mention of `APT_REPO_SSH_KEY`.
   - Document `APT_REPO_REMOTE` and `APT_REPO_BRANCH` overrides for local testing.
3. **If** the Phase 02 design used a different known_hosts fingerprint for `codeberg.org` (e.g. Codeberg rotates host keys), update the `KNOWN` heredoc to the current fingerprint from <https://docs.codeberg.org/security/ssh-fingerprint/> at the time of writing. Do not paste an unverified key.
4. Local smoke (no push):
   ```sh
   nix shell nixpkgs#reprepro nixpkgs#gnupg nixpkgs#openssh nixpkgs#git -c bash -c '
     bash -n scripts/publish-apt.sh
     # Static check: no token references survive.
     ! grep -E "APT_REPO_SSH_KEY|push_token|credential\.helper" scripts/publish-apt.sh
     # Static check: SSH path references are exact.
     grep -F "ssh://git@codeberg.org/caniko/rs-modde-apt.git" scripts/publish-apt.sh
     grep -F "pages" scripts/publish-apt.sh
   '
   ```
5. Do not run an end-to-end push from this phase; Phase 05 owns the smoke against the live repo. The static checks above are sufficient gating for this phase's diff to merge.

## Acceptance criteria

- [ ] `scripts/publish-apt.sh` exists, is executable (`test -x`), and `bash -n` parses cleanly.
- [ ] `grep -c 'APT_REPO_SSH_KEY\|push_token\|credential.helper' scripts/publish-apt.sh` returns `0`.
- [ ] `grep -F 'ssh://git@codeberg.org/caniko/rs-modde-apt.git' scripts/publish-apt.sh` returns exactly one match (the `APT_REPO_REMOTE` default).
- [ ] The script declares an `EXIT` trap that removes `$work` and tears down the ssh-agent (`ssh-agent -k`). `grep -E 'trap.*ssh-agent -k' scripts/publish-apt.sh` matches.
- [ ] The script pins `codeberg.org`'s host key via `UserKnownHostsFile` and uses `StrictHostKeyChecking=yes`. `grep -F 'StrictHostKeyChecking=yes' scripts/publish-apt.sh` matches.
- [ ] When `APT_REPO_SSH_KEY` is unset, running the script with `VERSION=0.0.0-test bash scripts/publish-apt.sh` exits 0 with a `::warning::APT_REPO_SSH_KEY unset` line on stdout.
- [ ] When `release/*.deb` does not exist, the script exits 0 with the existing `No .deb files in release/` warning.

## Files likely touched

- `scripts/publish-apt.sh` (rewritten).

## Pitfalls

- **`StrictHostKeyChecking=accept-new` looks fine but isn't**: it accepts a new host key on first run and pins it after. The runner has no "first run" supervision; a hostile network at first contact would pin the wrong key forever. Use `StrictHostKeyChecking=yes` with a pinned `UserKnownHostsFile`.
- **ssh-agent leaks**: if the script exits between `eval $(ssh-agent -s)` and the trap registration, the agent process is orphaned. Register the trap *before* starting the agent if possible, or accept that the trap that includes `ssh-agent -k` covers the common path and the runner's cgroup tear-down catches the rest.
- **`git push --force` vs `--force-with-lease`**: `--force-with-lease` refuses to overwrite a branch tip that has advanced since `git clone`. That is correct here — the only writer is this same script, so an advanced tip means a concurrent release race, which deserves a failure not a clobber.
- **`git clone --depth 1 --branch pages`**: requires Phase 01 to have set `pages` as the default branch (or at least to have made it explicitly clone-able). If Phase 01 went with URL choice B / Path B (`main` as serving branch), this script's branch default has to match. Coordinate via the README's recorded URL choice.
- **Codeberg host key drift**: the Ed25519 fingerprint pasted in the `KNOWN` heredoc is correct at the time this plan was written. If Codeberg rotates it (rare; announced via codeberg.org security posts), CI breaks with `Host key verification failed`. Treat this as a deliberate signal, not a bug; verify the new key from an out-of-band source and bump the heredoc.
- **`reprepro` includedeb behavior on duplicates**: re-running with the same `release/*.deb` set against an empty tree is idempotent; against a non-empty tree, the previous script's "wipe everything except .git/README.md and re-include" approach guarantees the rebuilt tree is a pure function of the .deb set plus the signing key, so re-runs are byte-stable (modulo `Release` file timestamps). Do not switch to incremental `includedeb` against the cloned tree; that introduces history that diverges from `release/*.deb` and complicates rollback.

## Reference

- Phase 01: established `caniko/rs-modde-apt` and the `pages` serving branch.
- Phase 02: provisioned `modde_apt_repo_ssh_key` in canix and the matching deploy key on Codeberg.
- Phase 04: will update `.forgejo/workflows/release.yml` to feed `secrets.modde_apt_repo_ssh_key` into `APT_REPO_SSH_KEY` and stop referencing the old token slot.
- Codeberg SSH fingerprints (canonical source): <https://docs.codeberg.org/security/ssh-fingerprint/>.
- The previous, HTTPS-token version of this script (for diff reference only; do not preserve any of its credential plumbing).
