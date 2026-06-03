# Phase 05 · Sub-layer 02 — `modde config` integration tests

> **Recommended Codex model: GPT 5.5 medium**
>
> Leaf-level integration testing with `assert_cmd` under an isolated config dir.
> The mild care needed — pointing `MODDE_*` config/data dirs at a temp location so
> the test can't read or clobber the real `~/.config/modde/settings.toml`, and
> exercising env-override reporting deterministically — keeps it at `medium`
> rather than `low`. No design content; bounded once Phase 03's CLI surface exists.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. Depends on **Phase 03** (the
`config show`/`set-database`/`test` surface it exercises) and transitively on
**Phase 01** (the resolution `show` reflects). Touches modde-cli test code only —
disjoint from the other sub-layers.

## Goal

The `modde config` surface is covered by integration tests that drive the real
binary under an isolated config directory: `set-database` writes the expected
`settings.toml`; `show` reports the resolved state including env overrides for all
fields and source annotations; clearing/reset works; and the `DatabaseSettings`
serde round-trip (incl. empty-string normalization and back-compat) holds.

## Why this matters now

`crates/modde-cli/src/commands/config.rs` has no `#[cfg(test)]`, and no
integration test invokes `config show`/`set-database`; the settings round-trip is
never exercised against `DatabaseSettings` (G12). Phase 03 adds clear/unset,
env-override reporting, validation, and a doctor — none of which is pinned, so
they can regress silently.

## Out of scope

- Implementing the CLI changes (Phase 03).
- The core resolver unit tests (sub-01).
- A live-PG `config test` run in CI (sub-03 owns CI infra; a local/ignored
  smoke is fine here but don't wire CI).

## Plan

1. **Isolate the environment.** In a new
   `crates/modde-cli/tests/cli_config_tests.rs`, run the binary with `assert_cmd`
   (already used by the existing CLI integration tests) under a `tempfile`
   `XDG_CONFIG_HOME` (and `MODDE_DATA_DIR` if the config dir derives from it) so
   `AppSettings::config_path()` resolves into the temp dir — never the real
   `~/.config/modde/settings.toml`. Confirm via `crate::paths::modde_config_dir()`
   how the config dir is derived and set the matching env.
2. **`set-database` → settings.toml.** Run `config set-database --backend postgres
   --name modde --host h --port 5432 --user u`; assert the written
   `settings.toml` `[database]` section has the expected keys/values and **no**
   password.
3. **`show` reflects settings + env.** Run `config show` with no env → asserts the
   stored values + `(from settings.toml)` annotations; then run with
   `MODDE_DATABASE_HOST=other MODDE_DATABASE_NAME=other_db` → asserts `show`
   reports the env values with `(from MODDE_DATABASE_HOST)` / `(from
   MODDE_DATABASE_NAME)` annotations (the G2 fix).
4. **Clear / reset / normalize.** `set-database --backend sqlite` (or
   `reset-database`) → assert the postgres fields are gone; `set-database --url ""`
   → assert `url` is unset (not `""`); `--clear host` → assert `host` removed.
5. **Validation.** `set-database --backend postgres` with neither url nor name →
   non-zero exit + clear message; url + discrete together → rejected (mirrors the
   HM assertions).
6. **`config test` doctor (optional, gated).** A `#[ignore]` or
   `MODDE_TEST_PG_URL`-gated test that runs `config test` against a reachable PG
   and asserts `OK` + redacted summary + no password in output. Leave the always-on
   CI parity to sub-03.
7. **Serde round-trip unit** (if not covered in sub-01): a `DatabaseSettings`
   round-trip incl. `skip_serializing_if` fields and the empty-string→`None`
   normalization, so the CLI and the struct agree.

## Acceptance criteria

- [ ] `cargo test -p modde-cli` green with a new `cli_config_tests.rs` covering:
  `set-database` write, `show` settings+env reporting with source annotations,
  clear/reset/empty-normalize, and validation rejections.
- [ ] Every test runs under an **isolated** config dir — running the suite never
  reads or modifies the real `~/.config/modde/settings.toml`.
- [ ] CLI help-snapshot tests reflect the Phase 03 surface (accepted, if changed).
- [ ] `cargo clippy -p modde-cli --all-targets -- -D warnings` clean.

## Files likely touched

- `crates/modde-cli/tests/cli_config_tests.rs` (new).
- `crates/modde-cli/tests/snapshots/*.snap` (only if help text changed in Phase 03).
- `crates/modde-cli/Cargo.toml` — dev-deps if `assert_cmd`/`tempfile`/`predicates`
  aren't already present (the existing integration tests likely pull them in).

## Pitfalls

- **Symptom:** tests pass locally but mutate the developer's real modde config.
  **Cause:** the config dir wasn't isolated. **Recovery:** set the config-dir env
  to a tempdir and assert the path the binary writes is inside it.
- **Symptom:** env-override test is flaky. **Cause:** env leaks across `assert_cmd`
  invocations or parallel tests. **Recovery:** set env per-`Command` (assert_cmd's
  `.env(...)`), not via `std::env`; each `Command` is its own process.
- **Symptom:** snapshot tests fail. **Cause:** Phase 03 changed `config` help.
  **Recovery:** confirm the diff is the intended new surface, then accept.
- **Symptom:** the doctor test fails in CI with no DB. **Cause:** it wasn't gated.
  **Recovery:** gate on `MODDE_TEST_PG_URL` or `#[ignore]`; sub-03 runs the real
  parity job.

## Reference

- Phase README + merge plan: [README.md](./README.md).
- CLI surface under test: [../03-cli-show-honesty-and-doctor.md](../03-cli-show-honesty-and-doctor.md).
- Existing CLI integration tests to mirror: `crates/modde-cli/tests/`.
