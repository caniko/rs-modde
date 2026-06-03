# Phase 03 — CLI: make `config show` honest, allow clearing fields, add a connection doctor

> **Recommended Codex model: GPT 5.5 medium**
>
> Moderate and mostly mechanical once Phase 01's resolver exists — `show` calls
> the shared resolver, `set-database` gains clear/unset semantics, and a small
> `test`/doctor subcommand opens a connection and runs `SELECT 1`. It is not
> trivial (`low`) because the unset/normalize semantics have sharp edges (an
> empty-string `--url ""` that must become `None`, not `Some("")`) and the doctor
> must drive the async DB through the runtime without panicking — but there is no
> design ambiguity, so `medium` holds. A weaker model risks leaving `show` and
> the runtime able to drift (duplicating the merge instead of reusing Phase 01's
> helper).

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. **Depends on Phase 01** — it
reuses the pure resolver Phase 01 introduces so `config show` and runtime
resolution cannot diverge. Rebase onto Phase 01 before starting. Conflicts with
Phase 01 (both touch resolution); runs fine alongside Phases 02 and 04.

## Goal

`modde config show` reports the **fully resolved** database configuration —
applying env-over-settings for **all** fields (not just backend/url/password) and
flagging which are env-overridden — using Phase 01's shared resolver. `modde
config set-database` can **clear** any field (not just set it), so reverting to
sqlite leaves no stale postgres config and `--url ""` means "unset". A new `modde
config test` (doctor) opens the resolved connection, runs a trivial query, and
reports OK/error with a redacted summary. The CLI advertises the same contract
the HM module asserts.

## Why this matters now

**G2 — `show` lies for discrete fields.** `handle_show`
([crates/modde-cli/src/commands/config.rs](../../../../crates/modde-cli/src/commands/config.rs))
overlays env only for `MODDE_DATABASE_URL` and `MODDE_DB_PASSWORD_FILE`; it prints
`host/port/dbname/user` straight from `settings`, never consulting
`MODDE_DATABASE_HOST/PORT/NAME/USER`. The module doc (config.rs lines ~5-7)
claims `show` "reports both" env and settings — **false** for the discrete case.

**G3 — `set-database` is write-only-additive.** It overwrites a field only when
the arg is `Some` (the `if x.is_some()` blocks), so it cannot clear `url`, `host`,
`port`, `dbname`, `user`, or `password_file`. `--backend sqlite` leaves stale
postgres fields behind, and `--url ""` persists `Some("")` (no normalization),
which a later run treats as a set url that wins over discrete fields
([db/mod.rs:196](../../../../crates/modde-core/src/db/mod.rs)).

**G11 — no connection doctor.** `ConfigAction` has only `Show` + `SetDatabase`
([src/main.rs](../../../../crates/modde-cli/src/main.rs)); there is no side-effect-free
way to verify a connection actually works, so misconfigurations surface only when
the GUI/CLI next touches the DB.

## Out of scope

- The runtime resolver itself (Phase 01) — **reuse** it; do not reimplement the
  merge in the CLI.
- HM module assertions (Phase 04) — but **mirror** their contract here
  (Improvement 2): reject `--backend postgres` with neither url nor dbname;
  reject url + discrete simultaneously.
- Tests for the CLI (Phase 05 sub-02) — but keep handlers structured so they are
  testable under an isolated `XDG_CONFIG_HOME`.

## Plan

1. **Reuse Phase 01's resolver in `handle_show`.** Replace the settings-only reads
   for host/port/dbname/user with the shared resolver (or a thin "resolve for
   display" wrapper over it) so `show` reflects exactly what `open_postgres`
   would build. Annotate env-overridden fields the way the backend line already
   is (it prints `(overridden by MODDE_DATABASE_BACKEND=...)`), e.g. print
   `host: db.example (from MODDE_DATABASE_HOST)` vs `... (from settings.toml)`. (G2)
2. **Fix the module doc** at config.rs lines ~5-7 to state precisely which env
   vars are honored (post-Phase-01: BACKEND/URL/NAME/HOST/PORT/USER/PASSWORD_FILE)
   and that `show` reflects the resolved precedence. (G2)
3. **Add clear/unset to `set-database`.** Normalize empty-string args to `None`
   (so `--url ""` clears). Add a repeatable `--clear <field>` flag (field ∈
   url/host/port/dbname/user/password-file) and/or a `config reset-database`
   subcommand that wipes all postgres fields and sets `backend = sqlite`. When
   `--backend sqlite` is given without `--clear`, either wipe the postgres-only
   fields or warn that they remain (pick one and document it). (G3)
4. **Add `ConfigAction::Test` (doctor).** Dispatch through the tokio runtime (the
   existing `db_sync!`/`Runtime::new()?.block_on(...)` pattern in `main.rs`):
   `ModdeDb::open()`, run `SELECT 1` (or the lightest backend-agnostic probe),
   and print the resolved backend + a **redacted** connection summary (host/
   socket/dbname/user, never the password) + `OK`/error. (G11)
5. **Mirror the HM contract (validation).** In `set-database`, reject
   `--backend postgres` with no url and no dbname, and reject url + discrete set
   simultaneously — matching `databaseAssertions`
   ([nix/hm-module.nix:592-606](../../../../nix/hm-module.nix)). (Improvement 2)
6. **Wire the new subcommands** into the `Commands::Config` dispatch and the
   exhaustive matches in `main.rs` (the `command_mutates_state` classifier:
   `show`/`test` are read-only, `set-database`/`reset-database` mutate).

## Acceptance criteria

- [ ] `MODDE_DATABASE_HOST=h MODDE_DATABASE_NAME=d modde config show` prints
  `host: h (from MODDE_DATABASE_HOST)` and `dbname: d (from MODDE_DATABASE_NAME)`
  — env reflected for discrete fields, source-annotated.
- [ ] The config.rs module doc no longer claims `show` reports env for fields it
  doesn't, and lists only the honored env vars.
- [ ] `modde config set-database --backend sqlite` (or `reset-database`) leaves
  `settings.toml` with no postgres fields; `--url ""` results in `url` unset
  (not `Some("")`); `--clear host` removes `host`.
- [ ] `modde config test` against a reachable PG prints the resolved backend +
  redacted summary + `OK`; against an unreachable one prints a contextual error
  and non-zero exit — with no password in either output.
- [ ] `set-database --backend postgres` with neither url nor dbname is rejected;
  url + discrete together is rejected — same messages spirit as the HM assertions.
- [ ] `cargo build -p modde-cli` (default + `--no-default-features`) compiles;
  `cargo clippy -p modde-cli --all-targets -- -D warnings` clean; existing CLI
  snapshot tests updated if the `config` help text changed.

## Files likely touched

- `crates/modde-cli/src/commands/config.rs` — `handle_show` (resolver reuse +
  annotations), `handle_set_database` (clear/normalize/validate), module doc, new
  `handle_test`/`handle_reset`.
- `crates/modde-cli/src/main.rs` — `ConfigAction` enum (+ `Test`, optional
  `ResetDatabase`, `--clear`), dispatch, `command_mutates_state`, exhaustive
  matches, and the help-snapshot fixtures if help text changed.

## Pitfalls

- **Symptom:** `show` and the GUI disagree about the effective backend. **Cause:**
  the CLI reimplemented the merge instead of calling Phase 01's resolver.
  **Recovery:** call the shared function; if it isn't exported, export it from
  modde-core (coordinate with Phase 01).
- **Symptom:** CLI help-snapshot tests fail. **Cause:** adding `test`/`--clear`
  changed the generated help. **Recovery:** review the diff is exactly the new
  surface, then accept the `.snap.new` files (as done for the original `config`
  command).
- **Symptom:** `config test` panics with "no reactor running" or nested-runtime.
  **Cause:** awaited sqlx outside a runtime, or built a `Runtime` inside one.
  **Recovery:** use the existing `db_sync!`/`block_on` pattern the other
  DB-touching CLI handlers use.
- **Symptom:** `--url ""` still wins over discrete fields. **Cause:** stored
  `Some("")` instead of `None`. **Recovery:** normalize empty → `None` before
  saving; add the Phase 05 round-trip test for it.

## Reference

- Plan README + global constraints: [README.md](./README.md).
- Prereq (the resolver to reuse): [01-core-honor-discrete-env.md](./01-core-honor-discrete-env.md).
- CLI config code: [crates/modde-cli/src/commands/config.rs](../../../../crates/modde-cli/src/commands/config.rs),
  [crates/modde-cli/src/main.rs](../../../../crates/modde-cli/src/main.rs).
- HM contract to mirror: [nix/hm-module.nix:592-606](../../../../nix/hm-module.nix).
- CLI tests that exercise this: [05-tests-and-docs/sub-02-cli-integration-tests.md](./05-tests-and-docs/sub-02-cli-integration-tests.md).
