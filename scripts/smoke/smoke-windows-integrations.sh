#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"
VERSION="$1"
RELEASE_DIR="$2"

need timeout
need unzip
need wine

zip_artifact="$(find_one "$RELEASE_DIR" "modde-${VERSION}-x86_64-windows.zip" "Sign Windows Authenticode artifacts")"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

unzip -q "$zip_artifact" -d "$tmpdir/zip"
exe="$tmpdir/zip/modde.exe"
test -s "$exe" || die "${zip_artifact} did not contain modde.exe"

export WINEPREFIX="$tmpdir/wine"
export WINEDEBUG=-all

run_version_check "$VERSION" "Windows integration modde.exe --version via wine" timeout 60 wine "$exe" --version

tool_output="$(timeout 60 wine "$exe" tool status --game cyberpunk2077 2>&1)"
printf '%s\n' "$tool_output"
grep -F "ReShade" <<< "$tool_output" > /dev/null || die "Windows tool registry did not expose ReShade"
grep -F "OptiScaler" <<< "$tool_output" > /dev/null || die "Windows tool registry did not expose OptiScaler"
if grep -E "MangoHud|vkBasalt|GameMode|Proton" <<< "$tool_output" > /dev/null; then
  die "Windows tool registry exposed Linux-only tools"
fi

handler_output="$(timeout 60 wine "$exe" nxm install-handler 2>&1)"
printf '%s\n' "$handler_output"
grep -F "Registered nxm:// protocol handler in Windows registry." <<< "$handler_output" > /dev/null \
  || die "Windows NXM protocol handler was not registered"

touch "$RELEASE_DIR/windows-integrations.ok"
echo "Windows integration smoke marker: $RELEASE_DIR/windows-integrations.ok"
