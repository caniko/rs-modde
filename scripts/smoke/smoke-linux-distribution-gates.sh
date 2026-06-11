#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"
VERSION="$1"
RELEASE_DIR="$2"

need awk
need grep
need test

assert_file() {
  local path="$1"
  local producer="$2"

  test -s "$path" || die "missing ${path}; required upstream producer: ${producer}"
}

assert_optional_glob() {
  local pattern="$1"
  local producer="$2"

  if ! compgen -G "$pattern" > /dev/null; then
    die "missing artifact matching ${pattern}; required upstream producer: ${producer}"
  fi
}

assert_file Cargo.toml "workspace manifest"
assert_file flake.nix "release linuxDistributionSupport table"
assert_file dist/apt/conf/distributions "APT repository metadata"
assert_file dist/aur/modde/PKGBUILD "AUR source package template"
assert_file dist/aur/modde/.SRCINFO "AUR source package metadata"
assert_file dist/aur/modde-bin/PKGBUILD "AUR binary package template"
assert_file dist/aur/modde-bin/.SRCINFO "AUR binary package metadata"
assert_file dist/aur/modde-git/PKGBUILD "AUR development package template"
assert_file dist/aur/modde-git/.SRCINFO "AUR development package metadata"
assert_file modde.spec "COPR SRPM spec"

grep -q "linuxDistributionSupport" flake.nix || \
  die "flake.nix does not define linuxDistributionSupport; required upstream producer: release configuration"
for channel in apt copr aur nix flatpak appimage tarball; do
  grep -q "${channel} = {" flake.nix || \
    die "linux_distribution_support is missing channel ${channel}; required upstream producer: release configuration"
done

grep -q '^Architectures: amd64$' dist/apt/conf/distributions || \
  die "APT metadata must keep Architectures: amd64 until arm64 .deb builds are produced"

if [ ! -s dist/apt/key.gpg.asc ]; then
  die "missing dist/apt/key.gpg.asc; required upstream producer: apt repository signing-key bootstrap; regenerate with: gpg --armor --export \"\$MODDE_APT_REPO_GPG_KEY_ID\" > dist/apt/key.gpg.asc; validate with: test -s dist/apt/key.gpg.asc && gpg --show-keys --with-fingerprint dist/apt/key.gpg.asc"
fi

assert_optional_glob "${RELEASE_DIR}/modde-${VERSION}-x86_64-linux.tar.gz" "Build release artifacts tarball output"
assert_optional_glob "${RELEASE_DIR}/modde-${VERSION}-aarch64-linux.tar.gz" "Build release artifacts aarch64 tarball output"
assert_optional_glob "${RELEASE_DIR}/modde-${VERSION}-x86_64.AppImage" "Build release artifacts appimage-cli output"
assert_optional_glob "${RELEASE_DIR}/modde-ui-${VERSION}-x86_64.AppImage" "Build release artifacts appimage-ui output"
assert_optional_glob "${RELEASE_DIR}/com.tartanoglu.modde.json" "Build release artifacts flatpak-manifest output"
assert_optional_glob "${RELEASE_DIR}/cargo-sources.json" "flatpak-cargo-generator output"
assert_optional_glob "${RELEASE_DIR}/rs-modde-${VERSION}.tar.gz" "Build release artifacts source archive"

debs=()
mapfile -t debs < <(compgen -G "${RELEASE_DIR}"/*.deb | sort || true)
if [ "${#debs[@]}" -eq 0 ]; then
  warn "no .deb artifacts found; Debian-family release channel remains gated off until Build Debian packages produces .deb files"
fi

srpms=()
mapfile -t srpms < <(compgen -G "${RELEASE_DIR}"/*.src.rpm | sort || true)
if [ "${#srpms[@]}" -eq 0 ]; then
  die "missing .src.rpm artifact; required upstream producer: Build SRPM for COPR"
fi
