# Phase 02 — Finish the UI async DB cleanup

> **Recommended Codex model: GPT 5.5 high**
>
> This is a non-trivial coding phase inside iced's event loop. The current
> render-path guard passes, but residual `block_on` calls span model loading,
> tool operations, profile operations, and startup. High is warranted because
> the agent must distinguish justified CPU/filesystem bridges from UI-thread DB
> blocking without breaking tests.

## Working tree

`/data/nvme0/can/Projects/rs-modde`. This phase must start from the current
`trunk` tree and should not touch docs except test comments if needed.

## Goal

All remaining `crate::app::block_on` sites in `crates/modde-ui/src` are either
removed from DB-only async work or explicitly justified behind a blocking bridge
for synchronous CPU/filesystem work, with regression tests covering the rule.

## Why this matters now

The retired async DB plan is partial. The guard
`render_path_sources_do_not_call_block_on` passes, but static inspection still
finds `block_on` in `app/model.rs`, `app/tool_ops.rs`, `app/tool_settings.rs`,
`app/profile_ops.rs`, and startup helpers. Leaving the old plan in place hides
the narrower remaining work; deleting it without replacement would lose the
cleanup criterion.

## Out of scope

- Do not redesign iced navigation or introduce subscriptions for one-shot DB
  work.
- Do not introduce a process-global database pool; test isolation depends on
  per-app/per-test handles.
- Do not remove `spawn_blocking` where the closure performs substantial
  synchronous filesystem or launcher generation work.

## Plan

1. Inventory current sites:
   `rg -n "crate::app::block_on" crates/modde-ui/src`.
2. Classify each site as startup-only, test-only, CPU/filesystem bridge, or
   removable DB-only async work. Record the classification in a short comment or
   test fixture only where it prevents future confusion.
3. Convert removable DB-only sites to direct `.await` inside existing async
   helpers or `Task::perform` futures. Prioritize:
   `app/model.rs::load_profile_context`, `app/profile_ops.rs::fork_profile`,
   `app/profile_ops.rs::run_experiment_write`, and pure DB calls in
   `tool_settings.rs`.
4. Keep `spawn_blocking` only when the closure mixes DB reads/writes with
   blocking filesystem/tool generation. Narrow the `block_on` shim doc comment
   to that contract.
5. Extend the regression guard so it checks every source file that can execute
   on the iced update/view/new path, while allowing annotated startup/test or
   bridge-only exceptions.
6. Run the modde-ui gates.

## Acceptance criteria

- [ ] `rg -n "crate::app::block_on" crates/modde-ui/src` returns only annotated
  startup/test or blocking-bridge sites; no DB-only helper wraps async DB calls
  in `block_on`.
- [ ] `render_path_sources_do_not_call_block_on` covers the current update/view
  path file set and passes.
- [ ] `nix develop . -c cargo build -p modde-ui` succeeds.
- [ ] `nix develop . -c cargo build -p modde-ui --no-default-features` succeeds.
- [ ] `nix develop . -c cargo clippy -p modde-ui --all-targets -- -D warnings`
  succeeds.
- [ ] `nix develop . -c cargo test -p modde-ui` succeeds.

## Files likely touched

- `crates/modde-ui/src/app.rs`
- `crates/modde-ui/src/app/model.rs`
- `crates/modde-ui/src/app/profile_ops.rs`
- `crates/modde-ui/src/app/tool_ops.rs`
- `crates/modde-ui/src/app/tool_settings.rs`
- `crates/modde-ui/src/app/tests.rs`

## Pitfalls

- **Symptom:** future is not `Send`. **Cause:** borrowed app state or borrowed DB
  handle crosses `.await`. **Recovery:** pass owned `ModdeDb` clones and owned
  arguments into the async helper.
- **Symptom:** UI tests leak state between cases. **Cause:** a global pool or
  shared runtime state was introduced. **Recovery:** keep DB handles on `Modde`
  and test fixtures.
- **Symptom:** launcher/tool generation blocks the async executor. **Cause:**
  converting a CPU/filesystem bridge to direct `.await`. **Recovery:** keep that
  bridge in `spawn_blocking` and document why it is not a render-path DB wait.

## Reference

- `crates/modde-ui/src/app/tests.rs::render_path_sources_do_not_call_block_on`
- `crates/modde-ui/src/app/model.rs`
- `crates/modde-ui/src/app/profile_ops.rs`
- Predecessor planning docs remain available in git history if historical
  context is needed.
