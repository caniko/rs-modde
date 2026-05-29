# Phase 02 — Observability: stop the library crate printing to stdout

> **Recommended Codex model: GPT 5.5 medium**
>
> Moderate complexity with a design judgment: the fix is mechanical (move
> `println!` out of a library), but deciding *how* the CLI re-emits the output
> without changing what the user sees — and confirming the GUI path was silently
> dropping it — needs reading both the library and the CLI caller. A `low` tier
> would likely just delete the prints and regress CLI output. Sub-agent role.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. Disjoint from Phases 01 and
09 — safe to run concurrently. **Shares `modde-cli/src/commands/install.rs` with
Phase 03**, so land this before 03 (or rebase).

## Goal

`crates/modde-games/src/launcher.rs` (a library module) no longer writes
user-facing text to stdout via `println!`; the launcher functions communicate
results structurally (return value and/or `tracing` at the appropriate level),
and `modde-cli` owns the user-facing printing — so the GUI (`modde-ui`), which
calls the same functions, no longer silently loses that output. **The text a CLI
user sees is unchanged.**

## Why this matters now

The observability audit found ~9 `println!` calls inside the library crate
`modde-games`, all in `launcher.rs`, several of which simply duplicate an adjacent
`tracing` call:

- `launcher.rs:314, 320` — wrapper restore / env-var counts (paired `info!` at ~306).
- `launcher.rs:363, 372` — Steam/Unknown launcher instructions (paired `warn!` at ~359/371).
- `launcher.rs:473` — Heroic `WINEDLLOVERRIDES` notice (paired `info!` at ~456/464).
- `launcher.rs:496` — non-Heroic wrapper instruction.
- `launcher.rs:561` — Heroic wrapper registered (paired `info!` at ~562).
- `launcher.rs:771, 777` — applied tool env-var / wrapper counts.

A library printing to stdout is a layering violation: the GUI calls these
functions (`modde-cli/src/commands/install.rs:524,536` is the CLI caller) and the
prints vanish into a detached stdout there. `dbg!` count is zero workspace-wide;
no other library crate prints. This is the only genuine observability debt.

## Out of scope

- `modde-cli` and `modde-ui` `println!`/`eprintln!` — those are legitimate
  user-facing output and stay.
- `crates/modde-sources/src/bin/update-wabbajack-fixture.rs` `eprintln!` — a
  dev-only fixture tool; leave it.
- Adding a logging framework or changing `tracing` setup.
- Touching any launcher logic beyond where it emits output.

## Plan

1. Read `crates/modde-games/src/launcher.rs` and catalog every `println!`/`eprintln!`
   and its purpose. For each, note whether an adjacent `tracing` call already
   conveys the same information.
2. Read the CLI caller(s) — `modde-cli/src/commands/install.rs` around lines
   524/536 — to see how the launcher result is consumed and where user output
   belongs.
3. Choose the minimal layering-correct shape. Preferred: have the launcher
   functions **return** the structured facts they currently print (e.g. a small
   result struct or counts already in scope), keep the `tracing` calls, and have
   `modde-cli` format the user-facing lines from the returned value. Where a print
   is pure duplication of an existing `info!`/`warn!`, deleting it (and relying on
   the CLI to surface the structured result) is correct.
4. Update `modde-cli` to print exactly what the CLI user saw before — verify
   against the current strings so the visible CLI output is byte-for-byte the same
   (or intentionally improved, noted in the commit).
5. Confirm the GUI path: `modde-ui` calling the same launcher functions now gets
   the structured result (or `tracing` events) instead of lost stdout.
6. Gate and commit (`refactor(games): ...` or `fix(games): ...`).

## Acceptance criteria

- [ ] `rg -n 'println!|eprintln!' crates/modde-games/src/launcher.rs` returns
      nothing (zero library prints in that file).
- [ ] `rg -n 'println!|eprintln!' crates/modde-{core,sources,games}/src` shows no
      new library prints anywhere (the bin fixture tool is the only allowed `eprintln!`).
- [ ] Running the relevant CLI launcher command produces the same user-facing
      lines as before (manually compare the strings; note any intentional wording
      change in the commit body).
- [ ] `cargo clippy -p modde-games -p modde-cli --all-targets -- -D warnings` clean.
- [ ] `cargo test -p modde-games -p modde-cli` passes.

## Files likely touched

- `crates/modde-games/src/launcher.rs` (remove prints; return/log instead).
- `crates/modde-cli/src/commands/install.rs` (print the structured result).
- Possibly a small result type in `modde-games` if the functions need to return
  more than they do today.

## Pitfalls

- **Symptom:** CLI output goes missing after the change. **Cause:** you deleted a
  library `println!` without re-emitting it from the CLI. **Recovery:** every
  print that was *not* pure tracing-duplication must reappear in `modde-cli`.
- **Symptom:** signature change ripples further than expected. **Cause:** the
  launcher fn is called from several places. **Recovery:** keep the return type
  additive (return a struct the existing callers can ignore) rather than changing
  parameters.
- **Symptom:** `tracing` macro args trip `clippy::doc_markdown` or uninlined
  format args. **Recovery:** use inline captures (`info!(count, "...")`).

## Reference

- Observability audit (rust-ultra polish stage): the launcher.rs print inventory.
- CLI caller: `crates/modde-cli/src/commands/install.rs:524,536`.
- Sibling: shares a file with [03](./03-typed-source-error.md) — land 02 first.
