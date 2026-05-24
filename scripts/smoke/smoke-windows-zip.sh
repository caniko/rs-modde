#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"
VERSION="$1"
RELEASE_DIR="$2"

need osslsigncode
need tar
need timeout
need unzip
need wine

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

verify_and_run() {
  local label="$1"
  local exe="$2"

  test -s "$exe" || die "missing ${label} executable at ${exe}"
  osslsigncode verify -in "$exe"

  export WINEPREFIX="$tmpdir/wine"
  export WINEDEBUG=-all
  run_version_check "$VERSION" "${label} modde.exe --version via wine" timeout 60 wine "$exe" --version
}

zip_artifact="$(find_one "$RELEASE_DIR" "modde-${VERSION}-x86_64-windows.zip" "Sign Windows Authenticode artifacts")"
mkdir -p "$tmpdir/zip"
unzip -q "$zip_artifact" -d "$tmpdir/zip"
verify_and_run "Windows zip" "$tmpdir/zip/modde.exe"
test -s "$tmpdir/zip/modde-ui.exe" || die "${zip_artifact} did not contain modde-ui.exe"
osslsigncode verify -in "$tmpdir/zip/modde-ui.exe"

tarball="$(find_one "$RELEASE_DIR" "modde-${VERSION}-x86_64-windows.tar.gz" "Sign Windows Authenticode artifacts")"
mkdir -p "$tmpdir/tarball"
tar xzf "$tarball" -C "$tmpdir/tarball"
verify_and_run "Windows tarball" "$tmpdir/tarball/modde.exe"
