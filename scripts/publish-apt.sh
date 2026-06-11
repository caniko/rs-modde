#!/usr/bin/env bash
# Build a signed apt repository tree from release/*.deb and push it to
# caniko/apt-modde over SSH for Codeberg Pages serving.
#
# The script is idempotent for a given tag: rebuilding the same set of .deb
# files re-creates the same Packages/Release files (modulo Release timestamps).
# It is invoked from the release workflow on stable tags only.
#
# Required environment:
#   VERSION                   release tag (X.Y.Z), used in commit message only
#   APT_REPO_GPG_KEY          ascii-armored secret key for the apt repository
#   APT_REPO_SSH_KEY          ed25519 private key with write deploy-key access
#                             to caniko/apt-modde
#
# Optional:
#   APT_REPO_GPG_PASSPHRASE   if the apt key is password-protected
#   APT_REPO_REMOTE           git remote URL override for local testing
#                             (default: apt-modde on Codeberg over SSH)
#   APT_REPO_BRANCH           branch override for local testing
#                             (default: pages)
#
# Behavior:
# - Missing apt signing or SSH material is a hard failure. The apt repository is
#   documented as a supported Linux channel, so publishing must not silently
#   omit required signing data.
# - When all inputs are present, the script:
#     1. Imports the secret key into a throwaway GNUPGHOME.
#     2. Loads the SSH deploy key into a throwaway ssh-agent.
#     3. Stages every release/*.deb into a fresh reprepro tree under work/apt.
#     4. Commits dists/ + pool/ + key.gpg.asc to the apt repo and force-pushes
#        the Pages-serving branch with lease protection.
set -euo pipefail

VERSION="${VERSION:?VERSION must be set to the release tag}"
APT_REPO_REMOTE="${APT_REPO_REMOTE:-ssh://git@codeberg.org/caniko/apt-modde.git}"
APT_REPO_BRANCH="${APT_REPO_BRANCH:-pages}"

if [ -z "${APT_REPO_GPG_KEY:-}" ]; then
  echo "error: APT_REPO_GPG_KEY is unset; required upstream producer: apt repository signing-key bootstrap." >&2
  echo 'regenerate/validate: export MODDE_APT_REPO_GPG_KEY_ID=<fingerprint>; gpg --armor --export "$MODDE_APT_REPO_GPG_KEY_ID" > dist/apt/key.gpg.asc; test -s dist/apt/key.gpg.asc && gpg --show-keys --with-fingerprint dist/apt/key.gpg.asc' >&2
  exit 1
fi
if [ -z "${APT_REPO_SSH_KEY:-}" ]; then
  echo "error: APT_REPO_SSH_KEY is unset; required upstream producer: Codeberg apt-modde deploy key secret." >&2
  echo "validation: ssh -i <deploy-key> -T git@codeberg.org" >&2
  exit 1
fi

debs=(release/*.deb)
if [ ! -e "${debs[0]}" ]; then
  echo "error: no .deb files in release/; required upstream producer: Build Debian packages." >&2
  echo "validation: ls -l release/*.deb" >&2
  exit 1
fi

if [ ! -s dist/apt/key.gpg.asc ]; then
  echo "error: missing dist/apt/key.gpg.asc; required upstream producer: apt repository signing-key bootstrap." >&2
  echo 'regenerate: gpg --armor --export "$MODDE_APT_REPO_GPG_KEY_ID" > dist/apt/key.gpg.asc' >&2
  echo "validation: test -s dist/apt/key.gpg.asc && gpg --show-keys --with-fingerprint dist/apt/key.gpg.asc" >&2
  exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"; [ -n "${SSH_AGENT_PID:-}" ] && ssh-agent -k >/dev/null 2>&1 || true' EXIT
chmod 700 "$work"

# --- gpg setup, exactly as the previous version ---
GNUPGHOME="$work/gpg"; mkdir -p "$GNUPGHOME"; chmod 700 "$GNUPGHOME"
export GNUPGHOME
printf '%s' "$APT_REPO_GPG_KEY" | gpg --batch --import 2>&1 | sed 's/^/gpg: /'
APT_REPO_GPG_KEY_ID="$(gpg --batch --with-colons --list-secret-keys | awk -F: '$1 == "fpr" { print $10; exit }')"
test -n "$APT_REPO_GPG_KEY_ID"
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
   -c user.email='release-bot@modde.rs' \
    add -A
if git diff --cached --quiet; then
  echo "apt: no changes for ${VERSION}; skipping push."
  exit 0
fi
git -c user.name='modde release bot' \
   -c user.email='release-bot@modde.rs' \
    commit -m "apt: publish modde ${VERSION}"
git push --force-with-lease origin "HEAD:refs/heads/${APT_REPO_BRANCH}"
