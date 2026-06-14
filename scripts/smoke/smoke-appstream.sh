#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"

need appstreamcli

test -s dist/com.tartanoglu.modde.metainfo.xml || die "missing dist/com.tartanoglu.modde.metainfo.xml; required upstream producer: Phase 04c Flathub/AppStream metadata"
output="$(mktemp)"
trap 'rm -f "$output"' EXIT

if appstreamcli validate --strict dist/com.tartanoglu.modde.metainfo.xml >"$output" 2>&1; then
  cat "$output"
  exit 0
fi

cat "$output" >&2
if awk '
  /^E:/ { bad = 1 }
  /^W:/ && $0 !~ /(url-not-reachable|screenshot-image-not-found)/ { bad = 1 }
  END { exit bad }
' "$output"; then
  warn "AppStream metadata is structurally valid; live website/screenshot URL reachability is warning-only for local deploy"
  exit 0
fi

die "AppStream metadata validation failed with non-network diagnostics"
