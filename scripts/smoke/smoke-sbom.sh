#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"
RELEASE_DIR="$2"

need grype

sboms=()
collect_many sboms "$RELEASE_DIR" "*.cdx.json" "Generate supply-chain reports"

for sbom in "${sboms[@]}"; do
  grype "sbom:${sbom}" --fail-on high
done
