#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"
VERSION="$1"
RELEASE_DIR="$2"

need dpkg-deb

debs=()
collect_glob debs "${RELEASE_DIR}/*.deb"

if [ "${#debs[@]}" -eq 0 ]; then
  warn "no .deb artifacts found; skipping Debian package smoke checks (the debuild chroot may not be available in this environment)"
  exit 0
fi

for deb in "${debs[@]}"; do
  dpkg-deb -I "$deb"
  dpkg-deb --field "$deb" Package Version Architecture
done

if command -v lintian >/dev/null 2>&1; then
  if ! lintian --pedantic "${debs[@]}"; then
    warn "lintian reported Debian packaging diagnostics; continuing because lintian is warning-only in smoke policy"
  fi
fi

if [ "$(id -u)" -ne 0 ] && { ! command -v sudo >/dev/null 2>&1 || ! sudo -n true >/dev/null 2>&1; }; then
  warn "skipping Debian chroot install smoke because root/passwordless sudo is unavailable; dpkg-deb metadata checks passed"
  exit 0
fi

need debootstrap

root_run_noninteractive() {
  if [ "$(id -u)" -eq 0 ]; then
    "$@"
  else
    sudo -n "$@"
  fi
}

rootdir="$(mktemp -d)"
cleanup() {
  root_run_noninteractive rm -rf "$rootdir"
}
trap cleanup EXIT

root_run_noninteractive debootstrap --variant=minbase bookworm "$rootdir" http://deb.debian.org/debian
root_run_noninteractive install -d -m 0755 "$rootdir/tmp/modde-debs"
for deb in "${debs[@]}"; do
  root_run_noninteractive cp "$deb" "$rootdir/tmp/modde-debs/"
done

root_run_noninteractive chroot "$rootdir" apt-get update
root_run_noninteractive chroot "$rootdir" apt-get install -y --no-install-recommends ca-certificates
root_run_noninteractive chroot "$rootdir" apt-get install -y /tmp/modde-debs/*.deb
run_version_check "$VERSION" "Debian package modde --version" root_run_noninteractive chroot "$rootdir" modde --version
