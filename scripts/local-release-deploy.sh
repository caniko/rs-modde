#!/usr/bin/env bash
set -euo pipefail

version="${1:-}"
publish_flag="${2:-}"
publish_version="${3:-}"
if [ -z "$version" ] || [ "$publish_flag" != "--publish" ] || [ "$publish_version" != "$version" ]; then
  echo "usage: local-release-deploy <version> --publish <version>" >&2
  echo "refusing to publish without an explicit matching confirmation" >&2
  exit 2
fi

repo="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$repo"

missing=()
warnings=()
published=()
skipped=()
failed=()
repo_secret_names=""
repo_variable_names=""

. "$repo/scripts/release-local-env.sh"

record_published() {
  published+=("$1")
  printf 'published: %s\n' "$1"
}

record_skipped() {
  skipped+=("$1")
  printf 'skipped: %s\n' "$1" >&2
}

run_publisher() {
  local name="$1"
  shift
  local skipped_before="${#skipped[@]}"
  if "$@"; then
    if [ "${#skipped[@]}" -eq "$skipped_before" ]; then
      record_published "$name"
    fi
  else
    failed+=("$name")
    return 1
  fi
}

is_prerelease() {
  [[ "$version" =~ - ]]
}

write_release_env() {
  cat > release-env <<EOF
VERSION='$version'
IS_PRERELEASE='$(if is_prerelease; then printf true; else printf false; fi)'
EOF
}

require_tool_for_publish() {
  local destination="$1"
  local tool="$2"
  if command -v "$tool" >/dev/null 2>&1; then
    return 0
  fi
  record_skipped "$destination missing tool $tool"
  return 1
}

artifact_sha() {
  local artifact="$1"
  awk -v a="$artifact" '$2 == a { print $1 }' release/SHA256SUMS.txt
}

have_artifact() {
  local path="$1"
  [ -s "$path" ]
}

build_non_macos_artifacts() {
  write_release_env
  export VERSION="$version"

  cargo deny check -D vulnerability -W unmaintained advisories bans sources licenses

  mkdir -p srpms release
  shopt -s nullglob
  local existing_srpms=(srpms/*.src.rpm)
  shopt -u nullglob
  if [ "${#existing_srpms[@]}" -eq 0 ]; then
    git archive --format=tar.gz --prefix=rs-modde/ -o "rs-modde-${version}.tar.gz" HEAD
    tmp_vendor="$(mktemp -d)"
    trap 'rm -rf "$tmp_vendor"' RETURN
    tar xf "rs-modde-${version}.tar.gz" -C "$tmp_vendor"
    (cd "$tmp_vendor/rs-modde" && cargo vendor vendor > "$repo/cargo-vendor-config.toml")
    tar czf vendor.tar.gz -C "$tmp_vendor/rs-modde" vendor
    local spec_dir spec
    spec_dir="$(mktemp -d)"
    spec="${spec_dir}/modde.spec"
    trap 'rm -rf "$spec_dir"; rm -rf "$tmp_vendor"' RETURN
    sed "0,/^Version:.*$/s//Version:        ${version}/" modde.spec > "$spec"
    rpmbuild -bs "$spec" --define "_sourcedir $(pwd)" --define "_srcrpmdir $(pwd)/srpms"
  else
    printf 'already-current: SRPM artifact exists\n'
  fi

  if ! have_artifact release/THIRD_PARTY_LICENSES.html; then
    cargo about generate --output-file release/THIRD_PARTY_LICENSES.html about-template.hbs
  fi
  if ! have_artifact "release/modde-${version}.cdx.json"; then
    cargo sbom --output-format cyclone_dx_json_1_5 > "release/modde-${version}.cdx.json"
  fi
  if ! have_artifact "release/modde-${version}.spdx.json"; then
    cargo sbom --output-format spdx_json_2_3 > "release/modde-${version}.spdx.json"
  fi

  copy_nix_binary() {
    local result_dir="$1" binary="$2" destination="$3"
    if [ -f "${result_dir}/bin/.${binary}-wrapped" ]; then
      cp "${result_dir}/bin/.${binary}-wrapped" "$destination"
    else
      cp "${result_dir}/bin/${binary}" "$destination"
    fi
  }

  if ! have_artifact "release/modde-${version}-x86_64-linux.tar.gz"; then
    nix build .#modde --out-link linux-result
    mkdir -p release/linux-x86_64
    copy_nix_binary linux-result modde release/linux-x86_64/modde
    copy_nix_binary linux-result modde-ui release/linux-x86_64/modde-ui
    tar czf "release/modde-${version}-x86_64-linux.tar.gz" -C release/linux-x86_64 modde modde-ui
    cp release/linux-x86_64/modde "release/modde-${version}-x86_64-linux"
    cp release/linux-x86_64/modde-ui "release/modde-ui-${version}-x86_64-linux"
  else
    printf 'already-current: x86_64 Linux artifacts exist\n'
  fi

  if [ "${MODDE_LOCAL_DEPLOY_SKIP_AARCH64:-0}" = "1" ]; then
    record_skipped "aarch64 Linux artifact build skipped by MODDE_LOCAL_DEPLOY_SKIP_AARCH64=1"
  elif ! have_artifact "release/modde-${version}-aarch64-linux.tar.gz"; then
    nix build .#modde-aarch64-linux --out-link aarch64-linux-result
    mkdir -p release/linux-aarch64
    copy_nix_binary aarch64-linux-result modde release/linux-aarch64/modde
    copy_nix_binary aarch64-linux-result modde-ui release/linux-aarch64/modde-ui
    tar czf "release/modde-${version}-aarch64-linux.tar.gz" -C release/linux-aarch64 modde modde-ui
    cp release/linux-aarch64/modde "release/modde-${version}-aarch64-linux"
    cp release/linux-aarch64/modde-ui "release/modde-ui-${version}-aarch64-linux"
  else
    printf 'already-current: aarch64 Linux artifacts exist\n'
  fi

  if ! have_artifact "release/modde-${version}-x86_64-windows.zip"; then
    nix build .#modde-windows --out-link windows-result
    mkdir -p release/windows-x86_64
    cp windows-result/bin/modde.exe windows-result/bin/modde-ui.exe release/windows-x86_64/
    if [ -f windows-result/bin/libmcfgthread-2.dll ]; then
      cp windows-result/bin/libmcfgthread-2.dll release/windows-x86_64/
    fi
  else
    printf 'already-current: Windows archive exists\n'
  fi

  if ! have_artifact "release/modde-ui-${version}-x86_64.AppImage"; then
    nix build .#appimage-ui --out-link appimage-ui-result
    cp appimage-ui-result "release/modde-ui-${version}-x86_64.AppImage"
  fi
  if ! have_artifact "release/modde-${version}-x86_64.AppImage"; then
    nix build .#appimage-cli --out-link appimage-cli-result
    cp appimage-cli-result "release/modde-${version}-x86_64.AppImage"
  fi

  if ! have_artifact "release/rs-modde-${version}.tar.gz"; then
    git archive --format=tar.gz --prefix=rs-modde/ -o "release/rs-modde-${version}.tar.gz" HEAD
  fi
  if ! have_artifact release/com.tartanoglu.modde.json; then
    source_sha256="$(sha256sum "release/rs-modde-${version}.tar.gz" | awk '{print $1}')"
    nix build .#flatpak-manifest --out-link flatpak-result
    cp flatpak-result release/com.tartanoglu.modde.json
    sed -i "s/@SOURCE_TARBALL_SHA256@/${source_sha256}/" release/com.tartanoglu.modde.json
  fi
  if ! have_artifact release/cargo-sources.json; then
    nix run .#flatpak-cargo-generator -- Cargo.lock -o release/cargo-sources.json
  fi
  cp srpms/*.src.rpm release/ 2>/dev/null || true

  if ! have_artifact "release/modde_${version}_amd64.deb" || ! have_artifact "release/modde-ui_${version}_amd64.deb"; then
    if ! bash scripts/build-deb.sh "$version" release; then
      record_skipped "deb artifacts unavailable; APT publish will be skipped unless existing .deb artifacts are present"
    fi
  else
    printf 'already-current: Debian artifacts exist\n'
  fi

  if [ ! -d release/windows-x86_64 ]; then
    record_skipped "Windows artifacts absent; Chocolatey/Scoop/Winget will be skipped"
  elif [ -n "${WINDOWS_SIGNING_PFX:-}" ] && [ -n "${WINDOWS_SIGNING_PASS:-}" ] && [ ! -s "release/modde-${version}-x86_64-windows.zip" ]; then
    pfx_file="$(mktemp)"
    pass_file="$(mktemp)"
    trap 'rm -f "$pfx_file" "$pass_file"' RETURN
    printf '%s' "$WINDOWS_SIGNING_PFX" | base64 --decode > "$pfx_file"
    printf '%s' "$WINDOWS_SIGNING_PASS" > "$pass_file"
    for exe in release/windows-x86_64/modde.exe release/windows-x86_64/modde-ui.exe; do
      osslsigncode sign -pkcs12 "$pfx_file" -readpass "$pass_file" -h sha256 \
        -n 'modde' -i 'https://modde.tartanoglu.com' -ts 'http://timestamp.digicert.com' \
        -in "$exe" -out "${exe}.signed"
      osslsigncode verify -in "${exe}.signed"
      mv "${exe}.signed" "$exe"
    done
  elif [ ! -s "release/modde-${version}-x86_64-windows.zip" ]; then
    record_skipped "windows authenticode signing credentials absent; publishing unsigned Windows artifacts"
  fi
  if [ -d release/windows-x86_64 ] && [ ! -s "release/modde-${version}-x86_64-windows.zip" ]; then
    for exe in release/windows-x86_64/modde.exe release/windows-x86_64/modde-ui.exe; do
      test -s "$exe"
    done
    tar czf "release/modde-${version}-x86_64-windows.tar.gz" -C release/windows-x86_64 modde.exe modde-ui.exe
    zip_out="$PWD/release/modde-${version}-x86_64-windows.zip"
    (cd release/windows-x86_64 && zip -q "$zip_out" modde.exe modde-ui.exe)
    cp release/windows-x86_64/modde.exe "release/modde-${version}-x86_64-windows.exe"
    cp release/windows-x86_64/modde-ui.exe "release/modde-ui-${version}-x86_64-windows.exe"
  fi

  (
    cd release
    shopt -s nullglob
    : > SHA256SUMS.txt
    for file in *.tar.gz *.zip *.AppImage *.deb *.src.rpm *.cdx.json *.spdx.json; do
      [ "$file" != SHA256SUMS.txt ] || continue
      sha256sum "$file" >> SHA256SUMS.txt
    done
    test -s SHA256SUMS.txt
  )

  test -s keys/minisign.pub
  test -n "${MINISIGN_SECRET_KEY:-}"
  test -n "${MINISIGN_PASSWORD:-}"
  umask 077
  minisign_key="$(mktemp)"
  trap 'rm -f "$minisign_key"' RETURN
  printf '%s' "$MINISIGN_SECRET_KEY" > "$minisign_key"
  printf '%s\n' "$MINISIGN_PASSWORD" | minisign -S -s "$minisign_key" -m release/SHA256SUMS.txt -x release/SHA256SUMS.txt.minisig
  minisign -V -m release/SHA256SUMS.txt -x release/SHA256SUMS.txt.minisig -p keys/minisign.pub

  if [ -n "${COSIGN_PRIVATE_KEY:-}" ]; then
    cosign_key="$(mktemp)"
    trap 'rm -f "$cosign_key"' RETURN
    printf '%s' "$COSIGN_PRIVATE_KEY" > "$cosign_key"
    shopt -s nullglob
    for file in release/*.tar.gz release/*.zip release/*.AppImage release/*.deb release/*.src.rpm release/*.exe; do
      cosign sign-blob --yes --key "$cosign_key" --bundle "${file}.cosign.bundle" "$file"
    done
  else
    record_skipped "cosign unavailable locally; minisign checksums are authoritative for local deploy"
  fi

  if [ -z "${FLATHUB_TOKEN:-}" ]; then
    MODDE_FLATPAK_MANIFEST_ONLY=1 run_non_macos_smoke
  else
    run_non_macos_smoke
  fi
}

run_non_macos_smoke() {
  local smoke_failed=0
  for script in scripts/smoke/smoke-*.sh; do
    case "$(basename "$script")" in
      smoke-darwin-tarball.sh) continue ;;
      smoke-srpm.sh)
        record_skipped "SRPM local Fedora rebuild skipped during deploy; COPR remote build validates the SRPM"
        continue
        ;;
    esac
    bash "$script" "$version" release || smoke_failed=1
  done
  return "$smoke_failed"
}

publish_codeberg_release() {
  require_local_secret_for_publish "Codeberg release" CODEBERG_TOKEN \
    'Codeberg fj auth store or canix runtime secret can_codeberg_token' \
    'fj -H codeberg.org auth status || jq '\''.hosts["codeberg.org"] | {type, name, token_present: (.token != null and .token != "")}'\'' "$HOME/.local/share/forgejo-cli/keys.json"' || return 0

  local api="${CODEBERG_API:-https://codeberg.org/api/v1}"
  local codeberg_repo="${CODEBERG_REPO:-caniko/rs-modde}"
  local payload status release_id asset_id file name
  payload="$(jq -n --arg tag "$version" --arg name "$version" --arg branch "trunk" --argjson prerelease "$(if is_prerelease; then printf true; else printf false; fi)" --rawfile body CHANGELOG.md '{tag_name: $tag, target_commitish: $branch, name: $name, body: $body, draft: false, prerelease: $prerelease}')"
  status="$(curl -sS -o release.json -w '%{http_code}' -H "Authorization: token ${CODEBERG_TOKEN}" -H 'Content-Type: application/json' -d "$payload" "${api}/repos/${codeberg_repo}/releases")"
  if [ "$status" = "409" ]; then
    curl -sS --fail -H "Authorization: token ${CODEBERG_TOKEN}" "${api}/repos/${codeberg_repo}/releases/tags/${version}" > release.json
  elif [ "$status" -lt 200 ] || [ "$status" -ge 300 ]; then
    cat release.json
    return 1
  fi
  release_id="$(jq -r '.id' release.json)"
  test "$release_id" != "null"
  shopt -s nullglob
  while IFS= read -r file; do
    [ -f "$file" ] || continue
    name="$(basename "$file")"
    asset_id="$(curl -sS --fail -H "Authorization: token ${CODEBERG_TOKEN}" "${api}/repos/${codeberg_repo}/releases/${release_id}/assets" | jq -r --arg name "$name" '.[] | select(.name == $name) | .id' | head -n 1)"
    if [ -n "$asset_id" ]; then
      curl -sS --fail -X DELETE -H "Authorization: token ${CODEBERG_TOKEN}" "${api}/repos/${codeberg_repo}/releases/${release_id}/assets/${asset_id}"
    fi
    curl -sS --fail -H "Authorization: token ${CODEBERG_TOKEN}" -H 'Content-Type: application/octet-stream' --data-binary "@${file}" "${api}/repos/${codeberg_repo}/releases/${release_id}/assets?name=${name}" > /dev/null
  done < <(printf '%s\n' release/* | LC_ALL=C sort -u)
}

publish_apt() {
  is_prerelease && { record_skipped "APT prerelease"; return 0; }
  require_local_secret_for_publish "APT" APT_REPO_GPG_KEY \
    '/data/nvme0/can/Projects/canix/age/secrets/modules/repos/apt/modde_apt_repo_gpg_key.age' \
    'export APT_REPO_GPG_KEY=...; gpg --show-keys --with-fingerprint <(printf %s "$APT_REPO_GPG_KEY")' || return 0
  require_local_secret_for_publish "APT" APT_REPO_SSH_KEY \
    '/data/nvme0/can/Projects/canix/age/secrets/modules/repos/apt/modde_apt_repo_ssh_key.age' \
    'ssh -i <(printf %s "$APT_REPO_SSH_KEY") -T git@codeberg.org' || return 0
  shopt -s nullglob
  local debs=(release/*.deb)
  shopt -u nullglob
  [ "${#debs[@]}" -gt 0 ] || { record_skipped "APT missing .deb artifacts"; return 0; }
  VERSION="$version" APT_REPO_BRANCH="${APT_REPO_BRANCH:-pages}" ./scripts/publish-apt.sh
}

publish_aur() {
  is_prerelease && { record_skipped "AUR prerelease"; return 0; }
  require_local_secret_for_publish "AUR" AUR_SSH_KEY \
    'canix runtime secret can_aur_ssh_key or exported AUR_SSH_KEY' \
    'ssh -i <(printf %s "$AUR_SSH_KEY") -T aur@aur.archlinux.org' || return 0
  require_tool_for_publish "AUR" makepkg || return 0
  local makepkg_conf source_sha bin_sha
  makepkg_conf="$(dirname "$(dirname "$(command -v makepkg)")")/etc/makepkg.conf"
  source_sha="$(artifact_sha "rs-modde-${version}.tar.gz")"
  bin_sha="$(artifact_sha "modde-${version}-x86_64-linux.tar.gz")"
  test -n "$source_sha"
  test -n "$bin_sha"
  install -d -m 700 "$HOME/.ssh"
  local aur_key="$HOME/.ssh/aur"
  printf '%s\n' "$AUR_SSH_KEY" > "$aur_key"
  chmod 600 "$aur_key"
  ssh-keyscan aur.archlinux.org >> "$HOME/.ssh/known_hosts" 2>/dev/null
  chmod 600 "$HOME/.ssh/known_hosts"
  export GIT_SSH_COMMAND="ssh -i $aur_key -o IdentitiesOnly=yes"
  publish_pkg() {
    local pkg="$1" repo_url="ssh://aur@aur.archlinux.org/${pkg}.git"
    rm -rf "aur-${pkg}"
    if ! git clone "$repo_url" "aur-${pkg}"; then
      mkdir "aur-${pkg}"
      git -C "aur-${pkg}" init
      git -C "aur-${pkg}" remote add origin "$repo_url"
    fi
    git -C "aur-${pkg}" checkout -B master
    cp "dist/aur/${pkg}/PKGBUILD" "aur-${pkg}/PKGBUILD"
    case "$pkg" in
      modde)
        sed -i -e "s/^pkgver=.*/pkgver=${version}/" -e 's/^pkgrel=.*/pkgrel=1/' "aur-${pkg}/PKGBUILD"
        awk -v sha="$source_sha" '/^sha256sums=/{print "sha256sums=(\047" sha "\047)"; next} {print}' "aur-${pkg}/PKGBUILD" > "aur-${pkg}/PKGBUILD.new"
        mv "aur-${pkg}/PKGBUILD.new" "aur-${pkg}/PKGBUILD"
        ;;
      modde-bin)
        sed -i -e "s/^pkgver=.*/pkgver=${version}/" -e 's/^pkgrel=.*/pkgrel=1/' "aur-${pkg}/PKGBUILD"
        awk -v b="$bin_sha" -v s="$source_sha" '/^sha256sums=/{print "sha256sums=(\047" b "\047"; print "            \047" s "\047)"; skip=1; next} skip && /^[[:space:]]*\047/{next} {skip=0; print}' "aur-${pkg}/PKGBUILD" > "aur-${pkg}/PKGBUILD.new"
        mv "aur-${pkg}/PKGBUILD.new" "aur-${pkg}/PKGBUILD"
        ;;
      modde-git) ;;
    esac
    (cd "aur-${pkg}" && makepkg --config "$makepkg_conf" --printsrcinfo > .SRCINFO)
    git -C "aur-${pkg}" add PKGBUILD .SRCINFO
    if git -C "aur-${pkg}" diff --cached --quiet; then
      printf 'already-current: AUR %s\n' "$pkg"
      return 0
    fi
    git -C "aur-${pkg}" -c user.email='release-bot@localhost' -c user.name='release bot' commit -m "$version"
    git -C "aur-${pkg}" push origin HEAD:master
  }
  for pkg in modde modde-bin modde-git; do publish_pkg "$pkg"; done
}

publish_copr() {
  require_local_secret_for_publish "COPR" COPR_LOGIN 'canix runtime secret can_coppr_login' 'test -n "$COPR_LOGIN"' || return 0
  require_local_secret_for_publish "COPR" COPR_USERNAME '/data/nvme0/can/Projects/canix/age/secrets/modules/repos/coppr/username' 'test -n "$COPR_USERNAME"' || return 0
  require_local_secret_for_publish "COPR" COPR_TOKEN 'canix runtime secret can_coppr_token' 'test -n "$COPR_TOKEN"' || return 0
  shopt -s nullglob
  local srpms=(srpms/*.src.rpm)
  shopt -u nullglob
  [ "${#srpms[@]}" -gt 0 ] || { record_skipped "COPR missing SRPM"; return 0; }
  local copr_config copr_name project chroot_args=()
  copr_config="$(mktemp -d)"
  cat > "$copr_config/copr" <<EOF
[copr-cli]
login = ${COPR_LOGIN}
username = ${COPR_USERNAME}
token = ${COPR_TOKEN}
copr_url = https://copr.fedorainfracloud.org
EOF
  chmod 600 "$copr_config/copr"
  copr_name="${COPR_PROJECT_NAME:-rs-modde}"
  is_prerelease && copr_name="${COPR_PROJECT_NAME:-rs-modde-testing}"
  project="${COPR_USERNAME}/${copr_name}"

  if ! XDG_CONFIG_HOME="$copr_config" nix run .#copr-cli -- get "$project" >/dev/null 2>&1; then
    local chroot
    for chroot in ${COPR_CHROOTS:-fedora-43-x86_64 fedora-42-x86_64 epel-10-x86_64}; do
      chroot_args+=(--chroot "$chroot")
    done
    XDG_CONFIG_HOME="$copr_config" nix run .#copr-cli -- create "$copr_name" \
      "${chroot_args[@]}" \
      --description 'modde release builds' \
      --instructions 'Install with: sudo dnf copr enable caniko/rs-modde && sudo dnf install modde modde-ui'
  fi

  XDG_CONFIG_HOME="$copr_config" nix run .#copr-cli -- build --nowait "$project" srpms/*.src.rpm
  rm -rf "$copr_config"
}

publish_chocolatey() {
  is_prerelease && { record_skipped "Chocolatey prerelease"; return 0; }
  require_local_secret_for_publish "Chocolatey" CHOCOLATEY_API_KEY 'canix runtime secret can_choco_api_key' 'test -n "$CHOCOLATEY_API_KEY"' || return 0
  test -s "release/modde-${version}-x86_64-windows.zip" || { record_skipped "Chocolatey missing Windows zip"; return 0; }
  local choco_push_source
  if [ "${CHOCO_PUSH_SOURCE+x}" = x ]; then
    choco_push_source="$CHOCO_PUSH_SOURCE"
  else
    choco_push_source="https://push.chocolatey.org/"
  fi
  export CHOCO_PUSH_SOURCE="$choco_push_source"
  if chocolatey_version_exists "$version"; then
    printf 'already-current: Chocolatey modde %s\n' "$version"
    return 0
  fi
  if simit dist chocolatey bump \
      --version "$version" \
      --package-dir chocolatey-package \
      --archive "x64=release/modde-${version}-x86_64-windows.zip" \
      --choco-name modde \
      --choco-id modde \
      --choco-title modde \
      --choco-authors 'Can H. Tartanoglu' \
      --choco-description 'Cross-platform game mod manager' \
      --choco-project-url 'https://modde.tartanoglu.com' \
      --choco-download-repo 'caniko/rs-modde' \
      --choco-archive-pattern 'modde-{version}-{arch}-windows.zip' \
      --push \
      --push-source "$CHOCO_PUSH_SOURCE" \
      --api-key-env CHOCOLATEY_API_KEY; then
    return 0
  fi
  if chocolatey_version_exists "$version"; then
    printf 'already-current: Chocolatey modde %s\n' "$version"
    return 0
  fi
  return 1
}

chocolatey_version_exists() {
  local check_version="$1"
  local filter
  filter="$(printf "Id eq 'modde' and Version eq '%s'" "$check_version" | jq -sRr @uri)"
  curl -fsSL "https://community.chocolatey.org/api/v2/Packages()?%24filter=${filter}" | grep -q '<entry>'
}

publish_scoop() {
  is_prerelease && { record_skipped "Scoop prerelease"; return 0; }
  require_local_secret_for_publish "Scoop" SCOOP_BUCKET_TOKEN 'export SCOOP_BUCKET_TOKEN for codeberg.org/caniko/scoop-modde' 'test -n "$SCOOP_BUCKET_TOKEN"' || return 0
  local zip_name="modde-${version}-x86_64-windows.zip"
  test -s "release/${zip_name}" || { record_skipped "Scoop missing Windows zip"; return 0; }
  local sha256
  sha256="$(artifact_sha "$zip_name")"
  test -n "$sha256"
  local credential_helper='!f() { echo username=x-access-token; echo "password=$SCOOP_BUCKET_TOKEN"; }; f'
  rm -rf scoop-bucket
  git -c credential.helper="$credential_helper" clone "${SCOOP_BUCKET_URL:-https://codeberg.org/caniko/scoop-modde.git}" scoop-bucket
  (
    cd scoop-bucket
    git config credential.helper "$credential_helper"
    git config user.email 'release-bot@localhost'
    git config user.name 'release bot'
    git remote set-head origin -a
    default_branch="$(git symbolic-ref --short refs/remotes/origin/HEAD | sed 's|^origin/||')"
    git checkout "$default_branch"
    mkdir -p bucket
    cd ..
    sed -e "s|{{VERSION}}|${version}|g" -e "s|{{SHA256}}|${sha256}|g" dist/scoop/modde.json > scoop-bucket/bucket/modde.json
    cd scoop-bucket
    if [ -z "$(git status --porcelain -- bucket/modde.json)" ]; then
      printf 'already-current: Scoop bucket\n'
      exit 0
    fi
    git add bucket/modde.json
    git commit -m "modde ${version}"
    git push origin "HEAD:${default_branch}"
  )
}

publish_flathub() {
  is_prerelease && { record_skipped "Flathub prerelease"; return 0; }
  require_local_secret_for_publish "Flathub" FLATHUB_TOKEN 'export FLATHUB_TOKEN for github.com/flathub/com.tartanoglu.modde' 'test -n "$FLATHUB_TOKEN"' || return 0
  test -s release/com.tartanoglu.modde.json || { record_skipped "Flathub missing manifest"; return 0; }
  test -s release/cargo-sources.json || { record_skipped "Flathub missing cargo sources"; return 0; }
  local credential_helper='!f() { echo username=x-access-token; echo "password=$FLATHUB_TOKEN"; }; f'
  rm -rf flathub-repo
  git -c credential.helper="$credential_helper" clone https://github.com/flathub/com.tartanoglu.modde.git flathub-repo
  (
    cd flathub-repo
    git config credential.helper "$credential_helper"
    git config user.email 'release-bot@localhost'
    git config user.name 'release bot'
    git checkout -B "release/${version}"
    cp ../release/com.tartanoglu.modde.json ../release/cargo-sources.json .
    if [ -z "$(git status --porcelain -- com.tartanoglu.modde.json cargo-sources.json)" ]; then
      printf 'already-current: Flathub manifest\n'
      exit 0
    fi
    git add com.tartanoglu.modde.json cargo-sources.json
    git commit -m "$version"
    git push --force-with-lease origin "release/${version}"
  )
  local payload status
  payload="$(jq -n --arg title "$version" --arg head "release/${version}" --arg base "master" --arg body "Update to ${version}." '{title: $title, head: $head, base: $base, body: $body}')"
  status="$(curl -sS -o pr.json -w '%{http_code}' -H "Authorization: Bearer ${FLATHUB_TOKEN}" -H 'Accept: application/vnd.github+json' -d "$payload" https://api.github.com/repos/flathub/com.tartanoglu.modde/pulls)"
  if [ "$status" -eq 422 ]; then
    printf 'already-current: Flathub PR exists or rejected\n'
    return 0
  fi
  if [ "$status" -lt 200 ] || [ "$status" -ge 300 ]; then
    cat pr.json
    return 1
  fi
}

publish_winget() {
  is_prerelease && { record_skipped "Winget prerelease"; return 0; }
  require_local_secret_for_publish "Winget" WINGET_PAT 'export WINGET_PAT for github.com/microsoft/winget-pkgs' 'test -n "$WINGET_PAT"' || return 0
  local zip_name="modde-${version}-x86_64-windows.zip"
  test -s "release/${zip_name}" || { record_skipped "Winget missing Windows zip"; return 0; }
  local zip_url="https://codeberg.org/caniko/rs-modde/releases/download/${version}/${zip_name}"
  tmpdir="$(mktemp -d)"
  trap 'rm -rf "$tmpdir"' RETURN
  export WINEPREFIX="$tmpdir/wine" WINEDEBUG=-all TERM=xterm
  release_json="$(curl -fsSL https://api.github.com/repos/microsoft/winget-create/releases/latest)"
  url="$(printf '%s' "$release_json" | jq -r '.assets[] | select(.name == "wingetcreate.exe") | .browser_download_url' | head -n 1)"
  curl -fsSL -o "$tmpdir/wingetcreate.exe" "$url"
  wine "$tmpdir/wingetcreate.exe" update Caniko.Modde --version "$version" --urls "${zip_url}|x64" --token "$WINGET_PAT" --submit
}

print_summary() {
  printf '\nlocal release deploy summary for %s\n' "$version"
  printf 'published:\n'
  if [ "${#published[@]}" -eq 0 ]; then printf '  - none\n'; else printf '  - %s\n' "${published[@]}"; fi
  printf 'skipped:\n'
  if [ "${#skipped[@]}" -eq 0 ]; then printf '  - none\n'; else printf '  - %s\n' "${skipped[@]}"; fi
  printf 'failed:\n'
  if [ "${#failed[@]}" -eq 0 ]; then printf '  - none\n'; else printf '  - %s\n' "${failed[@]}"; fi
  printf 'policy:\n'
  printf '  - crates.io publish remains CI-only\n'
  printf '  - Homebrew is skipped while macOS artifacts are disabled\n'
}

nix run "$repo#local-check-release" -- "$version"
load_canix_release_inputs
build_non_macos_artifacts

run_publisher "Codeberg release" publish_codeberg_release
run_publisher "APT repository" publish_apt
run_publisher "AUR packages" publish_aur
run_publisher "COPR" publish_copr
run_publisher "Chocolatey" publish_chocolatey
run_publisher "Scoop" publish_scoop
run_publisher "Flathub" publish_flathub
run_publisher "Winget" publish_winget
record_skipped "Homebrew disabled while macOS artifacts are skipped"
record_skipped "crates.io publish remains CI-only"

print_summary
test "${#failed[@]}" -eq 0
