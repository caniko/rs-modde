# Phase 04 — Audit & complete release-workflow secret reachability

> **Recommended Codex model: GPT 5.5 medium**
>
> Moderate-complexity sub-agent work spanning canix (agenix / runner-credential
> wiring) and Codeberg Actions-secret registration, where the deliverable is a
> precise mapping with a hard correctness bar: every channel must be cleanly
> "present → publishes" or "absent → soft-skips", never half-configured. The
> judgement (which exposure model per secret, what counts as a clean soft-skip)
> needs a capable model; the work itself is bounded inventory + wiring. `medium`;
> not `high` — no novel architecture, the exposure patterns already exist.

## Working tree

`/data/nvme0/can/Projects/canix` for runner-credential/agenix wiring; some
secrets are **Codeberg-UI-managed** Actions secrets (record which). Independent
of all other phases; runs in Wave 0. Use the `canix-cli` skill for
deploy/secret operations.

## Goal

Every secret the generated `release.yml` references is, at release time, either
reachable by the atlas job (so its channel **publishes**) or deliberately
absent (so its channel **cleanly soft-skips** with a logged reason) — with a
written mapping. No channel is in a half-configured state that would hard-fail
or silently no-op the real release.

## Why this matters now

The prerelease run (Phase 06) gates most channels on `IS_PRERELEASE=false`, so a
`-rc.N` tag exercises the build/sign/upload spine but **not** the downstream
pushes. The first time the downstream channels actually fire is the real release
(Phase 07) — an irreversible, public fan-out. A secret that's "registered on the
host but not mounted into the job container", or named differently than the
workflow expects, turns into either a hard failure mid-fan-out or a silent
no-op. This must be mapped and fixed **before** Phase 06, so Phase 07 has no
secret surprises.

The `release.yml` header enumerates the references (authoritative list):
`codeberg_token`, `MINISIGN_SECRET_KEY`/`MINISIGN_PASSWORD`,
`COSIGN_PRIVATE_KEY`/`COSIGN_PASSWORD` (optional), `modde_apt_repo_gpg_key`/
`_id`/`_passphrase`, `modde_apt_repo_ssh_key`, `AUR_SSH_KEY`,
`copr_login`/`copr_username`/`copr_token`, `WINDOWS_SIGNING_PFX`/`_PASS`
(optional), runner-env `CHOCOLATEY_API_KEY`, `FLATHUB_TOKEN` (optional),
`WINGET_PAT` (optional), `MASTODON_*`/`MATRIX_*` (optional), and the runner
credential `$ATTIC_TOKENS_DIR/rs-modde`.

## Out of scope

- Do **not** invent secret values. Only wire exposure of existing creds /
  register names. A genuinely missing secret is an external blocker to report
  (and a documented soft-skip), not something to fabricate.
- Do **not** change simit or rs-modde here.
- Do **not** broaden the runner's mounted secrets beyond what the release needs.
- Do **not** run a release; this only prepares secret reachability.

## Plan

1. **Enumerate references** from `release.yml` (the header list above plus any
   `${{ secrets.X }}` / runner-env reads in the body). Build a table: secret →
   how the workflow consumes it (`${{ secrets.X }}` Actions secret vs
   runner-provided env vs `$ATTIC_TOKENS_DIR/...` file).
2. **Inventory what exists** under
   `canix/root/modules/server/forgejo-runner-secrets/` and the atlas runner
   wiring (`forgejo-runners.nix`): `choco-api-key` (already wired as a
   runner-credential env, `CHOCOLATEY_API_KEY`), `codeberg-base-token`,
   `modde-apt-repo-*`, `modde-minisign-*`, the Attic token dir, etc. Map each to
   the reference it satisfies.
3. **Classify each reference** into one of:
   - **Runner-credential → job env** (mirror the Attic/choco pattern): mount +
     `-e` in `forgejo-runners.nix`. Confirm the env var name matches the
     workflow / `[chocolatey].api_key_env`.
   - **Codeberg Actions secret** (`${{ secrets.X }}`): must be registered on the
     repo/org with the exact name. This is **Codeberg-UI work** — record which
     secrets are UI-managed and confirm presence with the maintainer (agents
     cannot read secret values; confirm existence, not contents).
   - **Intentionally absent → soft-skip**: optional channels with no secret
     yet (e.g. `FLATHUB_TOKEN`, `WINGET_PAT`, `MASTODON_*`/`MATRIX_*`,
     `WINDOWS_SIGNING_PFX`). Confirm the workflow step guards on the secret's
     presence and logs a skip, not a hard failure.
4. **For any newly wired runner cred:** add its agenix module under
   `forgejo-runner-secrets/` following the existing `lib.nix secretMappings`
   pattern; `agenix rekey -a` as needed.
5. **Deploy** (`canix deploy switch atlas`) if anything changed; confirm the
   runner restarts cleanly and stays online (coordinate timing with Phase 03 —
   don't deploy while a release run is building). **Verify exposure without a
   real release:** inspect the running job container / a trivial probe job to
   confirm the intended env var/file is present (don't assume).
6. **Confirm soft-skip cleanliness:** for each intentionally-absent secret,
   read the corresponding `release.yml` step and verify it short-circuits with a
   logged "skipping …" rather than erroring. If a step would hard-fail on a
   missing optional secret, that's a generator bug → record it for Phase 05.

## Acceptance criteria

- [ ] A documented mapping exists: each `release.yml` secret reference → Actions
      secret name, or runner-exposed env var name, or "intentionally absent →
      soft-skip", with no reference unaccounted for.
- [ ] Every **must-publish** channel's secret (codeberg_token, minisign, apt
      gpg+ssh, AUR_SSH_KEY, copr_*, chocolatey runner env, Attic token) is
      confirmed reachable inside an atlas job — verified by inspection/probe, not
      assumed.
- [ ] Every **optional** channel with an absent secret has a confirmed clean
      soft-skip path in `release.yml` (logged skip, not hard failure); any step
      that would hard-fail is recorded as a Phase 05 generator fix.
- [ ] If canix changed: `canix deploy switch atlas` succeeded and
      `systemctl --failed` is empty with the runner online.

## Files likely touched

- `/data/nvme0/can/Projects/canix/root/hosts/atlas/server/forgejo-runners.nix`
  — mounts / `-e` for any newly exposed runner credential.
- `/data/nvme0/can/Projects/canix/root/modules/server/forgejo-runner-secrets/*.nix`
  (+ `lib.nix secretMappings`) — new agenix secret modules.
- Codeberg repo/org **Actions secrets** (UI) — registration is out-of-repo;
  record which names are UI-managed.

## Pitfalls

- **Credential on host but not in container.** systemd creds live at
  `/run/credentials/...` on the host; jobs run in containers. Symptom: env var
  empty in the job → channel soft-skips even though the secret "exists".
  Recovery: add the `-v` mount + `-e` like the Attic/choco token; verify inside
  the container.
- **Actions secret vs runner cred confusion.** `${{ secrets.X }}` resolves from
  Codeberg's Actions store, not from runner systemd creds. Symptom: a runner-only
  cred never appears as `secrets.X`. Recovery: register the Actions secret, or
  switch that channel to the runner-env model where simit supports it.
- **"Soft-skip" that's actually a hard fail.** A step that does
  `gpg --import "$KEY"` on an empty `$KEY` errors out. Verify the guard
  (`if [ -n "$X" ]`) exists; if not, it's a Phase 05 generator fix, not a secret
  to fabricate.
- **Disrupting CI by deploying mid-release.** Coordinate with Phase 03 — never
  `canix deploy switch atlas` while Phase 06/07 has a run building.

## Reference

- Reference list: `release.yml` header (`# Required secrets:` block) + body.
- Existing wiring: `canix/root/hosts/atlas/server/forgejo-runners.nix`
  (choco-api-key env, Attic token dir), `forgejo-runner-secrets/`.
- Skills: `canix-cli`, `atlas-runner`, `forgejo-atlas-ci`.
- Consumer: [06-prerelease-validation.md](./06-prerelease-validation.md);
  generator-fix bounce: [05-harden-simit-generator.md](./05-harden-simit-generator.md).
