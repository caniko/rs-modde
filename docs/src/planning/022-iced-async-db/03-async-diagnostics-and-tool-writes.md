# Phase 03 — Async diagnostics + tool-setting write handlers

> **Recommended Codex model: GPT 5.5 high**
>
> The loader→message→apply convention and the generation guard already exist
> after Phase 02, so this is not frontier work — but it is more than mechanical.
> Diagnostics is a CPU-and-DB job feeding a three-state machine (Idle / Complete
> / Error), and the tool-setting writes are **read-modify-write-then-reload**
> sequences where getting the ordering or the post-write refresh wrong silently
> persists the wrong value or shows a stale toggle. Multiple `update.rs` arms,
> non-trivial sequencing, established pattern to follow — orchestrator role at
> complex complexity. A weaker model tends to fire the reload before the write
> commits, producing intermittent "my change didn't stick" bugs.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. **Phase 02 must land
first** — this phase reuses the `ProfileContextSnapshot` loader, the
`apply_*`/generation-guard convention, and the now-async `reload`/tools path
that Phase 02 establishes. You will edit `update.rs` and `model.rs`, the same
files Phase 02 touched, so rebase onto Phase 02 before starting.

## Goal

Running diagnostics and changing a tool setting never block the iced event loop.
`run_diagnostics_now` becomes an async loader feeding `Message::DiagnosticsComputed`
+ a synchronous apply into the existing `DiagnosticsState`. The tool-setting
write handlers (`UpdateToolSetting`, `ToggleTool`, `RestoreToolSettings`, and the
proton/optiscaler setting writes) perform their DB write off-thread and then
trigger a tools refresh through the Phase-02 path. Success looks like: clicking
"Run diagnostics" shows a running state and repaints; toggling a tool persists
and the toggle reflects the committed value with no UI freeze.

## Why this matters now

`run_diagnostics_now` ([model.rs:352-419](../../../../crates/modde-ui/src/app/model.rs))
opens a `ProfileManager`, loads hidden files and plugin order, walks the staging
tree for symlink integrity, and runs the full diagnostics engine — **all
synchronously** from the `RunDiagnostics` arm ([update.rs:2274](../../../../crates/modde-ui/src/app/update.rs)).
For a large profile this freezes the window for seconds. The tool-setting writes
([update.rs:2514-2607 `UpdateToolSetting`], [2640-2665 `ToggleTool`],
[3055-3191 restore/proton/optiscaler]) each `block_on(ModdeDb::open())`, read the
config, mutate it, `block_on(save_tool_config*)`, then often
`block_on(generate_tool_configs)` — a multi-query render-thread stall on every
checkbox click. `current_tool_config`/`save_tool_settings`
([tool_settings.rs:378-421](../../../../crates/modde-ui/src/app/tool_settings.rs)) are the
synchronous read/write helpers underneath, also used at
[update.rs:2355/2368/2435/2439](../../../../crates/modde-ui/src/app/update.rs).

## Out of scope

- The profile/game read cluster — done in Phase 02. (If a write handler ends by
  calling the Phase-02 reload, just return that Task.)
- Dropping `spawn_blocking`/`block_on` from CPU+DB loaders — Phase 04. Here,
  diagnostics keeps the bridge (it is CPU-heavy: the staging-tree walk and the
  diagnostics engine belong off the async executor).
- The tool **apply/revert** handlers (`ApplyTool`, `RevertTool`,
  `ActivateOptiScaler`, etc.) — those are **already async** via `Task::perform`
  (`apply_tool_for_game` et al., [tool_ops.rs:363+](../../../../crates/modde-ui/src/app/tool_ops.rs));
  Phase 01 already handed them the shared handle. Leave their structure alone.

## Plan

1. **Diagnostics loader.** Move the body of `run_diagnostics_now` into an
   `async fn load_diagnostics(db: ModdeDb, profile: Profile) ->
   Result<DiagnosticsComputed, String>` wrapping the work in
   `spawn_blocking(move || block_on(…))` (CPU-heavy). It returns an owned struct
   carrying the `DiagnosticsReport` (entries, integrity summary), plus the
   `data_tab_conflicts` and `missing_store_mod_count` it computes as a side
   effect today.

2. **Diagnostics message + apply.** Add `Message::DiagnosticsComputed {
   generation: u64, result: Result<DiagnosticsComputed, String> }`. Add a
   `diagnostics_generation` (or reuse `context_generation` semantics) so a reload
   that changed the active profile invalidates an in-flight diagnostics run. The
   apply writes `DiagnosticsState::Complete{…}`/`Error(…)` and the data-tab
   fields, and sets the status message — exactly the synchronous tail today.

3. **Convert the `RunDiagnostics` arm** ([update.rs:2274](../../../../crates/modde-ui/src/app/update.rs))
   to: validate a profile is loaded (the early-return guards at
   [model.rs:353-359](../../../../crates/modde-ui/src/app/model.rs) stay synchronous), set
   `DiagnosticsState` to a running/in-progress variant (add one if only
   Idle/Complete/Error exist — the view should show "Running…"), bump the
   generation, and `return Task::perform(load_diagnostics(self.db.clone(),
   profile), …)`. Note `SwitchView(Diagnostics)` re-dispatches via
   `return self.update(Message::RunDiagnostics)`
   ([update.rs:218/249](../../../../crates/modde-ui/src/app/update.rs)) — that keeps working
   since it now returns the Task.

4. **Tool-setting writes — async write + refresh.** For `UpdateToolSetting`,
   `ToggleTool`, `RestoreToolSettings`, and the proton/optiscaler setting-write
   arms: extract the read-modify-write into an `async fn save_tool_setting_*(db:
   ModdeDb, …) -> Result<(), String>` (mirror `save_executable_for_game`,
   [tool_ops.rs:516](../../../../crates/modde-ui/src/app/tool_ops.rs)), returning a
   `Message::ToolSettingWritten { tool_id, result }`. The arm applies optimistic
   UI state if appropriate, then `return Task::perform(write_fut, …)`. The
   `ToolSettingWritten` apply, on `Ok`, returns the Phase-02 **tools reload
   Task** (`start_tools_load`) so the committed value is re-read; on `Err`, it
   reverts the optimistic state and sets an error status.

5. **`current_tool_config`/`save_tool_settings`** ([tool_settings.rs:378-421](../../../../crates/modde-ui/src/app/tool_settings.rs)):
   these are sync wrappers around `block_on`. Convert the **on-render-thread**
   call sites ([update.rs:2355/2368/2435/2439](../../../../crates/modde-ui/src/app/update.rs))
   to go through the async write fns above. The copies called from inside
   already-async loaders (`tool_ops.rs:45/61/141`) are off-thread — leave for
   Phase 04 (they become `.await` or keep `block_on` per Phase 04's rule).

6. **Generation guard for writes.** A toggle followed by a fast second toggle
   must not let the first write's reload clobber the second. Reuse the tools
   `load_generation` so the later `ToolsLoaded` wins.

7. **Tests.** Extend `tests.rs`: a diagnostics test that drives the Task and
   asserts `DiagnosticsState::Complete`; a tool-toggle test that drives the write
   Task + reload and asserts the persisted value. Use the existing iced_test
   harness ([tests.rs:105](../../../../crates/modde-ui/src/app/tests.rs)).

## Acceptance criteria

- [ ] `run_diagnostics_now` is gone (or is a thin sync guard that returns a
  Task); `rg -n "block_on" crates/modde-ui/src/app/model.rs` shows no
  diagnostics `block_on` on a `&mut self` path.
- [ ] `RunDiagnostics`, `UpdateToolSetting`, `ToggleTool`, `RestoreToolSettings`,
  and the proton/optiscaler setting-write arms each `return Task::perform(…)`;
  none call `block_on` inline.
- [ ] On-render-thread uses of `current_tool_config`/`save_tool_settings`
  ([update.rs:2355/2368/2435/2439]) are removed in favour of async write fns.
- [ ] A toggle persists across a reload (new tool-toggle test green); a stale
  reload from a superseded toggle is dropped by the generation guard (asserted).
- [ ] `cargo build -p modde-ui` and `--no-default-features` compile.
- [ ] `cargo clippy -p modde-ui --all-targets -- -D warnings` clean.
- [ ] `cargo test -p modde-ui` green.
- [ ] Manual (`run` skill): "Run diagnostics" on a large profile shows a running
  state immediately (no freeze) then results; toggling a tool sticks and the UI
  never stalls.

## Files likely touched

- `crates/modde-ui/src/app/model.rs` — diagnostics loader + apply; remove sync
  `run_diagnostics_now` body.
- `crates/modde-ui/src/app/update.rs` — `RunDiagnostics` + tool-setting write arms.
- `crates/modde-ui/src/app/tool_settings.rs` — async write fns; keep off-thread
  copies for Phase 04.
- `crates/modde-ui/src/app.rs` — `Message::DiagnosticsComputed`,
  `Message::ToolSettingWritten`.
- `crates/modde-ui/src/app/tests.rs` — diagnostics + toggle tests.

## Pitfalls

- **Symptom:** a toggled setting reverts on the next render. **Cause:** the
  reload Task fired before/parallel to the write committing, re-reading the old
  value. **Recovery:** chain — only fire the tools reload from the
  `ToolSettingWritten` **Ok** apply, after the write resolved; do not batch the
  write and the reload to run concurrently.
- **Symptom:** diagnostics shows stale results for the previously-active profile.
  **Cause:** no generation guard; the user switched profiles mid-run.
  **Recovery:** bump + check a generation in `DiagnosticsComputed` like Phase 02.
- **Symptom:** the view has no "running" state, so the user sees the old report
  frozen during the async run. **Cause:** `DiagnosticsState` only has
  Idle/Complete/Error. **Recovery:** add a `Running` variant and render a
  spinner/label in `crate::views::diagnostics::view`.
- **Symptom:** `Task::perform` won't compile (future not `Send`). **Cause:**
  same as Phase 02 F4. **Recovery:** keep the `spawn_blocking(move ||
  block_on(…))` wrapper.

## Reference

- Plan README + global constraints: [README.md](./README.md).
- Sync code being replaced: `run_diagnostics_now`
  ([model.rs:352](../../../../crates/modde-ui/src/app/model.rs)); tool-setting writes
  ([update.rs:2514/2640/3055-3191](../../../../crates/modde-ui/src/app/update.rs));
  `current_tool_config`/`save_tool_settings`
  ([tool_settings.rs:378/405](../../../../crates/modde-ui/src/app/tool_settings.rs)).
- Async write precedent: `save_executable_for_game`
  ([tool_ops.rs:516](../../../../crates/modde-ui/src/app/tool_ops.rs)).
- Prereq: [02-async-profile-game-context.md](./02-async-profile-game-context.md).
  Follow-on: [04-loader-hygiene.md](./04-loader-hygiene.md).
