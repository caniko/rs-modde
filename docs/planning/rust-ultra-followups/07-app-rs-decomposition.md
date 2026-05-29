# Phase 07 — Code reorg: decompose `modde-ui/src/app.rs`

> **Recommended Codex model: GPT 5.5 high**
>
> Complex and delicate, but mechanical and compile-driven with a strong test net
> (106 lib tests + clippy). The risk isn't novelty — it's volume and the Rust
> visibility rules: moving private functions and `impl Modde` methods into child
> modules requires getting `pub(super)`/`pub(crate)` and explicit `use super::{…}`
> right (no `wildcard_imports`), one cohesive cluster at a time, recompiling
> between each. `medium` would likely stall on the visibility cascade or leave a
> red tree; `high` carries the context of a 7.4k-line file and the patience for
> per-cluster extraction. Not `max`: there's no behavior judgment, the compiler
> enforces correctness, and a botched move fails loudly rather than shipping a
> subtle bug. Orchestrator role over one file.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. Run **after Phase 04** so the
`game_id` call sites inside `app.rs` are already `&GameId`-typed and you're moving
final code, not code that 04 will re-touch. Can overlap Phases 05/06 (disjoint
crate: 07 is `modde-ui`, 05/06 are `modde-core`) **only if 04 has landed**.

## Goal

`crates/modde-ui/src/app.rs` (currently ~7,417 lines) is decomposed into a
`src/app/` module tree of focused files (target 80–500 lines each), with behavior
**byte-for-byte unchanged** — the giant free-function clusters and the two large
`impl Modde` blocks are split into cohesive child modules, `app.rs` becomes a
slim coordinator that declares the submodules and holds the `Modde` struct
definition, and the whole crate still compiles and passes its 106 lib tests.

## Why this matters now

`rust-ultra`'s code-reorg concern flagged `app.rs` as the worst readability
offender in the workspace. The test module was already extracted (9,895 → 7,417),
but the production bulk remains: ~1,400 lines of cohesive free-function clusters
(tool-settings, tool-ops, install-ops) plus two `impl Modde` blocks (~970 and
~3,650 lines) and the `FOMODWizardState` impl (~580 lines). A 7.4k-line file
doesn't fit an LLM context window in full and is hard for humans to navigate. The
extraction is "monumental, plan-worthy" precisely because production code (unlike
the test module) needs visibility threading: child modules can read `Modde`'s
private fields (they're descendants of the `app` module), but private *functions*
moved into a child need `pub(super)`/`pub(crate)` to be called back from `app.rs`,
and `app.rs` must import them explicitly (`clippy::wildcard_imports` forbids
`use super::*` in non-test code).

## Out of scope

- Any behavior change, signature change to public items, or logic edit. Pure moves
  + visibility/import fixups.
- Re-extracting the test module (already in `app/tests.rs`).
- Splitting other modde-ui files (the `views/` tree is already reasonable).
- "Improving" the moved code while moving it — move first; any cleanup is a
  separate later change.

## Plan

Extract **one cohesive cluster per commit**, recompiling + testing between each.
Suggested order (lowest-risk first — pure free-function clusters before `impl`
blocks):

1. **Map the file.** `rg -n '^(pub )?(fn|async fn|struct|enum|impl|mod) '
   crates/modde-ui/src/app.rs` to get the current item map (line numbers will have
   shifted from earlier work). Identify the clusters: tool-settings free fns,
   tool-ops free fns (async loaders + executable ops), install/wabbajack free fns,
   the state types (`View`, `SidebarGroup`, `ToolState`, `ExecutableUiEntry`,
   drafts), `FOMODWizardState` + its impls, and the two `impl Modde` blocks.
2. **Set up the module dir.** `app.rs` stays at `src/app.rs`; child modules live in
   `src/app/` and are declared in `app.rs` as `mod tool_settings;` etc. (Rust 2018+
   resolves `mod x;` in `app.rs` to `src/app/x.rs`.) `app/tests.rs` already proves
   this layout works.
3. **Extract free-function clusters first** (e.g. `app/tool_settings.rs`,
   `app/tool_ops.rs`, `app/install_ops.rs`). For each:
   - Move the functions verbatim into the new file.
   - Mark each moved fn `pub(super)` (visible to `app`) — or `pub(crate)` if used
     outside `app` too. Keep `pub fn` ones `pub`.
   - In the new file, add explicit `use super::{TypesAndFnsItNeeds};` and external
     `use` lines (copy what it referenced). **No `use super::*`** — list items.
   - In `app.rs`, add `mod <cluster>;` and `use self::<cluster>::{the fns app.rs calls};`.
   - `cargo check -p modde-ui --all-targets` green, then `cargo clippy ... -D
     warnings`, then commit.
4. **Extract the state types** (`app/state.rs` or per-type files) and
   `FOMODWizardState` (+ its `Debug`/`Clone`/`Default`/main impl) into
   `app/fomod_wizard_state.rs`. These are types with their impls — one type per
   file per the reorg guideline. Re-export anything `views/` imports
   (`pub use` from `app.rs`) so external paths don't change.
5. **Extract the `impl Modde` blocks** into topical files (e.g. `app/update.rs`,
   `app/view.rs`, `app/tools_ui.rs`) as additional `impl Modde { ... }` blocks. A
   child module may `impl Modde` and access `Modde`'s private fields because it's a
   descendant of `app`. Move methods in cohesive groups; recompile between groups.
   This is the bulk — go in small commits.
6. After each extraction, run `cargo test -p modde-ui` (106 lib tests). The tests
   live in `app/tests.rs` and exercise the UI behavior — they're your behavior
   oracle. Keep `app.rs` documented with a `//!` header describing the module tree.

## Acceptance criteria

- [ ] `crates/modde-ui/src/app.rs` is materially smaller (target: under ~1,500
      lines — the `Modde` struct, the `mod`/`use` wiring, `main`/entry, and any
      genuinely-coordinator code); the bulk lives in `src/app/*.rs` files each
      ≤ ~500 lines.
- [ ] No `use super::*` in any non-test `app/*.rs` file (`rg -n 'use super::\*'
      crates/modde-ui/src/app` returns only `app/tests.rs`).
- [ ] `cargo test -p modde-ui` passes the same 106 lib tests (behavior unchanged).
- [ ] `cargo clippy -p modde-ui --all-targets -- -D warnings` clean; `cargo fmt --check` clean.
- [ ] Public paths used by `modde-ui/src/views/*` and `modde-cli` still resolve
      (re-exports from `app.rs` preserve them) — `cargo check --all-targets --workspace` green.
- [ ] Each extraction is its own atomic commit with a `refactor(ui): extract …` message.

## Files likely touched

- `crates/modde-ui/src/app.rs` (shrinks; gains `mod`/`use`/`pub use` wiring + `//!`).
- New `crates/modde-ui/src/app/*.rs` (tool_settings, tool_ops, install_ops, state,
  fomod_wizard_state, and the impl-block files — exact names your call).

## Pitfalls

- **Symptom:** `function is private` when `app.rs` calls a moved fn. **Cause:**
  moved fns default to module-private in the child. **Recovery:** mark them
  `pub(super)` (or `pub(crate)`); add the explicit `use self::<mod>::{…}` in `app.rs`.
- **Symptom:** `clippy::wildcard_imports` error. **Cause:** you reached for `use
  super::*` to pull in many items. **Recovery:** enumerate the imports; it's
  verbose but required (pedantic is on, and the rust-ultra pass confirmed wildcard
  is denied outside `#[cfg(test)]`).
- **Symptom:** `views/*` or `modde-cli` no longer finds a type after you moved it.
  **Cause:** the type's path changed. **Recovery:** `pub use self::<mod>::Type;`
  from `app.rs` so the external path is stable.
- **Symptom:** a private field of `Modde` is "inaccessible" from a child `impl
  Modde`. **Cause:** the child isn't actually a descendant of the module defining
  `Modde`. **Recovery:** keep the `Modde` struct *defined in `app.rs`* (the parent);
  child modules under `app/` are descendants and can access its private fields.
- **Symptom:** the diff is huge and a move accidentally changed a line. **Cause:**
  hand-moving thousands of lines. **Recovery:** move verbatim; rely on
  `cargo test -p modde-ui` after each cluster to catch any behavioral drift.

## Reference

- The already-working precedent: `crates/modde-ui/src/app/tests.rs` (extracted test
  module) — proves the `src/app/` child-module layout.
- rust-ultra code-reorg concern (design stage), which extracted the test module and
  flagged the production decomposition as the #1 follow-up.
- Depends on [04](./04-gameid-deref-removal.md) (app.rs `game_id` typing).
