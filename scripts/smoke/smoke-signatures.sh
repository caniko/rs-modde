#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

require_args "$@"
RELEASE_DIR="$2"

need cosign
need minisign

test -s keys/minisign.pub || die "missing keys/minisign.pub; required upstream producer: Phase 02 minisign public key committed to keys/"
test -s "$RELEASE_DIR/SHA256SUMS.txt" || die "missing ${RELEASE_DIR}/SHA256SUMS.txt"
test -s "$RELEASE_DIR/SHA256SUMS.txt.minisig" || die "missing ${RELEASE_DIR}/SHA256SUMS.txt.minisig"

minisign -V \
  -m "$RELEASE_DIR/SHA256SUMS.txt" \
  -x "$RELEASE_DIR/SHA256SUMS.txt.minisig" \
  -p keys/minisign.pub

cosign_identity_args=()
cosign_key_file=""
cleanup() {
  rm -f "$cosign_key_file"
}
trap cleanup EXIT

if [ -n "${COSIGN_PUBLIC_KEY:-}" ]; then
  cosign_key_file="$(mktemp)"
  printf '%s' "$COSIGN_PUBLIC_KEY" > "$cosign_key_file"
  cosign_identity_args=(--key "$cosign_key_file")
elif [ -s keys/cosign.pub ]; then
  cosign_identity_args=(--key keys/cosign.pub)
elif [ -n "${COSIGN_CERTIFICATE_IDENTITY:-}" ] && [ -n "${COSIGN_CERTIFICATE_OIDC_ISSUER:-}" ]; then
  cosign_identity_args=(
    --certificate-identity "$COSIGN_CERTIFICATE_IDENTITY"
    --certificate-oidc-issuer "$COSIGN_CERTIFICATE_OIDC_ISSUER"
  )
else
  die "missing cosign verification material; set COSIGN_PUBLIC_KEY, commit keys/cosign.pub, or set COSIGN_CERTIFICATE_IDENTITY and COSIGN_CERTIFICATE_OIDC_ISSUER for keyless verification"
fi

signed_artifacts=()
for pattern in "*.tar.gz" "*.zip" "*.AppImage" "*.deb" "*.src.rpm" "*.exe"; do
  pattern_matches=()
  collect_many pattern_matches "$RELEASE_DIR" "$pattern" "Sign checksums and attest release artifacts"
  signed_artifacts+=("${pattern_matches[@]}")
done

mapfile -t signed_artifacts < <(printf '%s\n' "${signed_artifacts[@]}" | sort)

for artifact in "${signed_artifacts[@]}"; do
  test -s "${artifact}.cosign.bundle" || die "missing cosign signature bundle for ${artifact}"
  test -s "${artifact}.intoto.bundle" || die "missing cosign attestation bundle for ${artifact}"
  test -s "${artifact}.intoto.jsonl" || die "missing intoto attestation payload for ${artifact}"

  cosign verify-blob "${cosign_identity_args[@]}" --bundle "${artifact}.cosign.bundle" "$artifact"
  cosign verify-blob-attestation "${cosign_identity_args[@]}" --type slsaprovenance1 --bundle "${artifact}.intoto.bundle" "$artifact"
done
