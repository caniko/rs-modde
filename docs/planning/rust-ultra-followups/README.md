# Plan: rust-ultra follow-ups

> **Recommended Codex model for plan-set orchestration: GPT 5.5 high**
>
> Coordinating this set means holding nine interacting phases across a
> six-crate workspace, where one phase (04, the `GameId` `Deref` removal)
> ripples into every other and dictates the wave ordering. That is
> complex-orchestrator territory: `5.5 high` for dispatch/merge decisions,
> while most individual phases run cheaper. This README is read by a human
> coordinating dispatch; each phase file is standalone for its executor.

## Scope and current state

This plan finishes the **deferred and recommend-only** follow-ups identified by
the `rust-ultra` housekeeping pass on `rs-modde` (a six-crate Cargo workspace,
~95.5k LOC, GPL-3.0, edition 2024, MSRV 1.85, on branch `trunk`).

**Already landed this session (do NOT redo):**

- Correctness: unwrap/fail-fast/panic audits, a real wabbajack zip-slip fix,
  documented `unreachable!()`s, BSA/BA2 cast guards, UTF-8 slice fixes.
- Design: dead `OptiScalerProfiles` trait removed; `FOMODWizardState::config()`
  returns `Option`; ~60 fs error-context additions in modde-games; the
  `app.rs` **test module** extracted to `app/tests.rs` (9,895 → 7,417 lines).
- Test-gaps: `paths::set_data_dir` made idempotent (fixed a parallel-test
  flake); traversal + UTF-8 regression tests.
- **Performance A1/A2/A3** (commit `2509246`): `ArchiveEntry::download_directive`,
  the `archives_by_hash` lookup map, and the env-read hoist in the wabbajack
  installer. **This plan's Phase 01 covers only the remaining perf items
  (A4/A5).**

The whole workspace is green at plan time: `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo check --all-targets`, and
`cargo test --workspace` (1609 passed, 0 failed).

**What these phases deliver:** the invasive, maintainer-driven improvements that
`rust-ultra` audited but deliberately did not auto-apply — quadratic-free zip
extraction, library-output discipline, typed download errors, domain newtypes,
the `app.rs` decomposition, public-API docs, and a dependency-audit gate.

## Phase table

| Phase | File | Depends on | Touches | Can parallel with | Blocking? |
|---|---|---|---|---|---|
| 01 perf: zip + filter | [01-perf-zip-and-filter.md](./01-perf-zip-and-filter.md) | — | `modde-sources/decompress/mod.rs`, `modde-core/filter.rs` | 02, 09 | no |
| 02 observability | [02-observability-library-output.md](./02-observability-library-output.md) | — | `modde-games/launcher.rs`, `modde-cli/commands/install.rs` | 01, 09 | no |
| 03 typed `SourceError` | [03-typed-source-error.md](./03-typed-source-error.md) | 02 (shares cli/install.rs) | `modde-sources/{traits,nexus/*,mega,gdrive,mediafire}`, call sites | — | no |
| 04 `GameId` Deref removal | [04-gameid-deref-removal.md](./04-gameid-deref-removal.md) | — | **all crates** (game_id/mod_id) | — (solo wave) | **yes** |
| 05 Nexus id newtypes | [05-nexus-id-newtypes.md](./05-nexus-id-newtypes.md) | 03, 04 | `modde-sources/nexus/*`, `modde-core/{profile,db}` | — | no |
| 06 `EnabledMod` typed enums | [06-enabledmod-typed-enums.md](./06-enabledmod-typed-enums.md) | 05 | `modde-core/{profile,db,installer}`, install/deploy | — | no |
| 07 `app.rs` decomposition | [07-app-rs-decomposition.md](./07-app-rs-decomposition.md) | 04 | `modde-ui/app.rs` (+ new `app/*`) | 05, 06 | no |
| 08 docs | [08-docs/](./08-docs/README.md) | all (last) | `modde-sources/*`, `modde-games/*` (doc comments) | — | no |
| 09 dependency audit | [09-dependency-audit.md](./09-dependency-audit.md) | — | `*/Cargo.toml` | 01, 02 | no |

## Parallelism layer (execution waves)

This workspace is conflict-dense: `game_id: &str` and the profile/DB types thread
through nearly every crate, so genuine parallelism is limited and **Phase 04
must run alone**. Honest wave structure:

- **Wave 0 (parallel — disjoint files):** 01, 02, 09. `decompress/filter` vs
  `launcher/cli-install` vs `Cargo.toml` don't overlap. Land all three, gate, commit.
- **Wave 1:** 03 (typed `SourceError`). Serialize after 02 because both edit
  `modde-cli/src/commands/install.rs`. Unlocks nothing else by itself.
- **Wave 2 (SOLO):** 04 (`GameId`/`ModId` `Deref` removal). Touches game-id call
  sites in every crate; it must not overlap any other phase. Everything after it
  rebases on the newtyped tree. This is the gate for 05/06/07.
- **Wave 3:** 05 (Nexus id newtypes). After 04 (newtyped tree) and 03 (nexus settled).
- **Wave 4:** 06 (`EnabledMod` enums + DB migration). After 05 — both edit
  `modde-core/src/profile/mod.rs` and `db.rs`.
- **Wave 5:** 07 (`app.rs` decomposition). After 04 (app.rs `game_id` settled).
  Can overlap 05/06 only if you accept that 07 touches `modde-ui` while 05/06
  touch `modde-core` — disjoint crates, so 07 ∥ 05/06 is safe if 04 has landed.
- **Wave 6 (last):** 08 (docs). Run after every signature is final so doc
  comments describe the shipped API; its two sub-layers (sources, games) fan out.

Plan is exhausted after Wave 6.

## Whole-set acceptance criteria

- [ ] `cargo fmt --check` clean.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` clean (the workspace
      enables `clippy::pedantic`; new code must satisfy `doc_markdown`,
      `redundant_locals`, etc.).
- [ ] `cargo check --all-targets --workspace` clean.
- [ ] `cargo test --workspace` ≥ 1609 passed, 0 failed (new phases add tests; the
      count only grows).
- [ ] `cargo deny check` and `cargo audit` pass (Phase 09); `cargo machete` reports
      no unused dependencies; `cargo msrv` confirms 1.85.
- [ ] No new `unsafe` (workspace has zero).
- [ ] Each phase committed atomically with a conventional message.

## Global constraints (apply to every phase)

- **Working tree:** `/data/nvme0/can/Projects/rs-modde`, branch `trunk`. Commit
  atomically per phase; do not squash across phases unless asked.
- **Green gate after every phase:** `cargo fmt && cargo clippy --all-targets
  --workspace -- -D warnings && cargo check --all-targets && cargo test --workspace`.
- **Pedantic clippy is on** with a tuned allow-list in the root `Cargo.toml`
  `[workspace.lints.clippy]`. `cast_possible_truncation`/`cast_sign_loss` are
  *allowed*; `doc_markdown`, `wildcard_imports`, `redundant_locals` are **not** —
  backtick code-ish words in docs, never `use super::*` in non-test modules.
- **Behavior preservation:** refactor phases (01, 04, 05, 06, 07) must not change
  observable behavior. The 1609-test suite + clippy are the safety net; if a
  change can't be made behavior-preserving, leave it and note it.
- **Per-crate test isolation:** the full `cargo test --workspace` is reliable
  (the `set_data_dir` flake is fixed), but if a `"data directory already set"`
  panic ever reappears, re-run the affected crate alone.
- **Use `simit` for any version/changelog/release action** (maintainer convention);
  these phases are code changes, plain `git commit` is fine.

## Reference

- Originating audit: the `rust-ultra` pass in this session (correctness → design →
  polish → deps), summarized in the final report and the squashed history below
  commit `2509246`.
- Recent landed commits: `git log --oneline 7f2d9e3..2509246`.
- Routing: each phase's callout reflects `gpt-plan-routing` (task complexity ×
  role-in-plan); see the chat dispatch summary.
