#!/usr/bin/env bash
# Build a signed apt repository tree from release/*.deb and push it to
# caniko/modde-apt for Codeberg-hosted serving at
# https://modde.tartanoglu.com/apt/.
#
# The script is idempotent for a given tag: rebuilding the same set of .deb
# files re-creates the same Packages/Release files (modulo Release timestamps).
# It is invoked from the release workflow on stable tags only.
#
# Required environment:
#   VERSION                   release tag (X.Y.Z), used in commit message only
#   APT_REPO_GPG_KEY          ascii-armored secret key for the apt repository
#   APT_REPO_GPG_KEY_ID       long-form key id or fingerprint that reprepro
#                             references via SignWith (e.g. D18B...E408)
#   APT_REPO_PUSH_TOKEN       Codeberg token with write access to caniko/modde-apt
#   APT_REPO_REMOTE           git remote URL (default: caniko/modde-apt on Codeberg)
#
# Optional:
#   APT_REPO_GPG_PASSPHRASE   if the apt key is password-protected
#
# Behavior:
# - When APT_REPO_GPG_KEY or APT_REPO_PUSH_TOKEN is unset, the script logs a
#   warning and exits 0. Use this to keep tag pushes green before the apt repo
#   has been bootstrapped (mirrors Homebrew/Scoop/Flathub gate semantics).
# - When secrets are present, the script:
#     1. Imports the secret key into a throwaway GNUPGHOME.
#     2. Stages every release/*.deb into a fresh reprepro tree under work/apt.
#     3. Commits dists/ + pool/ + key.gpg.asc to the modde-apt repo and pushes.
set -euo pipefail

VERSION="${VERSION:?VERSION must be set to the release tag}"
APT_REPO_REMOTE="${APT_REPO_REMOTE:-https://codeberg.org/caniko/modde-apt.git}"

if [ -z "${APT_REPO_GPG_KEY:-}" ] || [ -z "${APT_REPO_GPG_KEY_ID:-}" ]; then
  echo "::warning::APT_REPO_GPG_KEY / APT_REPO_GPG_KEY_ID unset; skipping apt publish."
  exit 0
fi
if [ -z "${APT_REPO_PUSH_TOKEN:-}" ]; then
  echo "::warning::APT_REPO_PUSH_TOKEN unset; skipping apt publish."
  exit 0
fi

debs=(release/*.deb)
if [ ! -e "${debs[0]}" ]; then
  echo "::warning::No .deb files in release/; nothing to publish to apt."
  exit 0
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
chmod 700 "$work"

GNUPGHOME="$work/gpg"
mkdir -p "$GNUPGHOME"
chmod 700 "$GNUPGHOME"
export GNUPGHOME

# Import the apt repo key.
printf '%s' "$APT_REPO_GPG_KEY" | gpg --batch --import 2>&1 | sed 's/^/gpg: /'
echo "${APT_REPO_GPG_KEY_ID}:6:" | gpg --batch --import-ownertrust

# reprepro needs the per-config tree at $work/apt; conf/distributions is
# checked into rs-modde at dist/apt/conf/distributions and is the canonical
# repo definition.
mkdir -p "$work/apt/conf"
cp dist/apt/conf/distributions "$work/apt/conf/distributions"

passphrase_args=()
if [ -n "${APT_REPO_GPG_PASSPHRASE:-}" ]; then
  passphrase_file="$work/passphrase"
  printf '%s' "$APT_REPO_GPG_PASSPHRASE" > "$passphrase_file"
  chmod 600 "$passphrase_file"
  passphrase_args=(--gnupg-home "$GNUPGHOME" --ask-passphrase --passphrase-file "$passphrase_file")
fi

for deb in "${debs[@]}"; do
  echo "reprepro: includedeb stable $deb"
  reprepro -b "$work/apt" includedeb stable "$deb"
done

# Stage the published tree in a git checkout of the apt repo, replace the
# served subdirectory, and push.
remote_with_token="$(printf '%s' "$APT_REPO_REMOTE" | sed "s#https://#https://caniko:${APT_REPO_PUSH_TOKEN}@#")"

git_checkout="$work/checkout"
git clone --depth 1 "$remote_with_token" "$git_checkout"

# Wipe previous tree but keep .git and a top-level README the bucket repo owns.
find "$git_checkout" -mindepth 1 -maxdepth 1 \
  -not -name .git \
  -not -name README.md \
  -exec rm -rf {} +

cp -r "$work/apt/dists" "$git_checkout/dists"
cp -r "$work/apt/pool"  "$git_checkout/pool"
cp dist/apt/key.gpg.asc "$git_checkout/key.gpg.asc"

cat > "$git_checkout/.codebergpages.toml" <<'TOML'
# Codeberg Pages serves this repository at https://modde.tartanoglu.com/apt/
# when the website CNAME forwards /apt/ to caniko.codeberg.page/modde-apt.
# Bootstrap details live in CONTRIBUTING.md ("crates.io publish policy" → apt
# section) in the rs-modde repo.
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
git push origin HEAD:refs/heads/main
