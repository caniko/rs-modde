# Phase 02 — Convert the profile/game context cluster to async loaders + apply

> **Recommended Codex model: GPT 5.5 max**
>
> This is the frontier phase. Five `model.rs` helpers
> (`reload_profile`, `switch_game_context`, `refresh_data_tab_conflicts`,
> `refresh_tools_state`, `accept_game_selection`) are mutually recursive and
> reached from **~20 distinct `update.rs` arms plus `new()`**. They cannot be
> converted one at a time — the tree only compiles once the whole cluster moves
> to a Task-returning shape, so this is a single atomic rollback boundary across
> a large, interconnected surface. On top of the mechanical breadth there is a
> genuine concurrency hazard: an async profile/game load that resolves *after*
> the user has already switched away will clobber current state with stale data
> unless every loader is generation-guarded. Mediocre work here ships a subtle,
> hard-to-reproduce state-tearing regression. Top-level/orchestrator role at
> frontier complexity — worth `max`, and it carries the set's risk so the README
> and other phases can stay at `high`/`medium`.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. **Phase 01 must land
first** — this phase assumes `Modde` holds a shared `db: ModdeDb` handle and
that `ProfileManager::with_db(self.db.clone())` is the way to get a manager.
Pull/rebase so `crates/modde-ui/src/app/{model,update}.rs` and `app.rs` reflect
Phase 01 before starting; you will edit all three heavily.

## Goal

Loading or reloading the profile/game context never blocks the iced event loop.
The five synchronous helpers are replaced by: (a) one (or a small number of)
**async loader function(s)** that compute an owned snapshot off the render
thread, returned via `Task::perform`; (b) new `Message::*Loaded` variant(s)
carrying that snapshot plus a generation token; (c) **synchronous `apply_*`
method(s)** that write the snapshot into `self`. Every `update.rs` arm and
`new()` that previously called a sync helper now returns the corresponding
`Task`. Success looks like: switching games/profiles repaints immediately and
shows a "loading…" status, the data fills in when the load resolves, and a fast
double-switch never displays the first game's data.

## Why this matters now

These helpers are the worst offenders. `refresh_tools_state`
([model.rs:540-799](../../../../crates/modde-ui/src/app/model.rs)) opens the DB and then,
**inside a loop over every tool**, issues `load_tool_config` +
`load_applied_files` + (sometimes) `save_tool_config` + `list_tool_setting_history`
— all via `block_on` on the render thread. `reload_profile`
([model.rs:123-166](../../../../crates/modde-ui/src/app/model.rs)) calls
`list`/`load`/`active`, then calls `refresh_data_tab_conflicts` **and**
`refresh_tools_state`. `switch_game_context`
([model.rs:168-196](../../../../crates/modde-ui/src/app/model.rs)) calls all of the above.
Every one of the ~20 call sites (e.g. `ExternalRefresh`
[update.rs:165](../../../../crates/modde-ui/src/app/update.rs), `SelectGame`
[update.rs:354](../../../../crates/modde-ui/src/app/update.rs), `CreateProfile`
[update.rs:280](../../../../crates/modde-ui/src/app/update.rs)) therefore freezes the
window for the full duration of a multi-query (PostgreSQL: multi-handshake)
sequence. This is the single largest source of UI stalls in the app.

`refresh_tools_state` is also **redundant**: its async twin already exists as
`load_tools_state`/`apply_tool_snapshot` (driven by `start_tools_load`,
[model.rs:453-503](../../../../crates/modde-ui/src/app/model.rs)). Part of this phase is
deleting the sync twin and routing its callers through the existing async one.

## Out of scope

- `run_diagnostics_now` and the tool-setting **write** handlers
  (`UpdateToolSetting`, `ToggleTool`, `RestoreToolSettings`, proton/optiscaler
  writes) — those are **Phase 03**. This phase only covers the *read/reload*
  cluster. (Where a write handler currently ends by calling `reload_profile()`,
  have it return the reload Task — but do not restructure the write itself.)
- Dropping `spawn_blocking`/`block_on` from loaders that also do heavy CPU work
  (tools, conflict analysis) — that is **Phase 04**. Here, loaders may keep the
  `spawn_blocking(move || block_on(…))` bridge; the win is moving them off the
  render thread, not eliminating the bridge.
- Any change to `view.rs` rendering logic beyond reading already-populated state.

## Plan

1. **Design the snapshot + loader shape.** The three reads that always travel
   together (profile + data-tab analysis + tools) should resolve in **one**
   off-thread job to avoid a render with half-populated state. Define owned
   result structs, e.g.:
   ```rust
   pub(super) struct ProfileContextSnapshot {
       pub profiles: Vec<ProfileSummary>,
       pub active_profile: Option<String>,
       pub loaded_profile: Option<modde_core::Profile>,
       pub experiment_depth: usize,
       pub current_fingerprint: Option<SaveFingerprint>,
       pub mod_id_filter_keys: Vec<String>,
       pub data_tab_conflicts: Vec<(String, Vec<String>)>,
       pub missing_store_mod_count: usize,
       pub tools: Option<ToolLoadSnapshot>,   // reuse the existing struct
       // …whatever switch_game_context / reload_profile currently set
   }
   ```
   Reuse `ToolLoadSnapshot` (already applied by `apply_tool_snapshot`).

2. **Write the async loader.** A free `async fn load_profile_context(db: ModdeDb,
   game_id: Option<String>, profile_name: Option<String>, …) ->
   ProfileContextSnapshot` that wraps the existing helper bodies. It may keep the
   `tokio::task::spawn_blocking(move || block_on(…))` form (mirroring
   `load_tools_state`, [tool_ops.rs:90-94](../../../../crates/modde-ui/src/app/tool_ops.rs))
   because it folds in the CPU-heavy `analyze_profile_state` and tool detection.
   Move the bodies of `reload_profile` / `refresh_data_tab_conflicts` /
   `refresh_tools_state` into this loader (or call the existing
   `load_tools_state_blocking` from inside it).

3. **Add the generation guard.** Add a `context_generation: u64` to `Modde`
   (mirror `tool_state.load_generation`, [model.rs:464](../../../../crates/modde-ui/src/app/model.rs)).
   Each kickoff bumps it; the `Message::ProfileContextLoaded { generation,
   snapshot }` arm **drops the result if `generation != self.context_generation`**.
   This is the linchpin against stale-snapshot clobbering on rapid switches.

4. **Add `Message` variants** (in [app.rs](../../../../crates/modde-ui/src/app.rs) `enum
   Message`): `ProfileContextLoaded { generation: u64, result:
   Result<ProfileContextSnapshot, String> }`. (One composite message is
   preferable to several; it keeps the apply atomic.)

5. **Write the synchronous `apply_profile_context(&mut self, snapshot)`** that
   writes every field into `self` — the synchronous tail of the old helpers.
   Reuse `apply_tool_snapshot` for the `tools` sub-field.

6. **Convert the kickoff methods to return `Task<Message>`.** Replace the bodies
   of `reload_profile`, `switch_game_context`, `accept_game_selection` so they
   set "loading" status, bump the generation, clear game-scoped state where they
   do today (`clear_game_scoped_state`, [model.rs:90](../../../../crates/modde-ui/src/app/model.rs)),
   and `return Task::perform(load_profile_context(self.db.clone(), …), move |r|
   Message::ProfileContextLoaded { generation, result: r })`. Note
   `accept_game_selection` has a synchronous early-return branch (opening the
   game-path dialog, [model.rs:218-225](../../../../crates/modde-ui/src/app/model.rs)) — that
   branch returns `Task::none()`.

7. **Thread Tasks through all callers.** Every site listed below must now return
   (or `Task::batch`) the Task instead of calling the helper for side effects:
   - `reload_profile` callers (16): `update.rs` lines 165, 244, 280, 305, 321,
     582, 641, 658, 1177, 1222, 1240, 1366, 1718, 2168, plus `model.rs:189`
     (inside `switch_game_context`, now composed) and `tests.rs:1969`.
   - `switch_game_context` callers: `update.rs` 299, 395, 408, `model.rs:232`.
   - `refresh_data_tab_conflicts` callers: `update.rs:206`, `model.rs` 164, 193.
   - `accept_game_selection` callers: `update.rs` 135, 356, 503, 539.
   Where an arm did other work and then called the helper, combine with
   `Task::batch([other_task, reload_task])`. Where it returned a different Task
   already, batch them.

8. **Convert `new()`'s initial load.** Phase 01 left a one-time blocking
   `list()` in `new()`. Now return an initial `Task::perform(load_profile_context(
   db.clone(), settings.selected_game.clone(), None, …), …)` from `new()` so the
   first paint shows "Loading…" and the profile list fills in asynchronously.
   Start the struct with empty `profiles`/`loaded_profile`. (The one-time
   `ModdeDb::open()` in `new()` from Phase 01 stays — only the *queries* move to
   the Task.)

9. **Delete the now-dead sync helpers** once no caller remains:
   `refresh_tools_state` (its async twin already exists), and the sync bodies of
   `reload_profile`/`refresh_data_tab_conflicts`. Keep the small pure helpers
   they used (`build_conflict_rows`, `load_hidden_files`, etc.) — the loader
   calls them off-thread now.

10. **Update tests.** `tests.rs:1969` calls `app.reload_profile()`
    synchronously; replace with driving the returned Task (the harness already
    uses `iced_test::futures::futures::executor::block_on`,
    [tests.rs:105](../../../../crates/modde-ui/src/app/tests.rs)) or by calling
    `load_profile_context(...)` + `apply_profile_context(...)` directly in the
    test. Keep all assertions.

## Acceptance criteria

- [ ] `refresh_tools_state` is **deleted**; `rg -n "fn refresh_tools_state"
  crates/modde-ui` returns nothing. `reload_profile`/`switch_game_context`/
  `accept_game_selection` return `Task<Message>` (or are subsumed by the loader).
- [ ] No `crate::app::block_on` remains in `model.rs` on a `&mut self`
  helper that is reachable from `update`/`view` (verify by reading; the only
  `block_on` left in the UI is inside `Task::perform`/`spawn_blocking` closures).
- [ ] A `context_generation` guard exists and the `ProfileContextLoaded` arm
  drops stale results. Add a unit test in `tests.rs` that fires two loads with
  generations N and N+1 and asserts the N result is discarded.
- [ ] `cargo build -p modde-ui` and `--no-default-features` compile.
- [ ] `cargo clippy -p modde-ui --all-targets -- -D warnings` clean.
- [ ] `cargo test -p modde-ui` green (all pre-existing tests, adjusted).
- [ ] Manual (`run` skill): selecting a game with many tools repaints
  immediately with a loading indicator; data fills in shortly after; **rapidly
  switching between two games never leaves the first game's tools/conflicts on
  screen** (the generation guard working).

## Files likely touched

- `crates/modde-ui/src/app/model.rs` — loader, snapshot struct, apply method,
  generation field, helper deletions.
- `crates/modde-ui/src/app/update.rs` — ~20 arms return Tasks; `new()` initial Task.
- `crates/modde-ui/src/app.rs` — new `Message` variant(s); possibly move
  `load_hidden_files`/`load_active_plugins` next to the loader.
- `crates/modde-ui/src/app/tool_ops.rs` — possibly reuse `load_tools_state_blocking`
  from the composite loader.
- `crates/modde-ui/src/app/tests.rs` — drive Tasks instead of calling sync helpers.

## Risk profile

- **R1 — Stale-snapshot clobber.** Async load for game A resolves after the user
  switched to game B; without the generation guard, A's tools/conflicts
  overwrite B's. Most likely on slow PostgreSQL.
- **R2 — Atomic compile boundary.** Because the five helpers are mutually
  recursive and shared by ~20 arms, a partial conversion does not compile; you
  cannot land "half" of this phase. High chance of a long red-tree window.
- **R3 — Lost side effects.** The sync helpers set many fields
  (`experiment_depth`, `current_fingerprint`, `mod_id_filter_keys`,
  `data_tab_state.missing_store_mod_count`, status messages, `active_profile`
  fallback logic). Dropping one in the snapshot/apply silently breaks a view.
- **R4 — `Task::batch` ordering.** Arms that did work *then* reloaded may depend
  on ordering; batching tasks that both touch the DB can interleave.
- **R5 — Test harness.** `tests.rs` drives `update` synchronously; Task-returning
  arms may need the iced_test harness to pump the Task to completion.

## Strategy

Use a **twin-then-migrate-then-delete** commit ladder to shrink the red-tree
window (R2), rather than one giant edit:

1. **Commit 1 — additive.** Land the snapshot struct, `load_profile_context`,
   `apply_profile_context`, the `Message` variant, and the `context_generation`
   field. Add new *Task-returning* methods (`reload_profile_task`, etc.)
   **alongside** the existing sync helpers. Tree compiles; nothing calls the new
   methods yet. *Revert cost: trivial (pure additions).*
2. **Commit 2 — migrate callers in batches.** Point `update.rs` arms at the new
   Task methods, a logical group at a time (profile dialog arms; then
   game-select arms; then experiment/reorder arms; then `ExternalRefresh`/`new`).
   Each batch compiles and tests green. *Revert cost: per-batch.*
3. **Commit 3 — delete the sync twins** once `rg` shows zero callers of the old
   helpers. *Revert cost: re-add the deleted fns (kept in commit-1 history).*

## Rollback drill

Practice before Commit 3 (the only destructive one — it deletes code):

```sh
git stash list                       # ensure clean baseline
git checkout -b phase02-deleteguard  # scratch branch
# … perform the deletion commit …
git revert --no-edit HEAD            # confirm the delete reverts cleanly
cargo build -p modde-ui              # must compile post-revert (twins restored)
git reset --hard HEAD~1              # drop the scratch revert
```

SLA: if `cargo build -p modde-ui` is still red **20 minutes** after Commit 3,
`git reset --hard` to Commit 2 and ship the twin-coexistence state (working,
just with dead sync helpers) rather than burning the session.

## Failure modes and recoveries

- **F1 — Stale clobber.** *Symptom:* switching games fast shows the wrong game's
  tools. *Cause:* missing/incorrect generation check. *Recovery:* the
  `ProfileContextLoaded` arm must early-return when `generation !=
  self.context_generation`; bump the generation in **every** kickoff before
  building the Task. Mirror `ToolsLoaded` ([model.rs:464-469, and the ToolsLoaded
  arm in update.rs]).
- **F2 — Panic "Cannot start a runtime from within a runtime."** *Symptom:* crash
  when a loader runs. *Cause:* you called `tokio::runtime::Runtime::new().block_on`
  inside an async Task instead of the `crate::app::block_on` shim or
  `Handle::current().block_on` inside `spawn_blocking`. *Recovery:* inside
  `Task::perform` closures use `spawn_blocking(move || crate::app::block_on(…))`
  (the proven form, [tool_ops.rs:91](../../../../crates/modde-ui/src/app/tool_ops.rs)), or
  `.await` directly if the future is `Send` (Phase 04 territory).
- **F3 — Missing field after apply.** *Symptom:* a view (Saves fingerprint,
  experiment badge, mod filter) goes blank after a reload. *Cause:* the snapshot
  omitted a field the old sync helper set. *Recovery:* diff the deleted helper
  bodies against `apply_profile_context`; the snapshot must carry every `self.X =`
  the old helpers performed (R3 list above).
- **F4 — Non-`Send` loader.** *Symptom:* `Task::perform` won't compile — future
  is not `Send`. *Cause:* the loader holds a borrow or a non-`Send` value across
  await. *Recovery:* keep the `spawn_blocking(move || block_on(…))` wrapper (its
  `JoinHandle` is `Send`); move owned data in.
- **F5 — Test deadlock/hang.** *Symptom:* a `tests.rs` test hangs. *Cause:* a
  Task is returned but never pumped, or the test created a nested runtime.
  *Recovery:* drive the Task via the existing `iced_test` harness, or in the test
  call `load_profile_context(...)`/`apply_profile_context(...)` directly and
  assert on the applied state.

## Reference

- Plan README + global constraints: [README.md](./README.md).
- The pattern to copy (async twin already proven): `start_tools_load` /
  `Message::ToolsLoaded` / `apply_tool_snapshot`
  ([model.rs:453-503](../../../../crates/modde-ui/src/app/model.rs)).
- The sync helpers being replaced: `model.rs` lines 123, 168, 198, 318, 540.
- Generation-guard precedent: `tool_state.load_generation`
  ([model.rs:464-469](../../../../crates/modde-ui/src/app/model.rs)).
- Prereq: [01-shared-db-handle.md](./01-shared-db-handle.md). Follow-ons:
  [03-async-diagnostics-and-tool-writes.md](./03-async-diagnostics-and-tool-writes.md),
  [04-loader-hygiene.md](./04-loader-hygiene.md).
