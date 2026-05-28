# Phase 04 — Workflow + docs rewire to the SSH model

> **Recommended Codex model: GPT 5.5 low**
>
> Pure mechanical edits across one workflow file and four documentation
> files: rename a secret, delete obsolete references, and propagate the
> URL choice from Phase 01. No design content, no log reading, no
> non-trivial reasoning. Low-tier is correct; anything higher is waste.

## Working tree

`/data/nvme0/can/Projects/rs-modde`. Files touched:

- `.forgejo/workflows/release.yml`.
- `SECURITY.md`.
- `docs/site/content/docs/getting-started/installation.md`.
- `docs/planning/release-integration-audit/audit-report.md`.
- `CONTRIBUTING.md` (only if it mentions apt secrets; double-check before editing).

Disjoint with Phase 03 (which touches only `scripts/publish-apt.sh`), so this phase and Phase 03 are safe to run in parallel.

## Goal

Every reference to the previous HTTPS-token apt push pattern is replaced by SSH-key terminology. The URL choice recorded by Phase 01 is reflected in the user-facing install docs and the SECURITY.md verification block. The release workflow's `Publish APT repository` step feeds `secrets.modde_apt_repo_ssh_key` into the script's `APT_REPO_SSH_KEY` env var.

## Why this matters now

After Phase 03 lands the SSH-only script, the workflow must pass the SSH-key secret into `APT_REPO_SSH_KEY`. Until this phase lands, every stable tag silently skips apt publish, because the script's gating clause exits 0 when `APT_REPO_SSH_KEY` is unset. That is graceful but silently incorrect; users following the install docs would also still see an apt URL that may not resolve.

## Out of scope

- Editing `scripts/publish-apt.sh`. Phase 03.
- Modifying canix-side files. Phase 02.
- Touching the minisign or Authenticode secret references. Out of scope; this plan is apt-only.
- Adding new install docs sections for non-Debian distributions.
- Re-running the parent plan's verify. Phase 05 owns the final verify run.

## Plan

1. **Workflow secret rename.** In `.forgejo/workflows/release.yml`, within the `Publish APT repository` step's `env:` block:
   - Ensure `APT_REPO_SSH_KEY: ${{ secrets.modde_apt_repo_ssh_key }}` is present.
2. **Workflow header comment.** Update the workflow-level "Required secrets" comment block (top of file) so it lists `modde_apt_repo_ssh_key` and removes any line referencing `modde_apt_repo_ssh_key`. The bullet should read:
   - `# - modde_apt_repo_ssh_key: ed25519 private key the runner uses to push the rebuilt apt tree to ssh://git@codeberg.org/caniko/rs-modde-apt.git. Public key is registered as a Codeberg deploy key with write access.`
3. **Inline comment in the apt step.** Update the per-step "Required secrets" comment block immediately above `- name: Publish APT repository` so it documents the SSH key instead of the token.
4. **`SECURITY.md` → "APT Repository Signing Key" section.** This section talks about the signing key (GPG), not the push transport, so most of it remains correct. Edit only the rotation procedure and the inline `keys[]` reference:
   - Rotation procedure: replace "replace the Forgejo `APT_REPO_GPG_KEY` ..." line and the surrounding paragraph to additionally instruct rotating `modde_apt_repo_ssh_key` only when *that* key is compromised (independent of the signing key).
   - Add a one-paragraph "APT repository push key" subsection (or extend the existing section) explaining that pushes use a per-repo SSH deploy key on `caniko/rs-modde-apt`, not a Codeberg access token. Reference Phase 02's deploy-key install steps so future maintainers know where to look.
5. **`docs/site/content/docs/getting-started/installation.md` → "Debian / Ubuntu (apt)" section.** Replace every occurrence of `https://caniko.codeberg.page/rs-modde-apt` with the URL chosen in Phase 01 (record from the README footer note). If choice A: `https://caniko.codeberg.page/rs-modde-apt`. If choice B: `https://modde.rs/apt`, with a one-line note above the install snippet explaining that the canonical host serves or proxies the Codeberg Pages apt origin.
6. **`docs/planning/release-integration-audit/audit-report.md` → "Required Secrets" + "Phase 04a APT Repository State".**
   - In the Required Secrets table, ensure the SSH-key row exists. Description: `Push rebuilt apt tree to caniko/rs-modde-apt over SSH`. Behavior when missing: `Skips apt publish; tarball/AppImage release continues`. Notes: `Ed25519 private key; matching public key registered as a Codeberg deploy key on caniko/rs-modde-apt with write access`.
   - In "Phase 04a APT Repository State", replace the placeholder repo name `caniko/rs-modde-apt` (if it appears) with `caniko/rs-modde-apt`. Replace the HTTPS+token bootstrap step with the SSH+deploy-key bootstrap; reference Phase 01 and Phase 02 of this plan.
7. **`CONTRIBUTING.md`.** Grep for `modde-apt`, `APT_REPO`, `apt-repo`, or any leftover token references. The file's primary apt mention is in the Yank/withdraw drill where it links to the apt repo URL; update the URL to match Phase 01's choice.
8. **Static sweep.** From the rs-modde checkout root:
   ```sh
   ! rg -F 'APT_REPO_SSH_KEY' .
   ! rg -F 'modde_apt_repo_ssh_key' .
   ! rg -F 'caniko/rs-modde-apt' .
   rg -F 'modde_apt_repo_ssh_key' .forgejo/ docs/ SECURITY.md
   rg -F 'caniko/rs-modde-apt' scripts/ docs/ SECURITY.md
   ```
   The first three must report no matches (negated greps return non-zero on a match, so the `!` flips that into the success case for the search). The last two must show at least one match each.
9. **CI sanity.** Run `nix shell nixpkgs#yamllint -c yamllint -d 'extends: relaxed' .forgejo/workflows/release.yml` and confirm no errors. The relaxed config matches what this repo's CI uses.

## Acceptance criteria

- [ ] `.forgejo/workflows/release.yml`'s `Publish APT repository` step passes `APT_REPO_SSH_KEY` (sourced from `secrets.modde_apt_repo_ssh_key`) into the script's environment, and does not pass any legacy token env var.
- [ ] The workflow-level header `# - Required secrets:` block lists `modde_apt_repo_ssh_key` and contains no legacy push-token line.
- [ ] `SECURITY.md` documents the SSH push key alongside the existing APT signing-key block; the rotation paragraph treats the two keys as independent.
- [ ] `docs/site/content/docs/getting-started/installation.md`'s apt section URL matches Phase 01's recorded URL choice exactly. No `caniko.codeberg.page/rs-modde-apt` URLs survive in end-user install commands; choice B may mention the Codeberg Pages origin only in an explanatory note.
- [ ] `docs/planning/release-integration-audit/audit-report.md`'s Required Secrets table includes a `modde_apt_repo_ssh_key` row and no legacy push-token row.
- [ ] `docs/planning/release-integration-audit/audit-report.md`'s "Phase 04a APT Repository State" subsection names the repo `caniko/rs-modde-apt` and describes the SSH bootstrap.
- [ ] `CONTRIBUTING.md` contains no stale `caniko/rs-modde-apt` references.
- [ ] The static sweep in Plan step 8 passes.
- [ ] `yamllint -d 'extends: relaxed' .forgejo/workflows/release.yml` exits 0.

## Files likely touched

- `.forgejo/workflows/release.yml`
- `SECURITY.md`
- `docs/site/content/docs/getting-started/installation.md`
- `docs/planning/release-integration-audit/audit-report.md`
- `CONTRIBUTING.md` (only if grep hits stale apt references)

## Pitfalls

- **Stale `modde-apt` repo name leaking through**: the previous fill-the-gaps pass wrote `caniko/rs-modde-apt` (without `rs-`) in several places. A search-and-replace for `modde-apt` alone is too broad — it'll match `rs-modde-apt`. Use `caniko/rs-modde-apt` as the search anchor.
- **URL choice ambiguity**: if Phase 01's README footer note is missing or unclear, do not pick a URL silently in this phase. Stop, read the parent plan's README, and only then propagate.
- **Forgetting CONTRIBUTING.md**: the apt URL appears in the Yank/withdraw drill (added in the previous fill-the-gaps pass) and is easy to miss because it sits below the larger Hotfix section.
- **Renaming env-var spelling**: the *local* env var inside the workflow step and the script can stay uppercase (`APT_REPO_SSH_KEY`). Only the `${{ secrets.X }}` reference changes case (`modde_apt_repo_ssh_key`, the canix-credential form). Do not change the env-var spelling on the bash side, or Phase 03's script will not find what it expects.
- **yamllint version drift**: this repo's CI uses the `relaxed` profile. Running with `default` will report line-length warnings that the project intentionally disables (`# yamllint disable rule:line-length` at the top of `release.yml`).

## Reference

- Phase 01 (URL choice and repo name).
- Phase 02 (the secret slot name in canix).
- Phase 03 (the env-var contract this workflow must honor).
- Existing apt step in `.forgejo/workflows/release.yml` (search for `Publish APT repository`).
