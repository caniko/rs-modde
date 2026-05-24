# Phase 05a-bis — Scoop bucket

> **Recommended Codex model: GPT 5.5 medium**
>
> Create a `caniko/scoop-modde` bucket repo with a single `bucket/modde.json` manifest; CI on each tag updates the version + URL + hash and pushes. Trivially mechanical except for the choice of bucket strategy (own bucket vs. submit to `ScoopInstaller/Extras`); judgement worth `medium`, not more.

## Working tree
- New repo (external): `caniko/scoop-modde` on Codeberg or GitHub. Scoop reads from a Git remote — Codeberg works.
- `.forgejo/workflows/release.yml` — new "Publish Scoop" step.

## Goal
1. A Scoop bucket at `https://codeberg.org/caniko/scoop-modde` with `bucket/modde.json`.
2. Per-tag CI updates the manifest and pushes.
3. Users install via `scoop bucket add modde https://codeberg.org/caniko/scoop-modde && scoop install modde`.

## Why
Scoop is the de-facto Windows package manager for developers and power users (winget audience skews toward general users). The two channels are complementary, not redundant.

## Out of scope
- Submitting to `ScoopInstaller/Extras` (the official "Extras" bucket) — defer until the own bucket is stable.

## Plan
1. Create the `caniko/scoop-modde` repo on Codeberg manually (one-time).
2. Add Forgejo secret `SCOOP_BUCKET_TOKEN` (Codeberg access token scoped to the bucket repo).
3. Bucket layout:
   ```
   caniko/scoop-modde/
     bucket/
       modde.json
     README.md
   ```
4. `bucket/modde.json` template:
   ```json
   {
     "version": "{{VERSION}}",
     "description": "Cross-platform game mod manager",
     "homepage": "https://modde.tartanoglu.com",
     "license": "GPL-3.0-only",
     "url": "https://codeberg.org/caniko/rs-modde/releases/download/{{VERSION}}/modde-{{VERSION}}-x86_64-windows.zip",
     "hash": "{{SHA256}}",
     "bin": ["modde.exe", "modde-ui.exe"],
     "checkver": { "url": "https://codeberg.org/caniko/rs-modde/releases", "regex": "tag/([\\d.]+)" },
     "autoupdate": {
       "url": "https://codeberg.org/caniko/rs-modde/releases/download/$version/modde-$version-x86_64-windows.zip"
     }
   }
   ```
5. Release-workflow step:
   ```bash
   if [ -z "${SCOOP_BUCKET_TOKEN:-}" ]; then echo "skipping scoop"; exit 0; fi
   SHA256=$(grep "modde-${VERSION}-x86_64-windows.zip" release/SHA256SUMS.txt | awk '{print $1}')
   git clone "https://x-access-token:${SCOOP_BUCKET_TOKEN}@codeberg.org/caniko/scoop-modde.git" scoop-bucket
   sed -i \
     -e "s|{{VERSION}}|${VERSION}|g" \
     -e "s|{{SHA256}}|${SHA256}|g" \
     dist/scoop/modde.json > scoop-bucket/bucket/modde.json
   (cd scoop-bucket && git -c user.email=ci@modde.tartanoglu.com -c user.name='modde release bot' \
     commit -am "modde ${VERSION}" && git push)
   ```
6. Store the template at `dist/scoop/modde.json` in the main repo so it's reviewable in PRs.
7. Install docs: add Scoop section.

## Acceptance criteria
- [ ] `dist/scoop/modde.json` template exists in the main repo.
- [ ] After a release, `caniko/scoop-modde/bucket/modde.json` has the new version and matching SHA256.
- [ ] On a fresh Windows install: `scoop bucket add modde https://codeberg.org/caniko/scoop-modde && scoop install modde` succeeds and both `modde.exe` and `modde-ui.exe` are on PATH.
- [ ] Step no-ops gracefully when `SCOOP_BUCKET_TOKEN` is missing.

## Files likely touched
- `dist/scoop/modde.json` (template, new)
- `.forgejo/workflows/release.yml`
- `docs/site/content/docs/getting-started/installation.md`
- External: `caniko/scoop-modde` (new repo)

## Pitfalls
- Scoop's `checkver` URL must return HTML containing tags in the documented regex form; verify with `scoop checkver modde` from a test install.
- The `hash` field is matched against the *downloaded zip*, not the binaries inside — must use the zip's sha256, not the binary's.

## Reference
- Scoop bucket docs: https://github.com/ScoopInstaller/Scoop/wiki/Buckets
- Phase 05a — winget uses the same `.zip` artifact.
