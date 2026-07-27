#!/usr/bin/env bash
set -euo pipefail

version="${1:-}"
release_dir="${2:-release}"
if [ -z "$version" ]; then
  echo "usage: build-deb <version> [release-dir]" >&2
  exit 2
fi

# Compatibility entry point for callers that have not moved to the generated
# release workflow yet. Simit owns package selection, cargo-deb invocation,
# architecture validation, and Debian metadata checks.
simit_bin="${SIMIT_BIN:-simit}"
args=(dist apt build --version "$version" --release-dir "$release_dir")
if [ "${MODDE_BUILD_DEB_IN_DEVSHELL:-0}" = "1" ]; then
  exec "$simit_bin" "${args[@]}"
fi
exec nix develop -c "$simit_bin" "${args[@]}"
