# Phase 05 · Sub-layer 01 — modde-core resolution + back-compat tests

> **Recommended Codex model: GPT 5.5 medium**
>
> Leaf-level test authoring, but not trivial: it must exercise the env→settings
> merge without tripping the `std::env` test race the crate already hit (an
> env-race test is `#[ignore]`d at `vfs/mod.rs:587`), which means testing the
> *pure* resolver through an injected accessor and quarantining any real-env
> test behind `#[serial]`. Rust 2024's `std::env::set_var` is `unsafe`, so the
> serial tests need care. `medium`, not `low`, for those sharp edges; not
> `high` — it's bounded leaf work once Phase 01's resolver exists.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. The discrete-assembly tests
depend on **Phase 01** (the pure `build_pg_options` resolver). The
`DbBackend::parse` and `[database]` back-compat tests are **independent** of Phase
01 and can be written immediately. Touches modde-core test code only —
file-disjoint from the other sub-layers.

## Goal

The resolution and settings behavior the chain depends on is pinned by fast,
server-free unit tests: discrete-field assembly with env-over-settings
precedence, `MODDE_DB_PASSWORD_FILE` trimming, the `MODDE_DATABASE_NAME → dbname`
mapping, `DbBackend::parse` aliases/round-trip, and `[database]`-absent
back-compat.

## Why this matters now

`crates/modde-core/src/db/tests.rs` only uses `open_memory`, and
`crates/modde-core/tests/postgres_parity.rs` sets only `url` — so the discrete
path, the env precedence, and the password-file trim
([db/mod.rs:220-222](../../../../../crates/modde-core/src/db/mod.rs)) have **no**
coverage (G6). `DbBackend::parse`'s aliases (`pg`/`postgresql`), trim, case-fold,
and `as_str` round-trip ([settings.rs:77-91](../../../../../crates/modde-core/src/settings.rs))
are untested, as is the "no `[database]` table → `Sqlite`" back-compat (G14).
Without these, Phase 01's keystone fix and the NAME↔dbname bridge can silently
regress.

## Out of scope

- Implementing the resolver (Phase 01) — this only **tests** it.
- A live-PostgreSQL parity run (that's sub-03's CI job; the parity test file
  itself already exists).
- CLI tests (sub-02).

## Plan

1. **Add `serial_test` as a dev-dependency** (workspace or modde-core
   `[dev-dependencies]`). Confirm `cargo clippy -- -D warnings` stays clean.
2. **Pure-resolver tests (no env, no server)** — call Phase 01's
   `build_pg_options(&settings, &env_fn)` with a closure standing in for the
   environment:
   - discrete env over empty settings → assert `host()`, `port()`,
     `get_database()`, `get_username()` on the returned `PgConnectOptions` match
     the env values.
   - discrete env over conflicting settings → env wins, per field.
   - settings-only (no env) → settings values used; dbname-required check fires
     when neither env nor settings supply a name.
   - `url` present (env or settings) → discrete fields ignored (url wins).
   - bad `MODDE_DATABASE_PORT` (`"x"`, `"70000"`) → `Err`, not panic, not silent
     default.
   - **`MODDE_DATABASE_NAME` populates `dbname`** — an explicit assertion so a
     future `MODDE_DATABASE_DBNAME` reader can't silently re-break it. (G13)
3. **Password-file helper test.** If Phase 01 exposed a `read_password_file`
   (trim) helper, test trailing-newline trimming directly; otherwise test it
   through `build_pg_options` with a temp file and assert the password is applied
   (use a `PgConnectOptions` getter or a redacted debug — never compare against a
   leaked secret in output).
4. **`DbBackend::parse` round-trip tests** (independent of Phase 01):
   `parse("SQLite") == Some(Sqlite)`, `parse("POSTGRES"/" pg "/"postgresql") ==
   Some(Postgres)`, `parse("mysql") == None`, and `parse(as_str(x)) == Some(x)`
   for both variants. (G14)
5. **Back-compat test:** deserialize a `settings.toml` string with **no**
   `[database]` table → `AppSettings.database.backend == Sqlite` and all
   connection fields `None`; and a `DatabaseSettings` serde round-trip (set →
   serialize → deserialize → equal), including the `skip_serializing_if` fields. (G14)
6. **Real-env routing test (quarantined).** A small `#[serial_test::serial]`
   test that sets `MODDE_DATABASE_BACKEND`/`MODDE_DATABASE_URL` via the (Rust
   2024 `unsafe`) `std::env::set_var`, asserts `open_with_settings` routes to the
   right backend, and restores the env. Keep this minimal — the pure-resolver
   tests carry the bulk so the serial surface stays tiny.

## Acceptance criteria

- [ ] `cargo test -p modde-core` green, including: discrete-assembly precedence
  (per field), bad-port error, NAME→dbname mapping, password-file trim,
  `DbBackend::parse` aliases + round-trip, and `[database]`-absent back-compat.
- [ ] The bulk of resolution tests use the **pure** resolver with an injected env
  accessor (no `std::env` mutation); any real-env test is `#[serial]` and
  restores the environment.
- [ ] `cargo clippy -p modde-core --all-targets -- -D warnings` clean (incl. the
  new `serial_test` dev-dep and any `unsafe` env blocks).

## Files likely touched

- `crates/modde-core/src/db/tests.rs` (or a new `db/connect_tests` module) —
  resolver + password-file + serial routing tests.
- `crates/modde-core/src/settings.rs` `#[cfg(test)]` — `DbBackend::parse` +
  back-compat/round-trip tests.
- `crates/modde-core/Cargo.toml` — `serial_test` dev-dependency.

## Pitfalls

- **Symptom:** intermittent test failures under parallel runs. **Cause:** mutating
  real `std::env`. **Recovery:** test the pure resolver with an injected closure;
  gate real-env tests with `#[serial]`.
- **Symptom:** `set_var` won't compile. **Cause:** Rust 2024 made it `unsafe`.
  **Recovery:** wrap in `unsafe { ... }` with a comment, only inside `#[serial]`
  tests, and restore the prior value.
- **Symptom:** a test prints/asserts the database password. **Cause:** comparing
  the applied password directly. **Recovery:** assert via a non-leaking getter or
  just that connection assembly succeeds; never surface the secret.
- **Symptom:** `PgConnectOptions` getter not found. **Cause:** sqlx getter names
  differ (`get_database`/`get_username` vs `host`/`port`). **Recovery:** check the
  sqlx version's `PgConnectOptions` API; use whatever getters that version
  exposes.

## Reference

- Phase README + merge plan: [README.md](./README.md).
- Resolver under test: [../01-core-honor-discrete-env.md](../01-core-honor-discrete-env.md).
- Code: [crates/modde-core/src/db/mod.rs:188-230](../../../../../crates/modde-core/src/db/mod.rs),
  [crates/modde-core/src/settings.rs:42-92](../../../../../crates/modde-core/src/settings.rs).
- The existing `#[ignore]`d env-race precedent: `crates/modde-core/src/vfs/mod.rs:587`.
