#!/usr/bin/env bash
set -euo pipefail

version="${1:-}"
release_dir="${2:-release}"
if [ -z "$version" ]; then
  echo "usage: build-deb <version> [release-dir]" >&2
  exit 2
fi

case "$(uname -s)" in
  Linux) ;;
  *)
    echo "error: Debian package build is supported only on Linux hosts." >&2
    exit 1
    ;;
esac
deb_version="${version}-1"

case "$(uname -m)" in
  x86_64)
    deb_arch="amd64"
    ;;
  *)
    echo "error: unsupported Debian package architecture $(uname -m); dist/apt/conf/distributions currently declares only amd64." >&2
    echo "required upstream producer: APT architecture metadata and smoke coverage for this architecture." >&2
    echo "validation: grep -F 'Architectures:' dist/apt/conf/distributions && bash scripts/smoke/smoke-linux-distribution-gates.sh ${version} ${release_dir}" >&2
    exit 1
    ;;
esac

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "error: missing required command: $1" >&2
    echo "required upstream producer: Nix dev shell or equivalent Debian packaging toolchain." >&2
    echo "regenerate: nix develop -c bash scripts/build-deb.sh ${version} ${release_dir}" >&2
    echo "validation: command -v $1" >&2
    exit 1
  }
}

need cargo
need cargo-deb
need dpkg-deb

mkdir -p "$release_dir"

echo "building Debian package binaries"
cargo build --release --locked --package modde --package modde-ui

build_one() {
  local cargo_package="$1"
  local deb_name="$2"
  local output="${release_dir}/${deb_name}_${version}_${deb_arch}.deb"

  cargo deb --locked --package "$cargo_package" --output "$output"
  test -s "$output"
  dpkg-deb --field "$output" Package Version Architecture >/dev/null
  dpkg-deb --field "$output" Package | grep -Fx "$deb_name" >/dev/null
  dpkg-deb --field "$output" Version | grep -Fx "$deb_version" >/dev/null
  dpkg-deb --field "$output" Architecture | grep -Fx "$deb_arch" >/dev/null
  printf 'built: %s\n' "$output"
}

build_one modde modde
exit 0
