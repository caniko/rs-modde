# Plan: current outstanding planning work

> **Recommended Codex model for plan-set orchestration: GPT 5.5 high**
>
> This set consolidates three stale or partially complete plan families into one
> active Codex plan. The remaining work crosses docs, UI async behavior, canix
> deployment evidence, and release/tag state, so orchestration is complex even
> though most individual phases are bounded. A smaller model can execute the
> leaf phases, but coordinating retirement coverage and release blockers needs a
> stronger model to avoid copying stale assumptions from the old plans.

## Scope and current state

This directory replaces the old planning directories:

- `docs/src/planning/021-green-release/`
- `docs/src/planning/022-iced-async-db/`
- `docs/src/planning/023-config-chain/`

Progress review found that much of the old work has landed, but none of the old
directories should remain as the active planning surface:

- The release workflow, Windows `PowrProf.h` workaround, cross-target package
  outputs, and generated release wiring now target workspace version `0.3.0`.
  Remote Codeberg tags still only expose `0.2.0` and `0.2.1`, while local
  manifests and changelogs say `0.3.0`. The old `0.2.1` release plan is stale as
  written, but its release/tag consistency intent is still live.
- The iced async DB migration has moved the guarded render path forward:
  `nix develop . -c cargo test -p modde-ui render_path_sources_do_not_call_block_on --all-features`
  passes. Static inspection still finds `crate::app::block_on` in
  `app/model.rs`, `app/tool_ops.rs`, `app/tool_settings.rs`,
  `app/profile_ops.rs`, and startup paths. Those are not all render-path bugs,
  but the old plan's "only the justified bridge remains" criterion is not
  complete.
- The config-chain code and tests have landed: core reads the discrete
  `MODDE_DATABASE_*` env vars, CLI config integration tests pass, and HM database
  assertions exist. Stable mdBook docs still do not document `[database]`,
  `programs.modde.database`, the honored env vars, or `modde config`, so the
  docs sub-layer remains active.
- The canix source tree exists and appears to contain the intended source-side
  changes: `flake.nix` uses `git+https://codeberg.org/caniko/rs-modde.git?ref=trunk`,
  `home/hosts/atlas/can.nix` points modde at
  `postgres:///modde?host=/run/postgresql`, and
  `root/hosts/atlas/server/postgres.nix` declares and grants the `modde`
  database. Live atlas activation, row-count migration, and `modde profile list`
  evidence were not available from source inspection and remain a verification
  item.

`docs/src/SUMMARY.md` does not publish `docs/src/planning`, so this consolidated
set is intentionally not added to mdBook navigation.

## Phase table

| Phase | File | Depends on | Touches | Can parallel with | Status |
|---|---|---|---|---|---|
| 01 | [01-document-database-configuration.md](./01-document-database-configuration.md) | config-chain code already landed | `docs/src/configuration/*`, `docs/src/reference/cli.md` | 02, 03 | outstanding |
| 02 | [02-finish-ui-async-db-cleanup.md](./02-finish-ui-async-db-cleanup.md) | current `modde-ui` async patterns | `crates/modde-ui/src/app*`, tests | 01, 03 | outstanding |
| 03 | [03-verify-canix-modde-database-split.md](./03-verify-canix-modde-database-split.md) | canix source changes present | `/data/nvme0/can/Projects/canix`, atlas runtime evidence | 01, 02 | outstanding verification |
| 04 | [04-reconcile-release-tag-state.md](./04-reconcile-release-tag-state.md) | 01, 03; optionally 02 if the cleanup should ship in the same release | tags, Codeberg release workflow evidence | none after started | outstanding/blocking |

## Parallelism layer

- **Wave 0:** Phases 01, 02, and 03 can start immediately. They touch disjoint
  surfaces: mdBook docs, UI code/tests, and canix/atlas verification. Phase 03
  must not overwrite the existing unrelated `canix` worktree change to
  `flake.lock`.
- **Wave 1:** Phase 04 starts after Phase 01 and Phase 03 are accepted, because a
  release/tag reconciliation should not publish known-missing database docs or
  claim the canix database split without evidence. If the maintainer wants the
  residual UI async cleanup in the same release, Phase 04 also waits for Phase
  02; otherwise it records Phase 02 as deferred.
- **Plan exhausted:** once Phase 04 records a consistent tag/release decision and
  all selected gates pass, rerun this plan set in verify mode and retire
  `024-current-outstanding`.

## External repo coordination

Phase 03 is the only phase with a second working tree. Treat
`/data/nvme0/can/Projects/canix` as a separate ownership boundary: inspect its
dirty state first, isolate any phase edits from existing changes, and do not
commit or revert unrelated paths. Live atlas checks are operational evidence,
not source edits; record command output or a stable run note before marking the
phase accepted.

## Merge-readiness checklist

Before Phase 04 starts, confirm:

- Phase 01 docs landed and `nix build .#docs` is green.
- Phase 03 either produced live atlas proof or a precise blocker report with
  regeneration and validation commands.
- The maintainer has decided whether Phase 02 is release-blocking or deferred.
- `git status --short` in rs-modde contains only intentional changes for the
  release decision.

## PR sequencing and cross-owner coordination

Use separate commits or PRs for docs, UI async cleanup, canix verification/source
changes, and release/tag reconciliation. Phase 04 should be last because it
turns the technical state into public release state. If Phase 03 requires canix
source edits, land those in canix before using the release phase to claim the
database split is complete.

## Whole-set acceptance criteria

- [ ] The database configuration surface is documented in stable docs:
  `[database]` settings keys, honored env vars, HM `database` options/assertions,
  and `modde config show/set-database/reset-database/test`.
- [ ] The residual UI async cleanup is either completed with explicit guards or
  intentionally deferred in the release decision with a current rationale.
- [ ] canix source and live atlas evidence prove modde uses the dedicated
  `modde` PostgreSQL database and skillnet remains on `can`, or the missing live
  evidence is reported with exact regeneration and validation commands.
- [ ] `Cargo.toml`, changelogs, local tags, remote Codeberg tags, and Codeberg
  release state are internally consistent for the chosen version.
- [ ] `nix build .#docs`, the targeted Rust tests named in the phase files, and
  the release/tag validation commands pass.
- [ ] `rg -n "021-green-release|022-iced-async-db|023-config-chain" docs/src -g '!docs/src/planning/024-current-outstanding/README.md'`
  returns no stale references outside this coverage report.

## Coverage and retirement report

| Old plan set | Classification | Representation here |
|---|---|---|
| `021-green-release` | Mostly obsolete as a `0.2.1` plan; current release state is inconsistent for `0.3.0`. | Phase 04 carries forward tag/version/release validation, prerelease-first discipline, generated-release-workflow constraint, and atlas freeze/run evidence requirements. |
| `022-iced-async-db` | Partial. Render-path guard is green, but residual `block_on` sites and justified-bridge documentation remain. | Phase 02 carries forward only the remaining async cleanup and guard-hardening work. |
| `023-config-chain` | Core/CLI/HM source work done; docs and live canix runtime proof remain. | Phase 01 carries docs; Phase 03 carries canix live verification. Source-side tests are preserved as acceptance evidence. |

Completed execution scaffolding, old phase numbering, and stale 0.2.1-specific
release instructions are intentionally not preserved. Durable behavior that is
not yet in stable docs is represented as new work instead of being copied into
current docs without verification.

## Planner Handoff

### Dossier path

docs/src/planning/024-current-outstanding/README.md

### Current-state summary

The old planning tree has been collapsed to four remaining work slices. Config
chain code and tests are green but stable docs are missing. UI render-path tests
are green but residual `block_on` use needs classification and cleanup. canix
source appears updated, but live atlas database split evidence is missing. The
workspace says `0.3.0` while remote Codeberg tags only show `0.2.0` and `0.2.1`,
so release state must be reconciled before claiming the release plan is complete.

### Recommended planner flavour

multi-phase-plan-codex, because the user explicitly requested the Codex flavour
and the remaining phases are intended for fresh Codex execution sessions.

### Work that should become phases

- Document database configuration in mdBook stable docs.
- Finish or explicitly classify the residual UI async DB cleanup.
- Verify canix live database split evidence without overwriting unrelated canix
  worktree changes.
- Reconcile workspace version, changelogs, local/remote tags, and Codeberg
  release state for the current release.

### Known blockers

- Live atlas database migration evidence was not available from source
  inspection. Upstream producer: the canix/atlas operator. Regenerate by running
  Phase 03's atlas validation commands after deployment. Validate with the
  `psql`, `modde profile list`, and skillnet commands listed in Phase 03.
- No remote `0.3.0` tag was visible from `git ls-remote --tags origin` during
  audit. Upstream producer: the release maintainer. Regenerate by making an
  explicit Phase 04 decision to tag/publish `0.3.0` or move the changelog state
  back under `Unreleased`. Validate with Phase 04's tag and release probes.

### Acceptance evidence to preserve

- `nix develop . -c cargo test -p modde-core db::tests --all-features` passed
  with 32 DB unit tests.
- `nix develop . -c cargo test -p modde-cli --test cli_config_tests --all-features`
  passed with 8 CLI config integration tests.
- `nix develop . -c cargo test -p modde-ui render_path_sources_do_not_call_block_on --all-features`
  passed with the render-path guard.
- `git ls-remote --tags origin` returned only `0.2.0` and `0.2.1` tags during
  audit, while `Cargo.toml` and changelogs contain `0.3.0`.
