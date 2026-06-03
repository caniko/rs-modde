# Phase 01 — Core: honor the discrete Postgres env vars + password file, via a pure testable resolver

> **Recommended Codex model: GPT 5.5 medium**
>
> The change is small (a per-field env-over-settings merge plus extracting a pure
> helper), so it is moderate work, not complex design — `medium` is right and
> inflating to `high` would burn tokens for no quality gain. But it is the
> plan's keystone: three later phases (CLI honesty, the test net, the docs)
> build on the resolver this phase introduces, so the precedence rules and the
> `MODDE_DATABASE_NAME → dbname` mapping must be exact. A weaker model tends to
> get the env-vs-settings precedence backwards, forget the fail-fast port parse,
> or invent a `MODDE_DATABASE_DBNAME` reader that silently re-breaks the mapping
> — hence `medium`, not `low`.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. Self-contained modde-core
change; no dependency on other phases. **Land this before Phase 03 (the CLI
reuses the resolver) and before Phase 05's docs (which describe this behavior).**

## Goal

`modde` connects to PostgreSQL from a **discrete-only** configuration
(`MODDE_DATABASE_HOST/PORT/NAME/USER`, no `url`) exactly as it does from a full
`url`. Env-over-settings precedence becomes **uniform** across all connection
fields. The merge logic lives in a **pure, unit-testable** function so Phase 05
can test it without a live server, and so `config show` (Phase 03) can reuse it
and never drift from runtime behavior. Connection and password-file failures
carry actionable, redacted context.

## Why this matters now

The Home Manager module exports `MODDE_DATABASE_HOST/PORT/NAME/USER`
([nix/hm-module.nix:620-623](../../../../nix/hm-module.nix)) into
`home.sessionVariables` and the activation script, but the runtime never reads
them. `open_postgres`
([crates/modde-core/src/db/mod.rs:188-230](../../../../crates/modde-core/src/db/mod.rs))
reads env only for `MODDE_DATABASE_URL` (line 192) and `MODDE_DB_PASSWORD_FILE`
(line 216); the no-url branch builds `PgConnectOptions` from `settings.host/port/
dbname/user` only (lines 199-213):

```rust
let mut o = PgConnectOptions::new();
if let Some(host) = &settings.host { o = o.host(host); }
if let Some(port) = settings.port { o = o.port(port); }
let dbname = settings.dbname.as_deref().ok_or_else(|| {
    CoreError::Other("postgres backend selected but no database name configured".into())
})?;                                    // <-- hard-fails for discrete-only env config
o = o.database(dbname);
if let Some(user) = &settings.user { o = o.username(user); }
```

So a discrete-only HM config flips the backend on (`BACKEND` is honored at
[db/mod.rs:148](../../../../crates/modde-core/src/db/mod.rs)) but assembles from an
empty settings struct and dies with *"postgres backend selected but no database
name configured"*. URL and password_file get env precedence; the four discrete
fields silently do not. This is the chain's keystone defect (G1); the
`MODDE_DATABASE_NAME → dbname` spelling bridge (G13) and the opaque connect/
password errors (G15) ride along.

## Out of scope

- The CLI `config show`/`set-database` changes (Phase 03) — but **design the
  resolver so Phase 03 can call it.** Export it from modde-core (e.g.
  `pub(crate)` or a small public helper) so `handle_show` reuses the exact merge.
- Deleting the discrete fields in favor of url-only (rejected — see README global
  constraints).
- Adding a first-class `sslmode`/socket option (Phase 04 doc note only).
- HM module / assertion changes (Phase 04).

## Plan

1. **Extract a pure resolver.** Add a free function in `db/mod.rs` (or a small
   `db/connect.rs`), pure and side-effect-free except for the injected env
   accessor, e.g.:
   ```rust
   #[cfg(feature = "postgres")]
   fn build_pg_options(
       settings: &DatabaseSettings,
       env: &dyn Fn(&str) -> Option<String>,
   ) -> Result<sqlx::postgres::PgConnectOptions> { ... }
   ```
   Injecting `env` (rather than calling `std::env::var` directly) sidesteps the
   `std::env` test race the crate already hit (`vfs/mod.rs:587` has an `#[ignore]`d
   env-race test) and makes Phase 05's unit tests deterministic.
2. **Layer env over settings, per field**, in the no-url branch:
   - `host  = env("MODDE_DATABASE_HOST").or(settings.host.clone())`
   - `port  = env("MODDE_DATABASE_PORT")` parsed to `u16` — **fail fast** with a
     clear `CoreError` on a non-numeric/out-of-range value (do not silently fall
     back) — `.or(settings.port)`
   - `dbname = env("MODDE_DATABASE_NAME").or(settings.dbname.clone())`
     **(env var is `MODDE_DATABASE_NAME`; struct field is `dbname`)**
   - `user  = env("MODDE_DATABASE_USER").or(settings.user.clone())`
   - Keep the **dbname-required** check, but against the *merged* value (so a
     discrete env-only config now satisfies it). host/port/user stay optional
     (sqlx defaults to socket/peer when unset — required for the atlas
     `postgres:///can?host=/run/postgresql` style).
3. **Keep `url` first.** If `MODDE_DATABASE_URL` or `settings.url` is set, parse
   it (current line 196-197) and skip the discrete branch — url still wins, as
   documented at [settings.rs:46-47](../../../../crates/modde-core/src/settings.rs).
4. **Password file unchanged in behavior, clearer in failure.** Keep the
   `MODDE_DB_PASSWORD_FILE`-over-`settings.password_file` precedence (db/mod.rs:216-219)
   and the `.trim()` (line 222). Wrap the read error (line 221) with context:
   `failed to read MODDE_DB_PASSWORD_FILE {path}`. (G15)
5. **Wrap the connect failure** (line 225) with a **redacted** summary —
   host/socket, port, dbname, user — and **never** the password. (G15)
6. **Pin the NAME↔dbname mapping** with a one-line comment at the env reader and
   at [settings.rs:55](../../../../crates/modde-core/src/settings.rs), and leave a
   hook Phase 05 will assert. (G13)
7. **Build both feature sets.** Ensure the resolver and any new error variants
   compile under `--no-default-features` (the whole postgres path is
   `#[cfg(feature = "postgres")]`; the `open_postgres` no-feature stub at
   db/mod.rs:232 must still error cleanly).

## Acceptance criteria

- [ ] A discrete-only env config (`MODDE_DATABASE_BACKEND=postgres` +
  `MODDE_DATABASE_HOST/PORT/NAME/USER`, **no** `MODDE_DATABASE_URL`, empty
  settings) builds a `PgConnectOptions` whose `host()/port()/get_database()/
  get_username()` match the env values — assert via the pure resolver in a unit
  test (no live server). (This test may land here or in Phase 05; the resolver
  must be callable for it.)
- [ ] Env overrides settings for **every** field (host/port/dbname/user, plus the
  existing url/password_file). A bad `MODDE_DATABASE_PORT` returns a clear error,
  not a panic and not a silent default.
- [ ] `url` (env or settings) still takes precedence over the discrete fields.
- [ ] The resolver is a pure function with an injected env accessor, reachable
  from modde-cli (Phase 03) without duplicating logic.
- [ ] Connect failure and password-file-read failure produce contextual errors
  with **no password** in the message.
- [ ] `cargo build -p modde-core` (default) and `--no-default-features` compile;
  `cargo clippy -p modde-core --all-targets -- -D warnings` clean;
  `cargo test -p modde-core` green.

## Files likely touched

- `crates/modde-core/src/db/mod.rs` — `open_postgres` (188-230) + new
  `build_pg_options` resolver (optionally a `db/connect.rs`).
- `crates/modde-core/src/settings.rs` — NAME↔dbname comment at line 55; no
  schema change.
- `crates/modde-core/src/error.rs` — context wrapping for connect/password-file
  errors (around the `Database`/`Io`/`Other` variants).

## Pitfalls

- **Symptom:** env var ignored when settings also set it. **Cause:** `.or()`
  argument order reversed (settings winning over env). **Recovery:**
  `env(...).or(settings...)` — env is the receiver, settings the fallback.
- **Symptom:** a future contributor reads `MODDE_DATABASE_DBNAME` and the HM
  module (which exports `MODDE_DATABASE_NAME`) silently stops working again.
  **Cause:** the three-way `name`/`NAME`/`dbname` spelling. **Recovery:** the
  comment + Phase 05's mapping test guard it; keep the env key `MODDE_DATABASE_NAME`.
- **Symptom:** `cargo test` flakes when run with other tests. **Cause:** mutating
  `std::env` in a test races other threads (the crate already `#[ignore]`d such a
  test). **Recovery:** the injected env accessor avoids real env in unit tests;
  reserve real-env tests for a `#[serial]` block (Phase 05).
- **Symptom:** `--no-default-features` build breaks. **Cause:** the resolver or a
  new error variant referenced outside `#[cfg(feature = "postgres")]`.
  **Recovery:** gate the postgres-only code; keep error variants backend-agnostic.

## Reference

- Plan README + global constraints: [README.md](./README.md).
- Keystone code: [crates/modde-core/src/db/mod.rs:188-230](../../../../crates/modde-core/src/db/mod.rs).
- Settings shape: [crates/modde-core/src/settings.rs:42-92](../../../../crates/modde-core/src/settings.rs).
- Consumers of this resolver: [03-cli-show-honesty-and-doctor.md](./03-cli-show-honesty-and-doctor.md),
  [05-tests-and-docs/](./05-tests-and-docs/README.md).
