#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"
VERSION="$1"
RELEASE_DIR="$2"

need flatpak
need flatpak-builder
need jq
need timeout

manifest="$(find_one "$RELEASE_DIR" "com.tartanoglu.modde.json" "Build release artifacts flatpak-manifest output")"
source_tarball="$(find_one "$RELEASE_DIR" "rs-modde-${VERSION}.tar.gz" "Build SRPM for COPR source archive copy into release/")"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

local_manifest="$tmpdir/com.tartanoglu.modde.json"
source_uri="file://$(cd "$(dirname "$source_tarball")" && pwd)/$(basename "$source_tarball")"

jq --arg source_uri "$source_uri" '
  .modules[0].sources |= map(
    if .type == "archive" and (.url | test("rs-modde-.*[.]tar[.]gz$"))
    then .url = $source_uri
    else .
    end
  )
' "$manifest" > "$local_manifest"

flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak-builder --user --install --force-clean --install-deps-from=flathub "$tmpdir/build" "$local_manifest"

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-$tmpdir/runtime}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"

timeout 10 flatpak run --user --no-sandbox com.tartanoglu.modde || {
  status=$?
  if [ "$status" -eq 124 ]; then
    echo "flatpak launch stayed alive for 10s; treating launch smoke as passed"
  else
    die "flatpak run failed; add a no-window modde-ui smoke flag if the atlas runner has no display server"
  fi
}
