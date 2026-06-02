# Plan: modde-ui — drive the async sqlx DB through iced Tasks (stop blocking the UI thread)

> **Recommended Codex model for plan-set orchestration: GPT 5.5 high**
>
> Coordinating this set means holding the whole `modde-ui` event loop in one
> head: a synchronous `update`/`view`/`new` path that currently blocks on a
> shared tokio runtime for every database read, a tangle of ~20 call sites that
> all funnel through five shared `model.rs` helpers, and a correctness hazard
> (stale async snapshots clobbering newer state) that only shows up under rapid
> game/profile switching. That is complex orchestration with non-trivial
> sequencing and a regression cost on a wrong call — orchestrator role at
> complex complexity. Not `max`: the frontier risk is concentrated in **Phase
> 02**, which is routed `max` on its own; the coordination here is
> well-structured and mostly serial.

## Scope and current state

modde's storage layer is now async (`sqlx`, both SQLite and PostgreSQL). The
GUI (`crates/modde-ui`, iced 0.14) was bridged to it with a **synchronous
`block_on` shim** ([crates/modde-ui/src/app.rs:40-62](../../../../crates/modde-ui/src/app.rs)):

```rust
/// modde's storage layer is async (`sqlx`), but iced's `update(&mut self, …)`
/// handler cannot `.await`. This bridges the two: the future is driven on a
/// dedicated multi-threaded runtime … use this only on the synchronous
/// `update`/`view`/`new` paths …
pub(crate) fn block_on<F>(future: F) -> F::Output { … }
```

Every call to `block_on` on the `update`/`view`/`new` path **stalls the iced
event loop** — the window cannot paint, resize, or process input until the
query (and for PostgreSQL, a fresh TCP+TLS connection handshake) completes.
There are **~60 such call sites** across `app.rs`, `model.rs`, and `update.rs`.

The good news, established by the research pass:

- `update(&mut self, Message) -> Task<Message>` **already returns a Task**
  ([update.rs:158](../../../../crates/modde-ui/src/app/update.rs)).
- `new() -> (Self, Task<Message>)` **already returns an initial Task**
  ([update.rs:37](../../../../crates/modde-ui/src/app/update.rs); wired at
  [app.rs:913](../../../../crates/modde-ui/src/app.rs) via `iced::application(Modde::new, …)`).
- The **canonical async pattern already exists and is proven** in this app:
  `start_tools_load()` builds `Task::perform(load_tools_state(request), …)`
  ([model.rs:468](../../../../crates/modde-ui/src/app/model.rs)) → `Message::ToolsLoaded`
  → `apply_tool_snapshot()` (a synchronous apply). The async tools loader
  `load_tools_state` is literally a duplicate of the **synchronous**
  `refresh_tools_state` ([model.rs:540](../../../../crates/modde-ui/src/app/model.rs)) —
  proving the migration target already works.
- `ProfileManager` already exposes `with_db(db: ModdeDb)` and `db() -> &ModdeDb`
  ([profile/mod.rs:430-435](../../../../crates/modde-core/src/profile/mod.rs)), and
  the inner `Db` enum is `#[derive(Clone)]`
  ([db/backend.rs:252](../../../../crates/modde-core/src/db/backend.rs)) — sqlx pools
  are `Arc`-backed, so cloning a handle is cheap.

This plan migrates the remaining **synchronous-path** DB work onto iced Tasks,
and reuses one shared connection pool instead of re-opening per operation.

### What "async in iced" means here (so no one reaches for the wrong tool)

The user's research framed three iced primitives. For this work:

- **`Task::perform` / `Task::future` (Commands)** — the right tool for every
  one-off DB load/write in this plan. Returned from `update`/`new`.
- **`Subscription`** — for *continuous, passive* event streams only. The app
  already has exactly one (`external_refresh_stream`, the CLI→GUI socket at
  [app.rs:788](../../../../crates/modde-ui/src/app.rs)). **No new subscriptions are
  needed** — DB reads are one-shot, not streams. Do not model a one-shot query
  as a subscription.
- **`spawn_blocking` + `block_on`** — kept *only* inside loaders that also do
  heavy synchronous CPU/filesystem work (tool detection, conflict analysis,
  OptiScaler scanning), where the blocking work belongs off the async executor
  anyway. Pure-DB loaders drop it (Phase 04).

### The Send constraint (the reason the shim exists at all)

`Task::perform(future, …)` requires `future: Send + 'static`. The original shim
comment claims sqlx connection futures are "not `Send` under a higher-ranked
bound" — that is true for futures that **borrow `&self`/`&db` across an await**.
With a **shared, owned pool handle** (Phase 01) and **owned arguments**, sqlx
pool-level query futures *are* `Send + 'static` and can be `.await`-ed directly
in a `Task::perform` closure. Loaders that mix DB with non-`Send`/blocking work
keep the `spawn_blocking(move || block_on(…))` bridge. This distinction is the
spine of Phases 02 and 04.

## Phase table

| Phase | File | Depends on | Touches files | Can parallel with | Blocking? |
|---|---|---|---|---|---|
| 01 | [01-shared-db-handle.md](./01-shared-db-handle.md) | — | `db/mod.rs`, `app.rs`, `app/update.rs` (new), `app/tool_ops.rs`, `app/install_ops.rs` | design-only of 02 | unblocks 02, 04 |
| 02 | [02-async-profile-game-context.md](./02-async-profile-game-context.md) | 01 | `app/model.rs`, `app/update.rs`, `app.rs`, `app/tests.rs` | — | blocks 03, 04 |
| 03 | [03-async-diagnostics-and-tool-writes.md](./03-async-diagnostics-and-tool-writes.md) | 02 | `app/update.rs`, `app/model.rs`, `app/tool_settings.rs`, `app/tests.rs` | — | blocks 04 (soft) |
| 04 | [04-loader-hygiene.md](./04-loader-hygiene.md) | 01, 02 (ideally 03) | `app/tool_ops.rs`, `app/install_ops.rs`, `app/tool_settings.rs`, `app.rs`, `app/model.rs` | — | terminal |

## Parallelism layer

This refactor is **mostly serial** — Phases 02/03/04 all edit
`crates/modde-ui/src/app/update.rs` **and** `crates/modde-ui/src/app/model.rs`,
so they cannot run concurrently without merge conflicts and gating-test churn.

- **Wave 0 — Phase 01.** Starts from the current tree. Lives mostly in
  `modde-core` (`#[derive(Clone)]` on `ModdeDb`) plus threading a shared handle
  into the *already-async* UI loaders. Independent of the cluster refactor; a
  perf win on its own (no more per-op pool open, critical for PostgreSQL).
  *Phase 02 may be designed/read in parallel, but cannot compile until 01's
  shared-handle API exists.*
- **Wave 1 — Phase 02.** Unlocked by 01. The frontier phase: converts the five
  entangled `model.rs` helpers (`reload_profile`, `switch_game_context`,
  `refresh_data_tab_conflicts`, `refresh_tools_state`, `accept_game_selection`)
  and `new()` into a composite async loader + synchronous apply, and threads the
  resulting `Task` through all ~20 `update.rs` call sites. One atomic rollback
  boundary (the tree only compiles once every caller is converted).
- **Wave 2 — Phase 03.** Unlocked by 02's loader/apply convention + the
  generation-guard infrastructure. Converts `run_diagnostics_now` and the
  tool-setting **write** handlers (`UpdateToolSetting`, `ToggleTool`,
  `RestoreToolSettings`, proton/optiscaler setting writes).
- **Wave 3 — Phase 04.** Unlocked by 01+02. Hygiene: pure-DB loaders drop
  `spawn_blocking`/`block_on` for direct `.await`; the shim is re-scoped and
  documented as the CPU+DB bridge only; a guard asserts no UI-thread `block_on`
  survives. **Plan exhausted** after this wave.

## Whole-set acceptance criteria

- [ ] No `crate::app::block_on(…)` call remains on a synchronous
  `update`/`view`/`new` code path. Verified by:
  `rg -n "crate::app::block_on" crates/modde-ui/src/app/update.rs crates/modde-ui/src/app/model.rs crates/modde-ui/src/app.rs`
  returns **only** sites inside `Task::perform`/`spawn_blocking` closures (or
  the one-time startup open in `new()`), and `view.rs` has zero.
- [ ] `ModdeDb` is opened **once** for the GUI's lifetime (one shared handle in
  `Modde`), not per operation. `rg -n "ModdeDb::open\(\)|ProfileManager::open\(\)" crates/modde-ui/src`
  returns no hits on the `update`/`model`/`view` paths.
- [ ] `cargo build -p modde-ui` (default features) and
  `cargo build -p modde-ui --no-default-features` both compile clean.
- [ ] `cargo clippy -p modde-ui --all-targets -- -D warnings` is clean (CI parity).
- [ ] `cargo test -p modde-ui` is green (the existing `tests.rs` harness, incl.
  the isolated-DB tests, still passes — adjusted to the loader/apply split).
- [ ] Manual smoke (the repo's `run` skill): switching games and profiles, the
  Data tab, Diagnostics, and Tools tab all populate **without the window
  freezing** during the load; rapidly switching games does not leave a stale
  game's tools/conflicts displayed.

## Global constraints (apply to every phase)

- **Preserve test isolation.** The test suite opens **per-test isolated
  databases** (`reset_isolated_db`, isolated DB path env in
  [app/tests.rs](../../../../crates/modde-ui/src/app/tests.rs)). Do **not** introduce a
  process-global pool/`OnceCell` — it would bleak one test's DB into another.
  The shared handle lives in `Modde` (app state) and in tests is the per-test
  isolated handle.
- **The generation guard is mandatory** for any loader whose result is applied
  to shared state that the user can change again before it resolves (game,
  profile, tools). The tools path already does this via
  `tool_state.load_generation` ([model.rs:464-469](../../../../crates/modde-ui/src/app/model.rs)) —
  mirror it; never apply a snapshot whose generation is stale.
- **No new `Subscription`s.** One-shot loads use `Task::perform`.
- **CI runs `cargo clippy -p <crate> --all-targets -- --deny warnings`** —
  warnings are errors. Every phase ends clean.

## Reference

- Originating shim + its own "use only on synchronous paths" warning:
  [crates/modde-ui/src/app.rs:27-62](../../../../crates/modde-ui/src/app.rs).
- Proven async pattern to copy: `start_tools_load` / `Message::ToolsLoaded` /
  `apply_tool_snapshot` ([model.rs:453-503](../../../../crates/modde-ui/src/app/model.rs)).
- Prior plan set in this repo for shape/convention:
  [docs/src/planning/021-green-release/](../021-green-release/README.md).
- iced 0.14 async model (user research): Commands via `Task::perform`/`Task::future`
  returned from `update`; `Subscription` for passive streams only.
