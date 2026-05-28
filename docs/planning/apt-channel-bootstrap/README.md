# APT channel bootstrap (`caniko/rs-modde-apt`, SSH)

> **Recommended Codex model: GPT 5.5 medium**
>
> Orchestration is shallow (five phases, single repo plus one external
> Codeberg repo plus one canix wiring touch), but the SSH-push design call
> and the end-to-end Debian/Ubuntu smoke need a model that won't
> hand-wave the deploy-key edge cases. Medium is the cheapest tier that
> holds the bar.

## Scope

Land the missing pieces of Phase 04a (`docs/planning/release-integration-audit/04-linux-channels/deb-and-apt.md`) so the parent verify pass marks Phase 04a `passed` and the release-integration-audit plan can retire.

The pieces already in place from the previous fill-the-gaps pass:

- `scripts/publish-apt.sh` — reprepro driver (currently HTTPS+token; this plan rewrites it for SSH).
- `dist/apt/conf/distributions` — reprepro repo config.
- `dist/apt/key.gpg.asc` — APT repo public signing key (fingerprint `CCFE4A8461DF8778F5227684B6DB8F177A951E1B`).
- `.forgejo/workflows/release.yml` — `Publish APT repository` step (rewired to `secrets.modde_apt_repo_ssh_key`).
- canix-stored secrets for the three apt-key bits (key, key-id, optional passphrase).

What this plan adds:

1. The destination repo `caniko/rs-modde-apt` on Codeberg, with Pages serving turned on for a stable URL.
2. An SSH deploy key for the runner to push to that repo (replaces the planned HTTPS+token).
3. A rewrite of `publish-apt.sh` to push via `ssh://git@codeberg.org/caniko/rs-modde-apt.git`.
4. Workflow + docs updates so every reference matches the SSH model.
5. An end-to-end smoke that publishes a dry-run apt tree and proves `apt install modde` works on a Debian/Ubuntu container.

## Current state summary

- The previously-leaked HTTPS-token design is documented but not used. No legacy push-token canix secret exists.
- The repo `caniko/rs-modde-apt` does **not** exist on Codeberg yet (verified by the maintainer asking for guidance on the push token).
- The `Publish APT repository` workflow step skips silently today because every apt-related secret is unset.

## Phase table

| Phase | File | Layout | Model | Direct deps | Blocking on |
|---|---|---|---|---|---|
| 01 | [01-codeberg-pages-bootstrap.md](./01-codeberg-pages-bootstrap.md) | flat | 5.5 medium | — | — |
| 02 | [02-ssh-deploy-key-canix-wiring.md](./02-ssh-deploy-key-canix-wiring.md) | flat | 5.5 medium | 01 | 01 |
| 03 | [03-publish-script-ssh-rewrite.md](./03-publish-script-ssh-rewrite.md) | flat | 5.5 medium | 02 | 02 |
| 04 | [04-workflow-and-docs-rewire.md](./04-workflow-and-docs-rewire.md) | flat | 5.5 low | 02 | 02 |
| 05 | [05-end-to-end-smoke-and-verify.md](./05-end-to-end-smoke-and-verify.md) | flat | 5.5 high | 03, 04 | 03, 04 |

## Parallelism layer

- **Wave 0**: Phase 01 (Codeberg-side bootstrap). External repo work; nothing else can usefully run until the repo exists.
- **Wave 1**: Phase 02 (SSH key + canix). Needs the repo URL fixed by 01 because the SSH deploy key gets registered against `caniko/rs-modde-apt`.
- **Wave 2**: Phase 03 (script rewrite) and Phase 04 (workflow + docs rewire) run **in parallel**. They touch disjoint files (`scripts/publish-apt.sh` versus `.forgejo/workflows/release.yml` + `SECURITY.md` + `docs/site/...` + audit-report). The shared semantic is "the secret is now an SSH key", which is established by Phase 02.
- **Wave 3**: Phase 05 (smoke + verify). Sequential; needs both 03 and 04 green.

Plan exhaustion: after Phase 05 confirms a clean smoke and a re-run of the parent plan's verify reports Phase 04a passing.

## Whole-set acceptance criteria

- [ ] `caniko/rs-modde-apt` exists on Codeberg, serves a default-branch index via Codeberg Pages at a stable URL.
- [ ] `modde_apt_repo_ssh_key` is a canix-stored secret deployed onto the `codeberg` runner instance on `atlas`; the matching SSH public key is registered as a deploy key on `caniko/rs-modde-apt` with write access.
- [ ] `scripts/publish-apt.sh` pushes via `ssh://git@codeberg.org/caniko/rs-modde-apt.git` and contains zero references to HTTPS tokens.
- [ ] `.forgejo/workflows/release.yml`, `SECURITY.md`, `docs/site/content/docs/getting-started/installation.md`, `docs/planning/release-integration-audit/audit-report.md`, and `CONTRIBUTING.md` (where it touches apt) consistently describe the SSH-key model. No `APT_REPO_SSH_KEY` or `modde_apt_repo_ssh_key` references remain.
- [ ] A throwaway prerelease tag's release run skips apt publish (prerelease gate), AND a throwaway stable tag's release run pushes a signed `dists/stable/Release` + `pool/` tree to `caniko/rs-modde-apt` that resolves via the public URL.
- [ ] In a Debian 12 (bookworm) container, `apt update && apt install modde modde-ui` succeeds after pinning the key fingerprint from `dist/apt/key.gpg.asc`.
- [ ] Running `/multi-phase-plan-codex verify docs/planning/release-integration-audit` flips Phase 04a to **passed**.

## Global constraints

- The repository name is exactly `rs-modde-apt`. Anything addressing the earlier non-`rs-` owner/name pair is stale and must be corrected.
- Transport is SSH only. The deploy key lives in canix; the runner reads it via systemd `LoadCredential`. The plaintext SSH private key must never enter the rs-modde repo, the canix `root/` tree, or any chat transcript.
- The APT GPG signing key already lives in canix (`modde_apt_repo_gpg_key`, fingerprint `CCFE4A8461DF8778F5227684B6DB8F177A951E1B`); this plan does not rotate it.
- The public-facing URL choice (`https://caniko.codeberg.page/rs-modde-apt/` versus `https://caniko.codeberg.page/rs-modde-apt/`) is settled in Phase 01 and propagated to every doc by Phase 04.

## Reference

- Parent plan: [docs/planning/release-integration-audit/04-linux-channels/deb-and-apt.md](../release-integration-audit/04-linux-channels/deb-and-apt.md).
- Existing apt scaffolding in this repo: `scripts/publish-apt.sh`, `dist/apt/`, `.forgejo/workflows/release.yml` (`Publish APT repository` step), `SECURITY.md` ("APT Repository Signing Key").
- canix add-secret pattern: `canix forgejo-runner add-secret -i codeberg --slug <slug> --credential-name <name>` (see `/data/nvme0/can/Projects/canix/cli/src/commands/forgejo_runner/add_secret.rs`).

## Execution reminder

Run each phase yourself in a fresh Codex session. Prompt `verify docs/planning/apt-channel-bootstrap` when done to audit acceptance criteria, and `verify docs/planning/release-integration-audit` to flip the parent plan.
