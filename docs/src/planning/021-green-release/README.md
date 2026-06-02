# Plan: rs-modde 0.2.1 — fully green multi-channel release

> **Recommended Codex model for plan-set orchestration: GPT 5.5 high**
>
> Coordinating this set means holding three repos (rs-modde, canix, simit),
> a live multi-job Codeberg release that fans out to ~10 external publish
> targets, and an irreversible terminal phase in one head. That is complex
> orchestration with non-trivial sequencing and a release-burn cost on a wrong
> call — orchestrator role at complex complexity. Not `max`: the per-phase
> files carry the frontier risk (Phase 07 is routed `max` on its own), and the
> coordination itself is well-structured here.

## Scope and current state

Get **rs-modde 0.2.1** (or the next version if a re-cut is cleaner) to a
**green Codeberg release run** in which every configured channel either
publishes correctly or soft-skips cleanly. The packaging pipeline is already
simit-generated and functionally proven — release run **199** progressed
through `validate-tag → SBOM → x86_64 build → into the aarch64 cross-build`
before dying. The investigation of runs 198/199 found the real blockers:

1. **PRIMARY HARD BLOCKER — the `modde-windows` cross-build fails.** Vendored
   unrar does `#include <PowrProf.h>` (capital); mingw-w64 ships lowercase
   `powrprof.h`; case-sensitive nix can't find it. Run **198** (head `8a042f1`)
   proved it:
   ```
   vendor/unrar/os.hpp:54:10: fatal error: PowrProf.h: No such file or directory
   modde-windows-deps-0.2.1.drv … builder failed with exit code 101
   ```
   The fix attempt in `flake.nix:346-352` (`.mingw-case-headers/PowrProf.h`
   symlink + `CXXFLAGS_x86_64_pc_windows_gnu` in `preBuild`) is **present but
   ineffective** — the include flag isn't reaching `cc-rs` inside
   `craneLib.buildDepsOnly` (`flake.nix:357`). Build order is x86_64 → aarch64
   → windows, so run 199's SIGTERM (during aarch64) merely *masked* this; the
   run would have failed at windows regardless.

2. **Run 199's SIGTERM was atlas reboot/switch teardown.** Phase 03 confirmed
   this from atlas journal evidence: at 17:55:05 CEST systemd-logind announced a
   reboot, and at 17:55:06 `forgejo-runner@nixTrusted` received shutdown with 1
   task running and `runner.shutdown_timeout=0s`, then forced the job down. The
   kernel journal had no OOM evidence in the window. Mitigation: freeze atlas
   maintenance during prerelease/real release jobs — no `canix deploy switch
   atlas`, atlas reboot, or `forgejo-runner@*.service` restart while the release
   builds.

3. **Tag hygiene.** Tag `0.2.1` = commit `1e4a555` has **zero** of the windows
   fix and lacks all 6 trunk commits. `validate-tag` validates a detached
   worktree of the *tag* while later build steps run on the *job checkout*
   (HEAD) — so `workflow_dispatch` on trunk validated `1e4a555` but built
   `69e23df`. A real **tag-push** checks out the tag, so a pushed `0.2.1` would
   build `1e4a555` = **missing the windows fix**. The tag must be re-cut onto a
   commit containing the fixes.

4. **The `ignoring untrusted flake configuration 'extra-substituters'` warning
   is benign on atlas** (the trusted container is a client of the host
   nix-daemon; the macOS SDK is GC-pinned + bind-mounted; `harbor-macos-sdk` is
   not a substituter on atlas because atlas *produces* it). Do not chase it as a
   cache bug. Cold cache makes the full 8-target build ~1 h.

Tag operations are **human-in-the-loop**: the maintainer signs tags with a
GPG smartcard (key `818D…CFE1`, PIN via pinentry). No agent can sign.

## Phase table

| Phase | File | Depends on | Touches | Can parallel with | Blocking? |
|---|---|---|---|---|---|
| 01 | [01-fix-windows-cross-build.md](./01-fix-windows-cross-build.md) | — | rs-modde `flake.nix` | 03, 04, 05 | **Yes** (critical path) |
| 02 | [02-verify-all-cross-targets.md](./02-verify-all-cross-targets.md) | 01 | rs-modde (build only) | 03, 04, 05 | **Yes** (critical path) |
| 03 | [03-diagnose-runner-teardown.md](./03-diagnose-runner-teardown.md) | — | canix / atlas host | 01, 02, 04, 05 | Yes (gates 06) |
| 04 | [04-audit-release-secrets.md](./04-audit-release-secrets.md) | — | canix + Codeberg UI | 01, 02, 03, 05 | Yes (gates 06) |
| 05 | [05-harden-simit-generator.md](./05-harden-simit-generator.md) | — | simit, then rs-modde `release.yml` | 01, 03, 04 | **No (optional)** |
| 06 | [06-prerelease-validation.md](./06-prerelease-validation.md) | 01, 02, 03, 04 | rs-modde tags + CI | — | Yes (gates 07) |
| 07 | [07-real-release.md](./07-real-release.md) | 06 | rs-modde tags + CI + public registries | — | **Yes (terminal)** |

## Parallelism layer

- **Wave 0 (start from current tree):** **01** (windows fix, rs-modde),
  **03** (SIGTERM forensics, canix/atlas), **04** (secrets audit, canix +
  Codeberg UI), and optional **05** (simit hardening, simit). All four touch
  disjoint repos/concerns and have no ordering between them. Fan them out
  freely.
- **Wave 1 (after 01 accepted):** **02** (verify all 8 cross-targets build,
  including the now-fixed windows). 02 is build-only and does not touch
  `release.yml`, so it does not conflict with 05's regeneration.
- **Wave 2 (after 01, 02, 03, 04 accepted; 05 optional):** **06** — push a
  signed `-rc.N` tag and validate the build + sign + Codeberg-upload spine and
  the prerelease gate logic on a real CI run. Requires the build green (01/02),
  the runner stable (03), and secrets present-or-deliberately-absent (04).
- **Wave 3 (after 06 green):** **07** — re-cut the real release tag onto a
  commit containing all fixes, push, watch the full fan-out, verify every
  channel publishes or cleanly soft-skips. Irreversible. Plan exhausted on a
  green real release with verified artifacts.

Serialization points: 06 and 07 both push tags to the same repo and both
consume the atlas runner (`capacity = 1`) — they are strictly sequential, and
no other phase should hold a release tag or dispatch a release run while 06/07
are in flight. **Do not `canix deploy switch atlas`, reboot atlas, or restart
`forgejo-runner@*.service` while a release run is building** (see Phase 03).

## Whole-set acceptance criteria

- [ ] `nix build .#modde-windows` succeeds locally (no `PowrProf.h` error); all
      8 release targets (`.#modde`, `.#modde-aarch64-linux`, `.#modde-windows`,
      `.#modde-darwin-x86_64`, `.#modde-darwin-aarch64`, `.#appimage-cli`,
      `.#appimage-ui`, `.#flatpak-manifest`) build green (darwin on atlas).
- [ ] The root cause of run 199's SIGTERM is identified from atlas
      journal/dmesg and a mitigation is in place (or it is positively confirmed
      transient with a documented "don't deploy during a release" rule).
- [ ] Every release-workflow secret reference is mapped: present → publishes,
      or intentionally absent → documented clean soft-skip. No half-configured
      channel.
- [ ] A signed `-rc.N` prerelease run on Codeberg is **green** — build + sign +
      Codeberg-release-upload executed; downstream channels skipped-by-gate with
      logged reasons; zero hard failures.
- [ ] A real release tag (re-cut onto a commit with the windows fix) produces a
      **green** run; the Codeberg release exists with all platform/deb/srpm/
      AppImage assets + `SHA256SUMS.txt(.minisig)` + cosign bundles.
- [ ] Each downstream channel is confirmed **published** or documented as
      **intentionally soft-skipped** — no channel in a silent-failure state.

## Global constraints

- **Smartcard-gated signing.** Every tag (`-rc.N` and real) must be GPG-signed
  by the maintainer (PIN). Agents prepare the exact `git tag -s …` /
  `git push --follow-tags …` commands and stop; the maintainer runs them.
- **Generator, not hand-edits.** `release.yml`, `dist/aur/*`, `modde.spec`,
  `.copr/Makefile`, `dist/apt/conf/distributions` are simit-generated. Fix CI
  bugs in the **simit generator** + regenerate (Phase 05), never by hand-editing
  the generated file (it reports as `ci=drift`).
- **Prerelease-first, always.** Never validate by pushing a real `X.Y.Z` first.
- **No silent channel disables.** A channel that should publish must publish or
  be explicitly deferred with the maintainer's agreement.
- **CI inspection without web login:** the public API lacks job-log endpoints
  on this Forgejo version. List runs with
  `curl -fsS -H "Authorization: token $(cat /home/can/.local/share/berg-cli/codeberg.org/TOKEN)" "https://codeberg.org/api/v1/repos/caniko/rs-modde/actions/tasks?limit=10"`;
  fetch a job log with
  `curl -fsS -u caniko:"$(cat /home/can/.local/share/berg-cli/codeberg.org/TOKEN)" "https://codeberg.org/caniko/rs-modde/actions/runs/<N>/jobs/0/attempt/1/logs"`.
  Prefer the `berg` CLI / `berg-codeberg-ci` skill for repo targeting.

## Out of scope for this plan

- Releasing simit's `feat-workspace-version-bump` branch — needed before
  rs-modde **0.2.2** (when a future `simit release` must bump
  `[workspace.package].version`), **not** for 0.2.1 (already bumped). Track
  separately.
- New channels or new packaging features beyond what `release.yml` already
  generates.

## Reference

- Investigation transcript: this session's run-198/199 diagnosis and the
  5-agent adversarial self-verification.
- simit multi-channel packaging memory:
  `~/.claude/projects/-data-nvme0-can-Projects-rs-modde/memory/project_simit_multichannel_packaging.md`.
- Prior phase set (simit side, superseded for the rs-modde release angle):
  `/data/nvme0/can/Projects/simit/docs/src/planning/multichannel-packaging-release/`.
