# Phase 03 — DNS + Codeberg-Pages first deploy

> **Recommended Codex model: GPT 5.5 medium**
>
> Moderate complexity, leaf-node role, but the work touches external
> state (DNS provider UI, Codeberg repo secrets) the agent cannot
> manipulate directly — the phase is a runbook to walk the maintainer
> through. The agent's job is to verify each step via observable
> commands and to catch the subtle Codeberg-Pages quirks (TLS issuance
> lag, `.domains` verification TXT record). A smaller model is likely
> to hand-wave the verification gates.

## Working tree

`/data/nvme0/can/Projects/rs-modde` on `trunk`. Plus DNS provider
console + Codeberg web UI — those are out of band.

## Goal

`https://modde.rs/` returns the rendered website, valid TLS issued by
Codeberg-Pages, with `https://modde.rs/docs/` serving the AdiDoks docs
underneath. The Forgejo Actions `pages.yml` workflow runs on push to
`trunk` and updates the `pages` branch without manual intervention.

## Why this matters now

[.forgejo/workflows/pages.yml](/data/nvme0/can/Projects/rs-modde/.forgejo/workflows/pages.yml)
already runs on push to `trunk` and calls `nix run .#deploy-pages`,
which orphan-branches the `#site` derivation onto `pages` and
force-pushes. Codeberg-Pages serves whatever lives on the `pages`
branch, honoring the `.domains` file at
[flake.nix:181](/data/nvme0/can/Projects/rs-modde/flake.nix#L181). What
is missing today:

1. The DNS records for `modde.rs` and `www.modde.rs` pointing at
   Codeberg-Pages (`6.tcp.eu.ngrok.io`-style CNAME, or whatever the
   current Codeberg-Pages docs prescribe).
2. The Codeberg TXT verification record proving `caniko` owns
   `modde.rs`.
3. The `codeberg_token` repo secret that the workflow expects at
   [pages.yml:28](/data/nvme0/can/Projects/rs-modde/.forgejo/workflows/pages.yml#L28).

Until those land, the workflow either errors on missing-secret or
publishes a `pages` branch that Codeberg refuses to serve under the
custom domain.

## Out of scope

- Editing the deploy script or the workflow yml (those are correct
  today and may be replaced by the canix-toolbelt helper in Phase 06).
- Standing up the APT subdomain (`apt.modde.rs` or similar) — that's
  the APT bootstrap plan's job.
- Custom TLS certificates — Codeberg-Pages auto-provisions Let's
  Encrypt.

## Plan

1. Confirm Codeberg-Pages docs are current. Read
   `https://docs.codeberg.org/codeberg-pages/custom-domain/` and note
   the verification TXT record format expected. Record the exact format
   in the commit message of any doc updates this phase produces.
2. In the DNS provider's console for `modde.rs`:
   - Add an apex `ALIAS`/`ANAME` (or A records) per Codeberg's
     current advice for the apex.
   - Add `CNAME www → caniko.codeberg.page.`.
   - Add the Codeberg TXT verification record on `_codeberg-pages-verification.modde.rs`
     (exact name per step 1).
3. Verify DNS propagation:
   - `dig +short modde.rs` returns a Codeberg-Pages IP.
   - `dig +short CNAME www.modde.rs` returns `caniko.codeberg.page.`.
   - `dig +short TXT _codeberg-pages-verification.modde.rs` returns the
     verification token.
4. In the Codeberg web UI for `caniko/rs-modde`:
   - Create a Personal Access Token with `repo:write` scope (or the
     minimum scope that allows pushing the `pages` branch).
   - Store it as repo secret `codeberg_token`.
   - On the repo's Pages settings page, add `modde.rs` and
     `www.modde.rs` as custom domains so Codeberg requests TLS for
     them.
5. Trigger a deploy. Either push an empty commit on `trunk` (`git
   commit --allow-empty -m "Trigger pages deploy"`) or re-run the
   latest pages workflow from the Codeberg Actions UI. Watch the run.
6. Verify the result:
   - `curl -fsSI https://modde.rs/ | head -5` returns 200 from Codeberg.
   - `curl -fsSI https://www.modde.rs/ | head -5` returns 200 or a 301
     redirect to apex.
   - `curl -fsSI https://modde.rs/docs/ | head -5` returns 200.
   - `openssl s_client -connect modde.rs:443 -servername modde.rs
     </dev/null 2>/dev/null | openssl x509 -noout -issuer` reports
     Let's Encrypt.
7. No commits in rs-modde required unless step 1 surfaces a doc that
   must be updated.

## Acceptance criteria

- [ ] `dig +short modde.rs` resolves and matches Codeberg-Pages docs.
- [ ] `dig +short TXT _codeberg-pages-verification.modde.rs` is set.
- [ ] Latest run of `.forgejo/workflows/pages.yml` on `trunk` finished
      green.
- [ ] `curl -fsS https://modde.rs/ | head -20` includes the modde
      navigation HTML.
- [ ] `curl -fsS https://modde.rs/docs/` returns 200 and renders the
      AdiDoks chrome.
- [ ] TLS issuer on `modde.rs:443` is Let's Encrypt.

## Files likely touched

- None in-tree, unless step 1 discovers a stale doc reference that
  must be patched.

## Pitfalls

- **TLS provisioning lag.** Codeberg-Pages can take up to ~10 minutes
  after first verification to issue a cert; a `curl` immediately after
  DNS propagation may fail with TLS errors. Wait and re-check before
  rolling back DNS.
- **`.domains` file ordering.** Codeberg uses the *first* line of
  `.domains` as the canonical host. The flake currently writes
  `modde.rs` first, `www.modde.rs` second — confirm that's still the
  desired canonical and that `www.modde.rs` 301s to apex (Codeberg's
  default behavior when both are listed).
- **Secret leakage in logs.** The workflow embeds `$CODEBERG_TOKEN` in
  the remote URL at
  [pages.yml:33](/data/nvme0/can/Projects/rs-modde/.forgejo/workflows/pages.yml#L33).
  Forgejo masks secrets in logs by default, but verify the run's log
  view actually shows `***`, not the raw token. If the token leaks,
  rotate it before completing the phase.
- **DNS provider TXT-record splitting.** Some providers split long TXT
  values across multiple strings automatically; if the verification
  fails with the value visibly correct, try entering the value as a
  single quoted string.

## Reference

- Research dossier: [../website-wiring-research.md](../website-wiring-research.md) —
  "Blockers And Missing Artifacts" rows on DNS and `codeberg_token`.
- [.forgejo/workflows/pages.yml](/data/nvme0/can/Projects/rs-modde/.forgejo/workflows/pages.yml).
- [scripts/deploy-pages.sh](/data/nvme0/can/Projects/rs-modde/scripts/deploy-pages.sh).
- Codeberg-Pages custom domain docs (read at phase start to capture
  current syntax).
