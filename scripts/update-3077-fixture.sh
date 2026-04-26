#!/usr/bin/env bash
set -euo pipefail

cargo run -p modde-sources --bin update-wabbajack-fixture -- 3077 "$@"
