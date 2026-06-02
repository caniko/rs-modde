# Phase 04 — Loader hygiene: direct `.await` for pure-DB loaders, re-scope the `block_on` shim, guard against regressions

> **Recommended Codex model: GPT 5.5 medium**
>
> Mostly mechanical cleanup against a now-stable design, but it carries one
> judgment that isn't pure boilerplate: deciding, per loader, whether its future
> is `Send + 'static` (so it can `.await` sqlx directly in a `Task::perform`
> closure) or whether it genuinely needs the `spawn_blocking` + `block_on`
> bridge because it also does non-`Send`/CPU-bound work. Get that wrong in the
> "drop the wrapper" direction and the crate stops compiling; get it wrong in the
> "keep the wrapper" direction and you've left a thread-pool hop in a hot path
> for no reason. Moderate complexity, leaf/sub-agent role, established context —
> `medium` is the right tier; not `low`, because the `Send` call and the
> regression-guard design need real reasoning, and not `high`, because the risky
> architectural decisions were all made in Phases 01-03.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. Depends on Phases 01 and 02
(shared handle + async cluster); ideally also on 03. Rebase onto the latest of
those. This phase edits the loader internals in `tool_ops.rs`/`install_ops.rs`/
`tool_settings.rs` and the shim doc in `app.rs` — touch `model.rs`/`update.rs`
only to delete now-dead code.

## Goal

The only `crate::app::block_on` calls that remain in the crate are inside
`spawn_blocking` closures (or genuinely-blocking loaders that also do CPU work)
and the single one-time startup open in `new()`. Pure-DB loaders — ones whose
work is only sqlx queries with owned arguments — `.await` the shared pool
directly inside their `Task::perform` futures, with no `spawn_blocking`/`block_on`
hop. The shim's doc comment is updated to describe its real (narrow) remaining
role, and a cheap guard prevents a future contributor from reintroducing a
render-thread `block_on`. Success looks like: the guard check passes, the crate
is smaller, and nothing on the UI thread blocks on the DB.

## Why this matters now

After Phases 01-03 the architecture is correct but the implementation is uneven:
some loaders still wrap pure sqlx queries in `spawn_blocking(move ||
block_on(…))` purely as a historical Send workaround that the shared pool
(Phase 01) made unnecessary. With an owned `ModdeDb` handle and owned args, sqlx
pool query futures are `Send + 'static` and can be awaited directly — simpler,
one fewer thread hop, and it lets `clippy` see the real async shape. Meanwhile
the `block_on` shim's doc still says "use … on the synchronous `update`/`view`/`new`
paths" ([app.rs:37-39](../../../../crates/modde-ui/src/app.rs)) — which, after this plan,
is exactly what must **never** happen again. Left undocumented and unguarded,
the next contributor copies the old pattern and silently reintroduces a UI
freeze.

## Out of scope

- Any behaviour change. This is a refactor: same results, fewer thread hops.
- Loaders that interleave DB with heavy CPU/filesystem work — `load_tools_state`
  / `load_tools_state_blocking` ([tool_ops.rs:90](../../../../crates/modde-ui/src/app/tool_ops.rs)),
  the diagnostics loader (Phase 03), anything calling `analyze_profile_state`,
  `scan_optiscaler_install`, the staging-tree walk, or image decoding —
  **keep** `spawn_blocking` + `block_on`. The blocking work belongs off the
  async executor regardless of the pool; do not "optimize" these into bare
  `.await`.
- The CLI and `modde-core` (untouched since Phase 01).

## Plan

1. **Classify every remaining `block_on` site.** Run
   `rg -n "block_on" crates/modde-ui/src` and tag each as either:
   - **(P) pure-DB** — body is only sqlx calls + owned data (e.g. the executable
     load/save in `tool_ops.rs:96-101/519-523/535-537`, browse-install profile
     lookups in `install_ops.rs:63-116` that are just DB reads), or
   - **(C) CPU+DB** — also does detection/analysis/fs walking/serde-heavy work.
2. **Convert (P) loaders to direct `.await`.** Change the loader to
   `async fn load_x(db: ModdeDb, …) -> …` that drops the `spawn_blocking`/
   `block_on` and `.await`s the queries on the shared handle. Confirm the
   resulting future is `Send + 'static` (it is, with owned `ModdeDb` + owned
   args). The `Task::perform` call site is unchanged.
3. **Leave (C) loaders alone**, but replace any `ModdeDb::open()` they might
   still do with the passed handle (should already be done in Phase 01).
4. **Re-scope the shim.** Rewrite the `block_on` doc comment
   ([app.rs:27-62](../../../../crates/modde-ui/src/app.rs)) to state its remaining purpose:
   "Drive a sqlx future to completion **inside a `spawn_blocking` closure** (or
   the one-time startup open). **Never call on the `update`/`view`/`new` render
   path** — use `Task::perform` instead." Keep the implementation.
5. **Add a regression guard.** Cheapest effective option — a test in
   `tests.rs` (or a tiny `#[test]`) that greps the source and fails if
   `crate::app::block_on` appears in `view.rs`, or on a `&mut self` helper in
   `model.rs` outside an allow-listed set. Alternatively, a comment-gated CI
   `rg` step. Prefer the in-tree test so it runs with `cargo test`. Example:
   read `model.rs`/`view.rs` at test time and assert no `block_on` outside lines
   annotated `// off-thread:` . Keep it simple and documented.
6. **Delete dead code.** Remove any helper left unused after Phases 02-03
   (`rg` for zero-caller `pub(super) fn`s; let `cargo build`'s
   `dead_code`/clippy guide you).
7. **Verify** (Acceptance criteria).

## Acceptance criteria

- [ ] `rg -n "crate::app::block_on" crates/modde-ui/src/app/view.rs` → **zero hits**.
- [ ] `rg -n "crate::app::block_on" crates/modde-ui/src/app/update.rs
  crates/modde-ui/src/app/model.rs` → hits only inside `Task::perform`/
  `spawn_blocking` closures or the `new()` startup open (verify by reading each).
- [ ] At least the executable load/save and browse-install DB-only loaders use
  direct `.await` (no `spawn_blocking`) — `rg -n "spawn_blocking" tool_ops.rs
  install_ops.rs` shows it remains only on CPU+DB loaders (tools, wabbajack
  install, stock snapshot).
- [ ] The shim doc comment no longer instructs use on the render path.
- [ ] A regression guard exists and passes (and **fails** if you temporarily add
  a `block_on` to `view.rs` — prove it once, then revert).
- [ ] `cargo build -p modde-ui` and `--no-default-features` compile.
- [ ] `cargo clippy -p modde-ui --all-targets -- -D warnings` clean.
- [ ] `cargo test -p modde-ui` green.
- [ ] Whole-set acceptance criteria in [README.md](./README.md) all satisfied.

## Files likely touched

- `crates/modde-ui/src/app/tool_ops.rs`, `crates/modde-ui/src/app/install_ops.rs`,
  `crates/modde-ui/src/app/tool_settings.rs` — (P) loaders → direct `.await`.
- `crates/modde-ui/src/app.rs` — shim doc comment.
- `crates/modde-ui/src/app/tests.rs` — regression guard test; dead-code-driven
  removals.
- `crates/modde-ui/src/app/model.rs`, `crates/modde-ui/src/app/update.rs` —
  delete now-unused helpers only.

## Pitfalls

- **Symptom:** `Task::perform` stops compiling after dropping `spawn_blocking`
  ("future is not `Send`"). **Cause:** the loader you converted was actually
  (C)-class, or it captured a non-`Send` value (a `Rc`, a borrow). **Recovery:**
  restore the `spawn_blocking(move || block_on(…))` wrapper — it was (C), not
  (P). Owned `ModdeDb` + owned `String`/`GameId` args are `Send`; `&self`
  borrows and `Rc`/`RefCell` are not.
- **Symptom:** a converted loader now runs the DB query on iced's async executor
  and a *different* heavy call beside it stalls the executor thread. **Cause:**
  you misclassified — there was hidden CPU work. **Recovery:** keep CPU work in
  `spawn_blocking`; only the pure-DB portion may `.await`.
- **Symptom:** the regression guard is flaky or over-broad (fails on legitimate
  `spawn_blocking` `block_on`). **Cause:** the grep matched off-thread uses.
  **Recovery:** scope the guard to `view.rs` (must be zero) and to `&mut self`
  helpers in `model.rs`; allow-list the `Task::perform`/`spawn_blocking` lines
  explicitly (e.g. by an `// off-thread:` marker).
- **Symptom:** `dead_code` warnings become errors under `-D warnings` after
  deletions exposed more unused fns. **Cause:** cascade. **Recovery:** delete the
  newly-unused fns too, or `#[cfg(test)]`/`pub(super)`-gate genuinely-needed ones.

## Reference

- Plan README + whole-set acceptance: [README.md](./README.md).
- The shim being re-scoped: [crates/modde-ui/src/app.rs:27-62](../../../../crates/modde-ui/src/app.rs).
- (C)-class loader to leave intact: `load_tools_state_blocking`
  ([tool_ops.rs:90+](../../../../crates/modde-ui/src/app/tool_ops.rs)); stock-snapshot
  `Handle::current().block_on` inside `spawn_blocking`
  ([update.rs:2073/2110](../../../../crates/modde-ui/src/app/update.rs)) — already correct.
- Prereqs: [01-shared-db-handle.md](./01-shared-db-handle.md),
  [02-async-profile-game-context.md](./02-async-profile-game-context.md),
  [03-async-diagnostics-and-tool-writes.md](./03-async-diagnostics-and-tool-writes.md).
