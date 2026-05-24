#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"
VERSION="$1"
RELEASE_DIR="$2"

need chmod
need timeout

appimage="$(find_one "$RELEASE_DIR" "modde-${VERSION}-x86_64.AppImage" "Build release artifacts appimage-cli output")"
chmod +x "$appimage"
run_version_check "$VERSION" "AppImage modde --version" timeout 30 "$appimage" --version
