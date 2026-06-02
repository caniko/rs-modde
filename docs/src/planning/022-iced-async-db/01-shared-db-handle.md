# Phase 01 — Hold one shared DB handle in app state (stop re-opening the pool per operation)

> **Recommended Codex model: GPT 5.5 high**
>
> This is a small diff with an outsized blast radius and one genuinely
> non-trivial design call: where the shared pool lives. A process-global
> `OnceCell` is the obvious move and the **wrong** one — it would silently break
> the test suite's per-test isolated databases. The correct choice (a handle in
> app state, cloned per loader, injectable in tests) requires understanding the
> `Send`/`Clone` story of the sqlx pools and the existing test-isolation
> harness. Cross-crate (`modde-core` + `modde-ui`) edit with a `Clone`/`Send`
> judgment that everything downstream depends on — orchestrator role, complex
> complexity. A weaker model tends to reach for the global and pass the tests
> locally while corrupting CI's parallel test runs.

## Working tree

`/data/nvme0/can/Projects/rs-modde` — the main repo, branch `trunk`. Same repo
as every other phase in this set. This phase is the prerequisite for Phase 02;
land it first.

## Goal

The GUI opens a `ModdeDb` **exactly once** for its lifetime, stores that handle
in the `Modde` struct, and every database operation reuses a cheap `Clone` of
it (an `Arc`-backed sqlx pool) instead of calling `ModdeDb::open()` /
`ProfileManager::open()` per operation. The existing async loaders
(`load_tools_state`, `load_executables_for_game`, the install/wabbajack flows)
take the shared handle. Per-test database isolation is preserved. Success looks
like: `rg -n "ModdeDb::open\(\)|ProfileManager::open\(\)" crates/modde-ui/src`
returns no operational call sites (only the single startup open and test
construction).

## Why this matters now

Today every DB touch does `crate::app::block_on(ModdeDb::open())` or
`ProfileManager::open()` first — e.g.
[tool_ops.rs:70](../../../../crates/modde-ui/src/app/tool_ops.rs),
[model.rs:549](../../../../crates/modde-ui/src/app/model.rs),
[update.rs:46](../../../../crates/modde-ui/src/app/update.rs). For SQLite that opens
the file and runs the `PRAGMA` setup each time; for **PostgreSQL** (now a
supported backend) it performs a **fresh TCP + TLS connection handshake on
every single query**. The Tools tab alone opens the pool, then loops
`all_tools()` issuing `load_tool_config` + `load_applied_files` +
`list_tool_setting_history` per tool ([model.rs:589-749](../../../../crates/modde-ui/src/app/model.rs)) —
dozens of handshakes for one tab render. Making the UI async (Phase 02) without
fixing this just moves a pile of redundant connection setup off the render
thread instead of eliminating it. A shared pool is the foundation that makes the
later async loaders actually cheap.

## Out of scope

- Do **not** convert the synchronous `model.rs` helpers or `update.rs` arms to
  Tasks yet — that is Phase 02. This phase only changes *where the handle comes
  from*, not *which thread the work runs on*. The five sync helpers keep using
  `block_on`, just against the shared handle.
- Do **not** touch the CLI (`crates/modde-cli`). CLI processes are short-lived;
  per-op open there is cheap and its integration tests rely on it. Out of scope.
- Do **not** introduce a process-global `OnceCell`/`static` pool (see Pitfalls).
- Do **not** change `ModdeDb::open*` resolution logic (env → settings → sqlite).

## Plan

1. **Make `ModdeDb` cloneable.** In
   [crates/modde-core/src/db/mod.rs:131-133](../../../../crates/modde-core/src/db/mod.rs)
   add `#[derive(Clone)]` to `pub struct ModdeDb` (the inner `Db` enum is already
   `#[derive(Clone)]` at [db/backend.rs:252](../../../../crates/modde-core/src/db/backend.rs),
   so this derives trivially). Add `Debug` too if not already present. Confirm
   the derive compiles: `cargo build -p modde-core`.

2. **Add the handle to app state.** In the `Modde` struct
   ([crates/modde-ui/src/app.rs:131-207](../../../../crates/modde-ui/src/app.rs)) add a
   field `pub(crate) db: modde_core::db::ModdeDb`.

3. **Open once in `new()`.** In `Modde::new()`
   ([update.rs:37-48](../../../../crates/modde-ui/src/app/update.rs)) replace the current
   `block_on(ProfileManager::open())…` with a **single** startup open:
   ```rust
   let db = crate::app::block_on(modde_core::db::ModdeDb::open())
       .expect("failed to open modde database at startup");
   let all_profiles = crate::app::block_on(
       ProfileManager::with_db(db.clone()).list()
   ).unwrap_or_default();
   ```
   This one-time blocking open runs before the window is shown, so it does not
   stall the event loop. Store `db` in the struct literal. (Phase 02 will move
   the `list()` itself onto a Task; for now it stays blocking — it is the same
   startup cost as today.)

4. **Thread the handle into the already-async loaders.** These run off-thread
   already (via `spawn_blocking`), so this is purely "stop calling `open()`
   inside them; take a `ModdeDb` argument and build `ProfileManager::with_db`
   from it":
   - `tool_ops.rs`: `load_tools_state` / `load_tools_state_blocking`
     ([tool_ops.rs:90-…](../../../../crates/modde-ui/src/app/tool_ops.rs)) and
     `load_executables_for_game` ([tool_ops.rs:96](../../../../crates/modde-ui/src/app/tool_ops.rs)) —
     add a `db: ModdeDb` param; replace the internal
     `block_on(ModdeDb::open())` with the passed handle. Update their callers
     `start_tools_load` / `start_executables_load`
     ([model.rs:468](../../../../crates/modde-ui/src/app/model.rs),
     [model.rs:525](../../../../crates/modde-ui/src/app/model.rs)) to pass `self.db.clone()`.
   - The other `async fn` loaders in `tool_ops.rs` (`apply_tool_for_game`,
     `revert_tool_for_game`, `deactivate_optiscaler_for_game`,
     `restore_tool_settings_for_game`, `save_executable_for_game`,
     `remove_executable_for_game`, `run_saved_executable_for_game`,
     `install_selected_*`) and `install_ops.rs`
     (`run_browse_install`, `run_wabbajack_install_for_ui`,
     `download_wabbajack_source`): add the `db: ModdeDb` param and stop opening.
     Update their `Task::perform` call sites in `update.rs` to pass
     `self.db.clone()`.
   - `ToolLoadRequest` ([state.rs](../../../../crates/modde-ui/src/app/state.rs)) may be a
     natural place to carry the handle for the tools loader — your call; a plain
     extra arg is fine.

5. **Point the synchronous helpers at the shared handle (no async change yet).**
   In `model.rs`, `reload_profile`/`switch_game_context`/
   `refresh_data_tab_conflicts`/`run_diagnostics_now`/`refresh_tools_state`
   currently do `block_on(ProfileManager::open())` / `block_on(ModdeDb::open())`.
   Replace each with `ProfileManager::with_db(self.db.clone())` /
   `self.db.clone()` (still wrapped in `block_on` for the queries — the thread
   change is Phase 02). Same for the `app.rs` helpers `load_hidden_files` /
   `load_active_plugins` ([app.rs:209-243](../../../../crates/modde-ui/src/app.rs)) — they
   take `&ProfileManager`, so callers just pass the shared-handle PM.

6. **Tests.** In [app/tests.rs](../../../../crates/modde-ui/src/app/tests.rs), wherever a
   `Modde` is constructed for a test, inject the test's isolated `ModdeDb` into
   the new `db` field (open it against the isolated path the harness already
   sets up). Keep `reset_isolated_db` working. The test-only `block_on` sites
   that construct `ModdeDb`/`ProfileManager` directly are fine to leave.

7. **Verify** (see Acceptance criteria).

## Acceptance criteria

- [ ] `#[derive(Clone)]` is present on `ModdeDb`; `cargo build -p modde-core` clean.
- [ ] `Modde` has a `db: ModdeDb` field, populated once in `new()`.
- [ ] `rg -n "ModdeDb::open\(\)|ProfileManager::open\(\)" crates/modde-ui/src/app`
  returns **only**: the single open in `new()`, and test-construction sites in
  `tests.rs`. No hits in `tool_ops.rs`, `install_ops.rs`, `model.rs` operational
  paths, or `update.rs` handlers.
- [ ] `cargo build -p modde-ui` and `cargo build -p modde-ui --no-default-features` compile.
- [ ] `cargo clippy -p modde-ui --all-targets -- -D warnings` clean.
- [ ] `cargo test -p modde-ui` green — **including** the isolated-DB tests
  (`reset_isolated_db` path); run with `--test-threads` default (parallel) to
  prove isolation holds.
- [ ] Behaviour unchanged: the app still loads profiles/tools exactly as before
  (this phase is a handle-plumbing change, not a behaviour change).

## Files likely touched

- `crates/modde-core/src/db/mod.rs` — `#[derive(Clone)]` on `ModdeDb`.
- `crates/modde-ui/src/app.rs` — `db` field on `Modde`; `load_hidden_files` /
  `load_active_plugins` callers already take `&ProfileManager`.
- `crates/modde-ui/src/app/update.rs` — `new()` startup open; pass `self.db.clone()`
  into `Task::perform` loader calls.
- `crates/modde-ui/src/app/model.rs` — `start_tools_load`/`start_executables_load`
  pass the handle; sync helpers use `self.db.clone()`.
- `crates/modde-ui/src/app/tool_ops.rs`, `crates/modde-ui/src/app/install_ops.rs` —
  loaders take `db: ModdeDb`.
- `crates/modde-ui/src/app/tests.rs` — inject isolated handle into `Modde`.

## Pitfalls

- **Symptom:** tests pass locally but flake/fail in CI with cross-test data
  bleed (a profile from test A appears in test B). **Cause:** you used a
  process-global `static`/`OnceCell<ModdeDb>`, which the parallel test runner
  shares across the per-test isolated DBs. **Recovery:** the handle must be an
  app-state field, opened per `Modde` instance; tests inject their own isolated
  handle. There is **no** global.
- **Symptom:** `ModdeDb` won't derive `Clone`. **Cause:** a field isn't `Clone`.
  **Recovery:** the only field is `db: Db`, which is already `Clone`; if a new
  field was added, wrap it in `Arc` or make it `Clone`.
- **Symptom:** a loader future stops being `Send` after you add the handle.
  **Cause:** you held `&self.db` (a borrow) across an await instead of moving an
  owned `self.db.clone()` into the closure. **Recovery:** clone before the
  closure and `move` the owned handle in.
- **Symptom:** the one-time `new()` open panics on a misconfigured PostgreSQL
  URL, killing startup with no UI. **Cause:** `.expect()` on `open()`.
  **Recovery:** acceptable for this phase (matches today's behaviour — the app
  can't run without a DB), but leave a `// TODO(phase-02): surface as a Task
  error state` so Phase 02 can show it in the status bar instead of panicking.

## Reference

- Plan README: [README.md](./README.md).
- `ProfileManager::with_db` / `db()`:
  [crates/modde-core/src/profile/mod.rs:430-435](../../../../crates/modde-core/src/profile/mod.rs).
- `Db` clone derive: [crates/modde-core/src/db/backend.rs:252](../../../../crates/modde-core/src/db/backend.rs).
- Next phase consuming this handle:
  [02-async-profile-game-context.md](./02-async-profile-game-context.md).
