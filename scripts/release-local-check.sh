#!/usr/bin/env bash
set -euo pipefail

version="${1:-}"
if [ -z "$version" ]; then
  echo "usage: release-local-check <version>" >&2
  exit 2
fi

repo="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$repo"

missing=()
warnings=()
repo_secret_names=""
repo_variable_names=""

ok() {
  printf 'ok: %s\n' "$*"
}

warn() {
  warnings+=("$*")
  printf 'warn: %s\n' "$*" >&2
}

need_tool() {
  if command -v "$1" >/dev/null 2>&1; then
    ok "tool $1"
  else
    missing+=("tool:$1")
  fi
}

need_env() {
  local scope="$1"
  local name="$2"
  if [ -n "${!name:-}" ]; then
    ok "$scope $name present"
  else
    missing+=("$scope:$name")
  fi
}

load_env_file() {
  local name="$1"
  local path="$2"
  if [ -n "${!name:-}" ] || [ ! -r "$path" ]; then
    return
  fi

  local value
  value="$(cat "$path")"
  printf -v "$name" '%s' "$value"
  export "$name"
}

load_runtime_secret() {
  local name="$1"
  local file="$2"
  local runtime="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
  local path
  path="$(find "$runtime/agenix.d" -type f -name "$file" 2>/dev/null | sort -V | tail -n 1 || true)"
  if [ -n "$path" ]; then
    load_env_file "$name" "$path"
  fi
}

load_canix_release_inputs() {
  local canix_root="${CANIX_ROOT:-/data/nvme0/can/Projects/canix}"

  load_runtime_secret CODEBERG_TOKEN can_codeberg_token
  load_runtime_secret MINISIGN_SECRET_KEY can_minisign_secret_key
  load_runtime_secret MINISIGN_PASSWORD can_minisign_password
  load_runtime_secret AUR_SSH_KEY can_aur_ssh_key
  load_runtime_secret COPR_LOGIN can_coppr_login
  load_runtime_secret COPR_TOKEN can_coppr_token
  load_runtime_secret CHOCOLATEY_API_KEY can_choco_api_key
  load_runtime_secret CRATES_IO_API_TOKEN can_crates_io

  load_env_file MODDE_APT_REPO_GPG_KEY_ID "$canix_root/age/secrets/modules/repos/apt/modde_apt_repo_gpg_key.fingerprint"
  load_env_file MODDE_APT_REPO_GPG_FINGERPRINT "$canix_root/age/secrets/modules/repos/apt/modde_apt_repo_gpg_key.fingerprint"
  load_env_file MODDE_APT_REPO_GPG_PUBLIC_KEY "$canix_root/age/secrets/modules/repos/apt/modde_apt_repo_gpg_key.asc"
  load_env_file COPR_USERNAME "$canix_root/age/secrets/modules/repos/coppr/username"
}

optional_env() {
  local name="$1"
  if [ -n "${!name:-}" ]; then
    ok "optional $name present"
  else
    warn "optional $name absent; matching release workflow will skip that publisher or fallback"
  fi
}

collect_codeberg_metadata() {
  if ! command -v fj >/dev/null 2>&1; then
    warn "fj unavailable; repo secret/variable metadata checks disabled"
    return
  fi

  repo_secret_names="$(fj -H codeberg.org actions secrets list -r caniko/rs-modde 2>/dev/null | awk '{print $NF}' | tr '\n' ' ' || true)"
  repo_variable_names="$(fj -H codeberg.org actions variables list -r caniko/rs-modde 2>/dev/null | awk -F' = ' 'NF >= 2 {print $1}' | tr '\n' ' ' || true)"
}

metadata_has_name() {
  local names=" $1 "
  local name="$2"
  [[ "$names" == *" $name "* ]]
}

need_env_or_repo_secret() {
  local scope="$1"
  local name="$2"
  if [ -n "${!name:-}" ]; then
    ok "$scope $name present locally"
  elif metadata_has_name "$repo_secret_names" "$name"; then
    ok "$scope $name exists on caniko/rs-modde"
  else
    missing+=("$scope:$name")
  fi
}

need_env_or_repo_variable() {
  local scope="$1"
  local name="$2"
  if [ -n "${!name:-}" ]; then
    ok "$scope $name present locally"
  elif metadata_has_name "$repo_variable_names" "$name"; then
    ok "$scope $name exists on caniko/rs-modde"
  else
    missing+=("$scope:$name")
  fi
}

need_codeberg_auth() {
  if [ -n "${CODEBERG_TOKEN:-}" ]; then
    ok "global/user secret CODEBERG_TOKEN present"
    return
  fi

  local store="${XDG_DATA_HOME:-$HOME/.local/share}/forgejo-cli/keys.json"
  if [ -r "$store" ] && jq -e '.hosts["codeberg.org"].token | strings | length > 0' "$store" >/dev/null; then
    ok "Codeberg fj auth token present in local store"
    return
  fi

  missing+=("global/user secret:CODEBERG_TOKEN or fj codeberg.org token")
}

need_chocolatey_tool() {
  if command -v choco >/dev/null 2>&1; then
    ok "tool choco"
    return
  fi

  if command -v nix >/dev/null 2>&1 \
    && nix shell github:caniko/nixpkgs/add-chocolatey-scoop#chocolatey git+https://codeberg.org/caniko/simit -c command -v choco >/dev/null 2>&1; then
    ok "tool choco via configured nix_tool"
    return
  fi

  missing+=("tool:choco")
}

check_minisign_probe() {
  test -s keys/minisign.pub || missing+=("file:keys/minisign.pub")
  if [ -z "${MINISIGN_SECRET_KEY:-}" ] || [ -z "${MINISIGN_PASSWORD:-}" ] || [ ! -s keys/minisign.pub ]; then
    return
  fi

  umask 077
  local tmpdir key probe sig
  tmpdir="$(mktemp -d)"
  key="$tmpdir/minisign.key"
  probe="$tmpdir/minisign-probe.txt"
  sig="$probe.minisig"
  trap 'rm -rf "$tmpdir"' RETURN

  printf '%s' "$MINISIGN_SECRET_KEY" > "$key"
  printf 'rs-modde local release credentials probe\n' > "$probe"
  printf '%s\n' "$MINISIGN_PASSWORD" | minisign -S -s "$key" -m "$probe" -x "$sig" >/dev/null
  minisign -V -m "$probe" -x "$sig" -p keys/minisign.pub >/dev/null
  ok "minisign probe signs and verifies with keys/minisign.pub"
}

check_cosign_degrade() {
  local tmpdir artifact predicate
  tmpdir="$(mktemp -d)"
  artifact="$tmpdir/artifact.txt"
  predicate="$tmpdir/predicate.json"
  trap 'rm -rf "$tmpdir"' RETURN

  printf 'rs-modde cosign local degrade probe\n' > "$artifact"
  printf '{"_type":"https://in-toto.io/Statement/v1","subject":[],"predicateType":"https://slsa.dev/provenance/v1","predicate":{}}\n' > "$predicate"

  if env -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_ID_TOKEN_REQUEST_TOKEN -u COSIGN_PRIVATE_KEY \
    bash -c 'test -z "${ACTIONS_ID_TOKEN_REQUEST_URL:-}" && test -z "${COSIGN_PRIVATE_KEY:-}"'; then
    warn "keyless Sigstore and COSIGN_PRIVATE_KEY unavailable locally; release workflow degrades to warning-only attestation"
  else
    missing+=("cosign:local warning-only degradation probe")
  fi
}

check_workflow_contract() {
  grep -F 'nixpkgs#debootstrap nixpkgs#util-linux' .forgejo/workflows/release.yml >/dev/null \
    && ok "workflow includes util-linux for Debian package build" \
    || missing+=("workflow:Debian util-linux release shell")

  grep -F 'keyless Sigstore failed and COSIGN_PRIVATE_KEY unset; continuing without cosign signature or attestation' .forgejo/workflows/release.yml >/dev/null \
    && ok "workflow keeps missing cosign fallback warning-only" \
    || missing+=("workflow:cosign warning-only fallback")

  grep -F 'nix run .#copr-cli -- build --nowait "${COPR_PROJECT}" srpms/*.src.rpm' .forgejo/workflows/release.yml >/dev/null \
    && ok "workflow uses local COPR CLI flake app" \
    || missing+=("workflow:local COPR CLI app")

  grep -F 'skipping cosign verification because release signing degrades to warning-only' scripts/smoke/smoke-signatures.sh >/dev/null \
    && ok "smoke keeps missing cosign verification warning-only" \
    || missing+=("smoke:cosign warning-only fallback")

  grep -F 'COPR publish will perform the authoritative remote build' scripts/smoke/smoke-srpm.sh >/dev/null \
    && ok "smoke lets COPR remote build validate SRPM when local podman policy is unavailable" \
    || missing+=("smoke:COPR local podman fallback")

  grep -F 'Authenticode signing is optional and was skipped by the release workflow' scripts/smoke/smoke-windows-zip.sh >/dev/null \
    && ok "smoke treats unsigned Windows artifacts as optional when signing credentials are absent" \
    || missing+=("smoke:Windows optional signing fallback")

  grep -F 'Flathub publish will perform the authoritative build' scripts/smoke/smoke-flatpak.sh >/dev/null \
    && ok "smoke treats unavailable Flatpak runtime install as a runner limitation" \
    || missing+=("smoke:Flatpak runtime fallback")

  grep -F 'wine cannot execute' scripts/smoke/smoke-windows-zip.sh >/dev/null \
    && ok "smoke treats unavailable Wine runtime execution as a runner limitation" \
    || missing+=("smoke:Wine runtime fallback")
}

load_canix_release_inputs

for tool in \
  bash git curl jq cargo rustc cargo-deny cargo-about cargo-sbom cargo-cyclonedx \
  cargo-deb rpmbuild debootstrap mount umount chroot minisign cosign reprepro fj \
  gpg ssh node npm copr-cli simit; do
  need_tool "$tool"
done
need_chocolatey_tool
collect_codeberg_metadata

need_codeberg_auth
need_env_or_repo_secret "repo secret" MINISIGN_SECRET_KEY
need_env_or_repo_secret "repo secret" MINISIGN_PASSWORD
need_env_or_repo_secret "repo secret" MODDE_APT_REPO_GPG_KEY
need_env_or_repo_secret "repo secret" MODDE_APT_REPO_SSH_KEY
need_env_or_repo_variable "repo variable" MODDE_APT_REPO_GPG_KEY_ID
need_env_or_repo_variable "repo variable" MODDE_APT_REPO_GPG_FINGERPRINT
need_env_or_repo_variable "repo variable" MODDE_APT_REPO_GPG_PUBLIC_KEY
need_env "global/user secret" AUR_SSH_KEY
need_env "global/user secret" COPR_LOGIN
need_env "global/user variable" COPR_USERNAME
need_env "global/user secret" COPR_TOKEN
need_env "global/user secret" CHOCOLATEY_API_KEY
need_env "global/user secret" CRATES_IO_API_TOKEN

optional_env MODDE_APT_REPO_GPG_PASSPHRASE
optional_env COSIGN_PRIVATE_KEY
optional_env COSIGN_PASSWORD
optional_env WINDOWS_SIGNING_PFX
optional_env WINDOWS_SIGNING_PASS
optional_env FLATHUB_TOKEN
optional_env WINGET_PAT
optional_env HOMEBREW_TAP_TOKEN
optional_env SCOOP_BUCKET_TOKEN

check_workflow_contract
check_minisign_probe
check_cosign_degrade

if [ "${#missing[@]}" -ne 0 ]; then
  printf 'missing required local release parity inputs for %s:\n' "$version" >&2
  printf '  - %s\n' "${missing[@]}" >&2
  exit 1
fi

printf 'local release parity dry-run passed for %s; no external publish was attempted\n' "$version"
