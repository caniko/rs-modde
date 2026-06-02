# Phase 05 — Convert the profile & experiment write handlers to async (close the whole-set gap)

> **Recommended Codex model: GPT 5.5 high**
>
> The loader→message→apply pattern, the generation guard, and the async write
> precedent (`ToolSettingWritten`) all exist after Phases 02-03, so this is not
> frontier work — but it is not pure boilerplate either. Nine handlers across
> three concern groups (profile CRUD, load-order mutation, experiment lifecycle)
> each need a write-then-reload conversion where ordering matters: fire the
> reload before the write commits and the user sees their change vanish; skip the
> optimistic state and the UI lags a round-trip; mishandle the experiment state
> machine (`try_profile`/`rollback`/`commit` touch git + `experiment_depth`) and
> you corrupt the active experiment. Multiple interacting handlers, real
> sequencing hazards, established template — orchestrator role at complex
> complexity. A weaker model tends to batch the write and reload concurrently and
> ship the intermittent "my change didn't stick" bug this phase exists to prevent.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. **Phases 01-04 must already
be landed** (verified `partial`: all four passed, this phase closes the one
unmet whole-set criterion). You will edit `crates/modde-ui/src/app/update.rs`
heavily and `crates/modde-ui/src/app.rs` (new `Message` variants), reusing the
`ProfileContextSnapshot` loader, the `context_generation` guard, and the
`Task::perform(write_fut) → Message::*Written → reload` template that
`ToolSettingWritten` already established in Phase 03.

## Goal

No `crate::app::block_on` call remains on a synchronous `update`/`view`/`new`
code path **anywhere** — the whole-set criterion the plan promised but the first
four phases left unmet. The nine profile/experiment write handlers perform their
DB mutation off the render thread and only then trigger the (already-async)
reload, so creating, deleting, forking, reordering, locking, unlocking a
profile, and starting/rolling-back/committing an experiment never freeze the
window. The Phase-04 regression guard is extended to police `update.rs` too, so
the property cannot silently regress.

## Why this matters now

Verify of this plan set found ~23 render-path `block_on` calls still live across
nine handlers in [update.rs](../../../../crates/modde-ui/src/app/update.rs):

- `CreateProfile` ([:306](../../../../crates/modde-ui/src/app/update.rs)) — `block_on(pm.create(&profile))`
- `DeleteProfile` ([~:344](../../../../crates/modde-ui/src/app/update.rs)) — `block_on(pm.delete(&name, …))`
- `ForkProfile` ([:365](../../../../crates/modde-ui/src/app/update.rs)) — `block_on(pm.fork(…))`
- `ReorderMod` ([:1212](../../../../crates/modde-ui/src/app/update.rs)) — `block_on(pm.load)` + `create`/`update`
- `LockMod` ([:1267](../../../../crates/modde-ui/src/app/update.rs)) / `UnlockMod` ([:1283](../../../../crates/modde-ui/src/app/update.rs)) — `block_on(pm.load)` + `update`
- `TryProfile` ([:2186](../../../../crates/modde-ui/src/app/update.rs)) — `block_on(pm.try_profile(…))`
- `RollbackExperiment` ([:2205](../../../../crates/modde-ui/src/app/update.rs)) — `block_on(pm.rollback(…))`
- `CommitExperiment` ([:2221](../../../../crates/modde-ui/src/app/update.rs)) — `block_on(pm.commit(…))`

In each, Phase 02 made the *reload* async (`self.reload_profile()` returns a
Task) but explicitly left the *write* synchronous ("do not restructure the write
itself"). So every profile mutation still blocks the UI thread for the duration
of the write — one query for CRUD/lock, and for `try_profile`/`rollback`/`commit`
a git-backed staging operation that can be slow. On PostgreSQL each is also a
fresh-or-pooled write round-trip. These are lower-frequency than the loads fixed
in Phases 02-03, but they are the last render-path blockers and the reason the
README's headline promise is only partially met.

## Out of scope

- The reload itself — already async (Phase 02). This phase only moves the
  *write* off-thread and then chains the existing reload Task.
- Any change to `ProfileManager` semantics in `modde-core`. Its methods are
  already `async`; this phase only changes *where* the UI awaits them.
- `view.rs`/`model.rs` `block_on` — already clean and guarded (Phase 04).
- The `new()` one-time startup open — allowed by the whole-set criterion; leave it.

## Plan

1. **Add async write fns** (in [update.rs](../../../../crates/modde-ui/src/app/update.rs)
   or a small `profile_ops.rs` next to `tool_ops.rs`/`install_ops.rs`),
   each taking an owned `ModdeDb` and owned args, returning a `Result<_, String>`.
   They `.await` the `ProfileManager::with_db(db)` method directly if the future
   is `Send + 'static` (it is, with owned args — see Phase 04's classification),
   else wrap in `spawn_blocking(move || crate::app::block_on(…))`. Group:
   - **CRUD:** `create_profile`, `delete_profile`, `fork_profile`.
   - **Load-order:** `reorder_mod` (load → mutate `load_order_rules` → update),
     `set_mod_lock` (lock/unlock → update). Reuse the existing in-arm mutation
     logic verbatim; only the DB calls change.
   - **Experiment:** `try_profile`, `rollback_experiment`, `commit_experiment`
     (these call `pm.try_profile`/`rollback`/`commit`, which do git+staging work
     — wrap these in `spawn_blocking`, they are CPU/IO heavy, mirror
     `run_wabbajack_install_for_ui`).

2. **Add `Message` variants** ([app.rs](../../../../crates/modde-ui/src/app.rs)) carrying
   the write result. Prefer a small set over one-per-handler where the apply is
   identical, e.g.:
   ```rust
   ProfileWriteDone { generation: u64, kind: ProfileWriteKind, result: Result<(), String> },
   ExperimentWriteDone { generation: u64, result: Result<(), String> },
   ```
   `ProfileWriteKind` distinguishes Create/Delete/Fork/Reorder/Lock/Unlock for
   the status message and any kind-specific apply (e.g. Create sets
   `active_profile`/`selected_game`; Delete may clear selection).

3. **Convert each arm.** The synchronous prelude that builds the profile/args and
   sets optimistic UI state stays synchronous; replace the `block_on(write)` +
   `self.reload_profile()` tail with: bump `context_generation`, then
   `return Task::perform(write_fut, move |result| Message::ProfileWriteDone {
   generation, kind, result })`. Where an arm previously did
   `Task::batch`-able extra work (status message, dialog close, `save_settings`),
   keep that synchronous and batch only if it itself returns a Task.

4. **Write the `*WriteDone` apply arms.** On `Ok`: apply kind-specific state
   (the bits the old synchronous tail set), set the success status, and
   **`return self.reload_profile()`** (the Phase-02 reload Task) so the committed
   state is re-read — *chained after the write resolves, never concurrent with
   it*. On `Err`: revert any optimistic state and set an error status; do not
   reload. Drop the result if `generation != self.context_generation` (a newer
   mutation/switch superseded this one).

5. **Preserve the experiment state machine.** `TryProfile`/`RollbackExperiment`/
   `CommitExperiment` adjust `experiment_depth` and depend on `pm.active(...)`.
   Keep the existing success/error branches (status text, `experiment_depth`
   update via the reload's `ProfileContextSnapshot.experiment_depth`); just move
   the git-backed call off-thread. Verify the `TryProfile` save-dir resolution
   (`Self::resolve_save_dir`) stays synchronous (it is pure path logic).

6. **Extend the regression guard.** In
   [tests.rs `render_path_sources_do_not_call_block_on`](../../../../crates/modde-ui/src/app/tests.rs)
   add `"app/update.rs"` to the policed `sources` array, with an allow-list of
   the legitimate off-thread/startup markers (the `new()` startup open, and any
   `Task::perform`/`spawn_blocking` closure lines). Match the model.rs marker
   approach: track the enclosing fn and allow `block_on` only inside
   `spawn_blocking`/`Task::perform` closures or `#[cfg(test)]` helpers. Prove it
   fails by temporarily reintroducing a render-path `block_on`, then revert.

7. **Tests.** Add (or extend) `tests.rs`: a create→reload test asserting the new
   profile appears and is active; a reorder/lock test asserting the mutation
   persists after the chained reload; a stale-generation test asserting a
   superseded `ProfileWriteDone` is discarded; an experiment try→commit test.
   Drive Tasks via the existing `iced_test` harness
   ([tests.rs:109](../../../../crates/modde-ui/src/app/tests.rs)) or the
   `#[cfg(test)]` `*_blocking` helper pattern Phase 02 introduced.

## Acceptance criteria

- [ ] `rg -n "crate::app::block_on" crates/modde-ui/src/app/update.rs` returns
  **only**: the two `new()` startup-open lines, and lines inside
  `Task::perform`/`spawn_blocking` closures. No `block_on` in a bare match arm.
- [ ] The nine handlers (`CreateProfile`, `DeleteProfile`, `ForkProfile`,
  `ReorderMod`, `LockMod`, `UnlockMod`, `TryProfile`, `RollbackExperiment`,
  `CommitExperiment`) each `return Task::perform(…)` for their write.
- [ ] `render_path_sources_do_not_call_block_on` now includes `app/update.rs`
  and passes; it **fails** if a render-path `block_on` is reintroduced into
  update.rs (proven once, then reverted).
- [ ] A create/reorder/lock test asserts the mutation persists across the chained
  reload; a stale-generation `ProfileWriteDone` is dropped (asserted); an
  experiment try→commit test is green.
- [ ] `cargo build -p modde-ui` and `cargo build -p modde-ui --no-default-features`
  compile clean.
- [ ] `cargo clippy -p modde-ui --all-targets -- -D warnings` clean.
- [ ] `cargo test -p modde-ui` green.
- [ ] Manual (`run` skill): creating, deleting, forking a profile; reordering /
  locking a mod; and starting / rolling back / committing an experiment all
  repaint immediately with no window freeze; a created profile becomes active
  and its mods/conflicts/tools fill in shortly after.
- [ ] **Whole-set criterion now met:** the README's "no `block_on` on a
  synchronous update/view/new path" is satisfiable by `rg` across `view.rs`,
  `model.rs`, **and** `update.rs`.

## Files likely touched

- `crates/modde-ui/src/app/update.rs` — nine write arms + the `*WriteDone` apply arms.
- `crates/modde-ui/src/app.rs` — `ProfileWriteDone`/`ExperimentWriteDone` (+ `ProfileWriteKind`).
- `crates/modde-ui/src/app/profile_ops.rs` *(optional new file)* — the async write fns.
- `crates/modde-ui/src/app/tests.rs` — guard extension + write/experiment tests.

## Pitfalls

- **Symptom:** a freshly created/reordered profile flickers back to its old
  state. **Cause:** the reload Task ran before/concurrent with the write
  committing. **Recovery:** only fire `self.reload_profile()` from the
  `*WriteDone` **Ok** apply, after the write future resolved — never batch the
  write and reload to run together.
- **Symptom:** experiment depth badge or active-experiment state goes wrong after
  try/rollback/commit. **Cause:** the experiment write's success branch dropped a
  side effect, or `experiment_depth` wasn't refreshed from the reload snapshot.
  **Recovery:** the reload's `ProfileContextSnapshot.experiment_depth` is the
  source of truth — apply it; keep the existing status-text branches.
- **Symptom:** `Task::perform` won't compile ("future is not `Send`"). **Cause:**
  same as Phases 02/04 — a borrow or non-`Send` value held across await, or the
  call is actually CPU/IO-heavy. **Recovery:** for CRUD/lock, move owned
  `ModdeDb`/args in and `.await` directly; for the git-backed experiment ops,
  keep `spawn_blocking(move || crate::app::block_on(…))`.
- **Symptom:** the extended guard is flaky or rejects legitimate off-thread
  `block_on`. **Cause:** update.rs has many `Task::perform`/`spawn_blocking`
  closures that legitimately call `block_on`. **Recovery:** scope the allow-list
  to lines inside such closures (enclosing-context tracking like the model.rs
  branch) plus the two `new()` startup lines; do not blanket-allow the file.
- **Symptom:** double-mutation race (two fast reorder clicks) applies the older
  result. **Cause:** missing generation check on `ProfileWriteDone`.
  **Recovery:** bump `context_generation` at each kickoff and discard stale
  results in the apply.

## Reference

- Plan README + whole-set acceptance: [README.md](./README.md).
- Verify finding that motivates this phase (the `missed-signal: hidden-prerequisite`
  gap): `.calibration.json` `verify` section in this directory.
- Async write template to copy: `ToolSettingWritten` / `toggle_tool_for_game`
  (Phase 03; [tool_ops.rs](../../../../crates/modde-ui/src/app/tool_ops.rs),
  [tool_settings.rs](../../../../crates/modde-ui/src/app/tool_settings.rs)).
- Reload Task to chain: `Modde::reload_profile()` and the
  `ProfileContextSnapshot` loader (Phase 02,
  [model.rs](../../../../crates/modde-ui/src/app/model.rs)).
- Regression guard to extend: `render_path_sources_do_not_call_block_on`
  ([tests.rs:429](../../../../crates/modde-ui/src/app/tests.rs)).
- The handlers being converted: update.rs lines 306, 344, 365, 1212, 1267, 1283,
  2186, 2205, 2221.
- Prereqs: [01](./01-shared-db-handle.md), [02](./02-async-profile-game-context.md),
  [03](./03-async-diagnostics-and-tool-writes.md), [04](./04-loader-hygiene.md).
