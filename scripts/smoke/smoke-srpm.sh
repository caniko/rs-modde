#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"
VERSION="$1"
RELEASE_DIR="$2"

need podman

srpm="$(find_one "$RELEASE_DIR" "*.src.rpm" "Build SRPM for COPR")"

if command -v rpmlint > /dev/null 2>&1; then
  if ! rpmlint --strict "$srpm"; then
    warn "rpmlint reported RPM packaging diagnostics; continuing because rpmlint is warning-only in smoke policy"
  fi
else
  warn "rpmlint is unavailable outside the Fedora rebuild container; style lint skipped before rebuild"
fi

repo_root="$(cd "${RELEASE_DIR}/.." && pwd)"
srpm_abs="$(cd "$(dirname "$srpm")" && pwd)/$(basename "$srpm")"

podman run --rm \
  --security-opt label=disable \
  -v "${repo_root}:/work" \
  -v "${srpm_abs}:/tmp/modde.src.rpm:ro" \
  -w /work \
  registry.fedoraproject.org/fedora:latest \
  bash -lc "
    set -euo pipefail
    dnf5 -y install rpm-build rpmlint cargo rust gcc pkgconf-pkg-config openssl-devel dbus-devel wayland-devel libxkbcommon-devel vulkan-loader-devel
    rpmlint --strict /tmp/modde.src.rpm || echo 'warning: rpmlint reported RPM packaging diagnostics; continuing because rpmlint is warning-only in smoke policy' >&2
    rpmbuild --rebuild /tmp/modde.src.rpm --define '_topdir /tmp/rpmbuild'
    dnf5 -y install /tmp/rpmbuild/RPMS/*/*.rpm
    output=\"\$(modde --version 2>&1)\"
    printf '%s\n' \"\$output\"
    grep -F -- '${VERSION}' <<< \"\$output\" > /dev/null
  "
