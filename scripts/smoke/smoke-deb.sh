#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"
VERSION="$1"
RELEASE_DIR="$2"

need debootstrap
need dpkg-deb

debs=()
collect_many debs "$RELEASE_DIR" "*.deb" "Build Debian packages workflow step; regenerate with cargo-deb inside the Debian debootstrap root"

for deb in "${debs[@]}"; do
  dpkg-deb -I "$deb"
done

rootdir="$(mktemp -d)"
cleanup() {
  root_run rm -rf "$rootdir"
}
trap cleanup EXIT

root_run debootstrap --variant=minbase bookworm "$rootdir" http://deb.debian.org/debian
root_run install -d -m 0755 "$rootdir/tmp/modde-debs"
for deb in "${debs[@]}"; do
  root_run cp "$deb" "$rootdir/tmp/modde-debs/"
done

root_run chroot "$rootdir" apt-get update
root_run chroot "$rootdir" apt-get install -y --no-install-recommends ca-certificates lintian
if ! root_run chroot "$rootdir" lintian --pedantic /tmp/modde-debs/*.deb; then
  warn "lintian reported Debian packaging diagnostics; continuing because lintian is warning-only in smoke policy"
fi

root_run chroot "$rootdir" apt-get install -y /tmp/modde-debs/*.deb
run_version_check "$VERSION" "Debian package modde --version" root_run chroot "$rootdir" modde --version
