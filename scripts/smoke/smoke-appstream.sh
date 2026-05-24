#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"

need appstreamcli

test -s dist/com.tartanoglu.modde.metainfo.xml || die "missing dist/com.tartanoglu.modde.metainfo.xml; required upstream producer: Phase 04c Flathub/AppStream metadata"
appstreamcli validate --strict dist/com.tartanoglu.modde.metainfo.xml
