# Phase 03 — Diagnose & mitigate the run-199 SIGTERM (runner teardown)

> **Recommended Codex model: GPT 5.5 medium**
>
> Host-forensics + light NixOS-ops work in a sub-agent/orchestrator role at
> moderate complexity: read the atlas journal/dmesg, correlate timestamps,
> decide transient-vs-config, and possibly make a small canix change. The
> judgement (distinguish a deploy restart from an OOM from a connection drop, and
> not over-fix a one-off) needs a capable model, but the surface is small and the
> evidence is concrete. `medium` effort; not `high` — no novel design, just
> careful correlation and a conservative mitigation.

## Working tree

`/data/nvme0/can/Projects/canix` for any config change; the diagnosis itself
runs **on the atlas host** (journal/dmesg), reached via the maintainer's normal
canix deploy/SSH path. Note: from the dev workstation `atlas` resolves to the
loopback alias `127.0.0.2` — the **real host is not reachable by that name from
the workstation**, so the journal must be read on atlas itself (use the
`atlas-runner` / `canix-cli` access path). Independent of all other phases
(different repo/concern); runs in Wave 0.

## Goal

The cause of run 199's termination is positively identified from atlas
journal/dmesg evidence, and either (a) a concrete mitigation is in place so a
release run won't be torn down mid-build again, or (b) it is confirmed a
one-off transient with a documented operational rule that prevents recurrence.

## Why this matters now

Run 199 was killed ~21.5 min in, ~6.5 min into the aarch64 cross-build:

```
2026-06-01T15:55:06Z ⚙️ [runner]: exitcode '143': failure
  …/podman.sock/…/archive?path=…/SUMMARY.md": context canceled
2026-06-01T15:55:07Z 🏁  Job failed
```

`exit 143 = SIGTERM`. The job log proves the kill was **external** (runner-layer
"context canceled" + podman teardown), not a build error, OOM-in-builder, or a
configured cap: there is no `timeout-minutes`, no act_runner `timeout` (default
3 h), and no systemd `RuntimeMaxSec`/`TimeoutStopSec`/`MemoryMax` on the runner
unit. The leading hypothesis is that a concurrent `canix deploy switch atlas`
(applying the Phase-05 secret wiring now present in `forgejo-runners.nix`)
restarted `forgejo-runner@nixTrusted.service`, SIGTERM-ing the in-flight
container. The secondary hypothesis is host OOM during the cold 8-target
cross-build (server profile: `max-jobs = 4`, `min-free = 5 GB`). A real release
(Phase 07) is ~1 h of cold building — if the trigger recurs, it kills the
release too. This must be settled before the prerelease run, not during it.

## Out of scope

- Do **not** broaden runner resources or rearchitect the runner; apply the
  smallest mitigation the evidence justifies.
- Do **not** touch rs-modde, simit, the windows build, or secrets (Phase 04).
- Do **not** deploy unrelated canix changes as part of this phase.

## Plan

1. **Read the journal around the kill** (on atlas), 15:50–16:00 UTC 2026-06-01:
   ```
   journalctl -u forgejo-runner@nixTrusted.service -S '2026-06-01 17:50' -U '2026-06-01 18:00'
   journalctl -u forgejo-runner@codeberg.service   -S '2026-06-01 17:50' -U '2026-06-01 18:00'
   journalctl -k -S '2026-06-01 17:50' -U '2026-06-01 18:00' | grep -iE 'oom|kill|memory'
   journalctl _COMM=systemd -S '2026-06-01 17:50' -U '2026-06-01 18:00' | grep -i forgejo
   ```
   Look for: a unit `Stopping/Stopped/Started` (= restart, i.e. a deploy),
   an `oom-kill`/`Out of memory` (= OOM), or nothing (= connection-side
   abandon). Also check `journalctl -u nixos-rebuild` / the system generation
   switch time (`nixos-rebuild list-generations` mtimes) to see if a deploy
   landed at ~17:55 local.
2. **Classify** the cause from the evidence into exactly one of:
   - **Deploy restart** (most likely): a generation switch / unit restart at
     ~17:55. → Mitigation: operational rule + optional guard.
   - **OOM**: kernel oom-killer near the build. → Mitigation: cap nix build
     parallelism for release-class builds or pre-warm the cache so less is built
     on-host (coordinate with Phase 02 step 5).
   - **Transient/connection**: no local restart or OOM; runner lost its server
     connection. → Likely one-off; document and move on.
3. **Apply the matching mitigation:**
   - Deploy restart → document the rule **"do not `canix deploy switch atlas`
     while a release run is building"** in the release runbook / Phase 07, and
     evaluate whether the runner unit should drain in-flight jobs on
     stop (only if cheaply done; do not destabilise other CI).
   - OOM → reduce `max-jobs`/`max-substitution-jobs` for the runner's nix usage,
     or rely on Phase 02 cache pre-warm to shrink on-host build volume; re-deploy
     and confirm the runner is back online.
   - Transient → no config change; record the finding.
4. **If any canix change was made:** `canix deploy switch atlas` (or the
   `canix-cli` equivalent), then confirm the runner is **online** and no systemd
   unit is failed (`systemctl --failed`; runner-online check from `atlas-runner`).
5. **Record** the verdict (cause + evidence excerpt + mitigation) for Phase 06/07
   to rely on.

## Acceptance criteria

- [x] The run-199 SIGTERM cause is stated with a journal/dmesg excerpt as
      evidence (deploy-restart / OOM / transient), not a guess.
- [x] If deploy-restart or OOM: a concrete mitigation is applied and verified
      (no canix redeploy required; runner online; unrelated failed units noted
      below); if transient: a one-line operational rule is documented.
- [x] The "do not deploy atlas during a release run" rule is written into the
      Phase 07 runbook regardless of cause (cheap insurance).
- [x] The atlas Forgejo runner is online and idle, ready for Phase 06.

## Result

**Verdict: deploy/reboot teardown.** Run 199 was killed by atlas entering a
system shutdown/reboot at the same instant as the Codeberg log's SIGTERM, not by
Nix, the builder, or an OOM condition.

Evidence from atlas journal, local time CEST:

```text
Jun 01 17:55:05 atlas systemd-logind[2309]: The system will reboot now!
Jun 01 17:55:06 atlas systemd[1]: Stopping Forgejo Actions Runner (nixTrusted)...
Jun 01 17:55:06 atlas forgejo-runner[3922736]: runner: received shutdown signal
Jun 01 17:55:06 atlas forgejo-runner[3922736]: runner: shutdown initiated, waiting [runner].shutdown_timeout=0s for running jobs to complete before shutting down
Jun 01 17:55:06 atlas forgejo-runner[3922736]: forcing the jobs to shutdown
Jun 01 17:55:06 atlas forgejo-runner[3922736]: [poller] shutdown begin, 1 tasks currently running
Jun 01 17:55:09 atlas forgejo-runner[3922736]: all jobs have been shutdown
Jun 01 17:55:09 atlas forgejo-runner[3922736]: runner: cancelled in progress jobs during shutdown
```

The sibling `forgejo-runner@codeberg.service` stopped in the same shutdown
transaction at 17:55:06, with 0 tasks running. The next boot began at
17:57:05, and both runners started and re-registered at 17:57:32. Kernel journal
for 17:50-18:00 had no `oom`, `kill`, or `memory` matches.

Mitigation: no canix resource or timeout change. This was an operational host
reboot/switch during a release-class build. The release rule is: **do not run
`canix deploy switch atlas`, reboot atlas, or otherwise restart
`forgejo-runner@*.service` while a prerelease or real release run is building.**
For required atlas maintenance, first confirm Codeberg has no active rs-modde
release job and the atlas runner has no active job containers.

Drain-on-stop was evaluated and deliberately not added here. The observed
`runner.shutdown_timeout=0s` explains the immediate cancellation, but increasing
it would make host switch/reboot behavior wait on arbitrary CI jobs and would
not protect an explicit host reboot. Avoiding atlas maintenance during release
runs is the smaller and more reliable mitigation.

Current verification on 2026-06-02: `forgejo-runner@codeberg.service` and
`forgejo-runner@nixTrusted.service` are active, both registered with Codeberg,
and `podman ps` shows no active Forgejo/act job containers. `systemctl --failed`
is not empty on atlas, but the failed units are unrelated to this phase
(`home-manager-can.service` and `infernix-download.service`); no Forgejo runner
unit is failed.

## Files likely touched

- `/data/nvme0/can/Projects/canix/root/hosts/atlas/server/forgejo-runners.nix`
  — only if a mitigation requires it (e.g. nix parallelism caps); otherwise none.
- `/data/nvme0/can/Projects/canix/root/modules/config/nix.nix` — only if an OOM
  mitigation caps `max-jobs`/`max-substitution-jobs`.

## Pitfalls

- **Over-fixing a one-off.** If the journal shows a clean deploy restart at the
  kill time, the cause is operational, not a config defect — adding timeouts or
  resource caps would be cargo-culting. Symptom: a `Stopped … Started` pair at
  17:55. Recovery: document the rule, change nothing.
- **Disrupting all CI with a runner edit.** A bad `forgejo-runners.nix` change
  breaks every Codeberg project on atlas. Symptom: runner offline / jobs fail to
  start post-deploy. Recovery: `canix deploy switch atlas` to the prior
  generation (or `nixos-rebuild --rollback` on the host); verify online before
  walking away.
- **Journal already rotated.** Retention is set to 30 days
  (`journald.extraConfig MaxRetentionSec=30day`), so 2026-06-01 should still be
  present in early June — but confirm; if rotated, rely on the system
  generation-switch timestamp as a proxy for the deploy hypothesis.
- **Confusing 143 with 137.** SIGTERM (143) ≠ SIGKILL (137). An OOM-killer hit
  is usually 137 on the *builder*; a 143 at the *runner* layer points to a
  graceful stop (restart) over OOM. Weigh the evidence accordingly.

## Reference

- Run 199 terminal log lines (basic-auth fetch per the plan README).
- Runner config: `canix/root/hosts/atlas/server/forgejo-runners.nix`; nix
  settings: `canix/root/modules/config/nix.nix`.
- Skills: `atlas-runner`, `canix-cli`, `berg-codeberg-ci`.
- Consumer (relies on a stable runner): [06-prerelease-validation.md](./06-prerelease-validation.md).
