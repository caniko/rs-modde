# Phase 02 — SSH deploy key + canix wiring

> **Recommended Codex model: GPT 5.5 medium**
>
> A small ed25519 keygen plus a single canix `add-secret` invocation plus
> a Codeberg deploy-key registration. The mechanical steps are routine,
> but the secret-handling discipline (writing the private key to a
> 0600 path, surfacing only the public key, wiping plaintext after
> canix encryption) is exactly where a low-tier agent gets careless and
> commits a private key. Medium is the right cost.

## Working tree

Two repositories are touched:

- **canix repo**: `/data/nvme0/can/Projects/canix`. This is where the agenix-encrypted secret and its `forgejo-runner-secrets/*.nix` mapping land. Run `canix forgejo-runner add-secret` from a `nix develop` shell inside this checkout.
- **rs-modde repo**: `/data/nvme0/can/Projects/rs-modde`. **Read-only for this phase.** Only Phase 03 and Phase 04 modify rs-modde.

No throwaway local work outside `/tmp` should leave plaintext behind.

## Goal

A single new canix-managed secret `modde_apt_repo_ssh_key` is encrypted, registered against the `codeberg` Forgejo runner instance on `atlas`, and rekeyed for atlas. The corresponding ed25519 public key is installed as a write-enabled deploy key on `caniko/rs-modde-apt`. After this phase the runner has — but has not yet used — the SSH credential needed to push to the new apt repo.

## Why this matters now

The original Phase 04a draft used HTTPS+token push (`modde_apt_repo_ssh_key`). The maintainer's directive is SSH transport instead. Establishing the SSH key as a runner credential before any script or workflow change avoids a window where `publish-apt.sh` is rewritten to reference an SSH agent that has nothing in it.

## Out of scope

- Modifying `scripts/publish-apt.sh`. Phase 03.
- Modifying `.forgejo/workflows/release.yml`. Phase 04.
- Modifying any docs. Phase 04.
- Deleting or renaming the previously-planned HTTPS push-token secret in canix. That secret was never created (Phase 02 of the previous fill-the-gaps pass deliberately skipped it because the maintainer redirected to SSH), so there is nothing to delete. If a legacy push-token `.age` file *does* exist under `/data/nvme0/can/Projects/canix/age/secrets/hosts/atlas/foregejo-runner/`, stop and confirm with the maintainer before removing it; it implies a deviation from this plan's premise.
- Rotating the APT GPG signing key (`modde_apt_repo_gpg_key`).

## Plan

1. Generate an ed25519 keypair in `/tmp` (not in either repo):
   ```sh
   work="$(mktemp -d -t rs-modde-apt-ssh.XXXX)"
   chmod 700 "$work"
   nix shell nixpkgs#openssh -c ssh-keygen \
     -t ed25519 \
     -N '' \
     -C 'modde-release-bot@atlas (rs-modde-apt push)' \
     -f "$work/id_ed25519"
   ls -la "$work"
   ```
   No passphrase: the runner cannot answer a passphrase prompt. The key is encrypted at rest by agenix before it leaves `/tmp`.
2. Install the **public** half as a Codeberg deploy key on `caniko/rs-modde-apt`. In the Codeberg UI: Settings → Deploy Keys → Add Deploy Key. Title: `rs-modde release CI (atlas)`. Key: paste contents of `"$work/id_ed25519.pub"`. **Check "Enable Write Access".**
   - The deploy-key flow is per-repo and per-key; this key cannot push to any other repo even if leaked.
   - If the project ever needs key rotation, a new keypair is generated and the old deploy key entry is deleted from this same UI page.
3. Encrypt the **private** half into canix:
   ```sh
   cd /data/nvme0/can/Projects/canix
   nix develop -c canix forgejo-runner add-secret \
     -i codeberg \
     --slug modde-apt-repo-ssh-key \
     --credential-name modde-apt-repo-ssh-key \
     --from-file "$work/id_ed25519"
   ```
   Expected output ends with `agenix rekey -a` running successfully (it ran successfully for the three apt secrets that already live in canix; if it hangs again, follow the recovery in step 4).
4. **Hang recovery**: in the previous fill-the-gaps pass, `agenix rekey -a` hung on one of the apt secrets because the maintainer's age identity needed a YubiKey or passphrase prompt that the non-interactive shell could not satisfy. If `agenix rekey -a` hangs here:
   - Kill the canix invocation.
   - Re-run with `--no-rekey`.
   - Run `agenix rekey -a` interactively from a real terminal (so the YubiKey/age-identity prompt is visible) before deploying.
5. Deploy the new credential onto `atlas` so the runner instance actually has it loaded:
   ```sh
   cd /data/nvme0/can/Projects/canix
   canix deploy switch atlas
   ```
6. Verify the runner sees the credential by inspecting (read-only) the resolved Forgejo runner config on `atlas`:
   ```sh
   ssh atlas systemctl cat forgejo-runner-codeberg.service 2>&1 \
     | grep -E 'LoadCredential.*modde-apt-repo-ssh-key' \
     || echo 'Credential not yet visible in unit; canix deploy switch must have failed.'
   ```
   The grep should match if step 5 succeeded.
7. **Smoke** (read-only): from a workstation, prove the deploy key is accepted by Codeberg:
   ```sh
   work_clone="$(mktemp -d)"
   GIT_SSH_COMMAND="ssh -i $work/id_ed25519 -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new" \
     git ls-remote ssh://git@codeberg.org/caniko/rs-modde-apt.git pages
   ```
   Expect the seeded `pages` SHA from Phase 01. Any 403 / `Permission denied (publickey)` here means the deploy-key registration in step 2 didn't grant write or didn't land at all.
8. Wipe `/tmp` plaintext:
   ```sh
   shred -uz "$work/id_ed25519" "$work/id_ed25519.pub" 2>/dev/null || rm -f "$work/id_ed25519" "$work/id_ed25519.pub"
   rm -rf "$work" "$work_clone"
   ```
9. Commit the canix-side changes from a separate canix-repo working session (this rs-modde session does not commit canix):
   ```sh
   cd /data/nvme0/can/Projects/canix
   git add age/secrets/hosts/atlas/foregejo-runner/modde-apt-repo-ssh-key.age \
           root/hosts/atlas/server/forgejo-runner-secrets/modde-apt-repo-ssh-key.nix \
           root/hosts/atlas/server/forgejo-runner-secrets/default.nix
   git commit -m "atlas: add modde-apt-repo-ssh-key forgejo runner secret"
   ```

## Acceptance criteria

- [ ] `caniko/rs-modde-apt`'s Deploy Keys page lists one entry titled `rs-modde release CI (atlas)` with Write access enabled.
- [ ] `/data/nvme0/can/Projects/canix/age/secrets/hosts/atlas/foregejo-runner/modde-apt-repo-ssh-key.age` exists and is non-empty.
- [ ] `/data/nvme0/can/Projects/canix/root/hosts/atlas/server/forgejo-runner-secrets/modde-apt-repo-ssh-key.nix` imports `./lib.nix` with `credentialName = "modde-apt-repo-ssh-key"` and `instance = "codeberg"`.
- [ ] `default.nix` in the same directory imports the new file.
- [ ] After `canix deploy switch atlas`, the resolved `forgejo-runner-codeberg.service` unit on atlas declares a `LoadCredential` entry whose source path resolves the new secret (verified by Plan step 6's grep).
- [ ] From a workstation with the *generated* private key (now wiped from /tmp; this check is performed before wipe in Plan step 7), `git ls-remote ssh://git@codeberg.org/caniko/rs-modde-apt.git pages` succeeds.
- [ ] No `id_ed25519*` file remains under `/tmp` after Plan step 8.
- [ ] The canix-side commit message is `atlas: add modde-apt-repo-ssh-key forgejo runner secret` and is the **only** commit added to canix in this phase (no other secrets touched, no unrelated nix module changes).

## Files likely touched

- canix repo:
  - `age/secrets/hosts/atlas/foregejo-runner/modde-apt-repo-ssh-key.age` (new).
  - `root/hosts/atlas/server/forgejo-runner-secrets/modde-apt-repo-ssh-key.nix` (new).
  - `root/hosts/atlas/server/forgejo-runner-secrets/default.nix` (modified: one import line).
- rs-modde repo: none.
- Codeberg side: one deploy-key entry on `caniko/rs-modde-apt`.

## Pitfalls

- **Write access checkbox**: forgetting "Enable Write Access" on the Codeberg deploy-key form yields a key that authenticates but can only `git fetch`. CI will fail at push time, not registration time. Double-check the toggle.
- **Wrong identity in `git ls-remote` smoke**: a workstation that already has the maintainer's personal SSH key loaded into `ssh-agent` will pass the smoke even if the deploy key is misconfigured, because the agent silently tries every identity. The `IdentitiesOnly=yes` flag in Plan step 7 is required to prove the deploy key specifically works.
- **Passphrase on the keypair**: `ssh-keygen -N ''` is intentional. A passphrase-protected key cannot be unlocked by the runner.
- **Slug vs credential-name mismatch**: keep them identical (`modde-apt-repo-ssh-key`). The previous apt secrets follow the same convention; Phase 03 and 04 assume it.
- **Rekey hang loop**: if step 3's rekey hangs and `--no-rekey` is used, Phase 03 will appear to work locally but the deployed runner will fail to decrypt the secret at release time. Do not call this phase complete until `agenix rekey -a` has finished cleanly for this secret.
- **`forgetjo-runner` vs `foregejo-runner` typo**: canix's on-disk path is `foregejo-runner` (with the typo). That is the canonical canix path, not a mistake to fix in this plan. Match it exactly.

## Reference

- canix CLI source for this subcommand: `/data/nvme0/can/Projects/canix/cli/src/commands/forgejo_runner/add_secret.rs`.
- Existing apt secrets that follow the same pattern: `modde-apt-repo-gpg-key`, `modde-apt-repo-gpg-key-id` (under the same directory layout in canix).
- Codeberg deploy-key docs: <https://docs.codeberg.org/security/deploy-keys/>.
- Phase 01 (must be complete before this phase).
