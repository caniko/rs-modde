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
cargo_sources="$(find_one "$RELEASE_DIR" "cargo-sources.json" "flatpak-cargo-generator output")"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

local_manifest="$tmpdir/com.tartanoglu.modde.json"
source_uri="file://$(cd "$(dirname "$source_tarball")" && pwd)/$(basename "$source_tarball")"
cp "$cargo_sources" "$tmpdir/cargo-sources.json"

jq --arg source_uri "$source_uri" '
  .modules[0].sources |= map(
    if type == "object" and .type == "archive" and (.url | test("rs-modde-.*[.]tar[.]gz$"))
    then .url = $source_uri
    else .
    end
  )
' "$manifest" > "$local_manifest"

if [ "${MODDE_FLATPAK_MANIFEST_ONLY:-0}" = "1" ]; then
  jq -e '."app-id" == "com.tartanoglu.modde" and (.modules | length > 0)' "$local_manifest" > /dev/null
  warn "flatpak-builder execution skipped by MODDE_FLATPAK_MANIFEST_ONLY=1; manifest and cargo source metadata validated"
  exit 0
fi

flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak_log="$tmpdir/flatpak-builder.log"
if ! flatpak-builder --user --install --force-clean --state-dir="$tmpdir/state" --install-deps-from=flathub "$tmpdir/build" "$local_manifest" > "$flatpak_log" 2>&1; then
  cat "$flatpak_log"
  if grep -E "open[(]O_TMPFILE[)]|Error installing deps|Failed to install org[.]freedesktop[.]Sdk" "$flatpak_log" > /dev/null; then
    warn "flatpak-builder cannot install runtime dependencies in this runner; manifest parsing passed and Flathub publish will perform the authoritative build"
    exit 0
  fi
  exit 1
fi
cat "$flatpak_log"

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
