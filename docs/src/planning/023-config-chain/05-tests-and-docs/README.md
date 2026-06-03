# Phase 05 — Tests + docs for the config chain (multi-sub-layer)

> **Recommended model for merge/orchestration: GPT 5.5 medium**
>
> The phase-level job is just landing four file-disjoint slices and confirming
> the whole chain is covered — a bounded merge, not a design task. The detail and
> the only mild fiddliness (the `std::env` test-race avoidance, the CI postgres
> service) live in the sub-layers; coordination is `medium`.

## Sub-layers

| # | Slug | Model | Touches | Sub-layer file |
|---|------|-------|---------|----------------|
| 01 | core-resolution-tests | 5.5 medium | `crates/modde-core/src/db/` tests, `settings.rs` tests, `Cargo.toml` (dev-dep `serial_test`) | [sub-01-core-resolution-tests.md](./sub-01-core-resolution-tests.md) |
| 02 | cli-integration-tests | 5.5 medium | `crates/modde-cli/tests/` (+ `config.rs` `#[cfg(test)]`) | [sub-02-cli-integration-tests.md](./sub-02-cli-integration-tests.md) |
| 03 | postgres-parity-ci | 5.5 medium | `.forgejo/workflows/` (CI), no source | [sub-03-postgres-parity-ci.md](./sub-03-postgres-parity-ci.md) |
| 04 | config-docs | 5.5 low | `docs/src/**` (settings-file, hm-module, cli) | [sub-04-config-docs.md](./sub-04-config-docs.md) |

## Goal (phase-level)

The configuration chain has a test net and accurate docs: the env→settings→sqlite
resolution, discrete-field assembly, `MODDE_DB_PASSWORD_FILE` injection,
`DbBackend::parse` round-trip, and settings back-compat are unit-tested; the
`config` CLI is integration-tested; the postgres-parity suite actually runs in
CI; and the `[database]` settings, honored env vars, HM `database` options +
assertions, and the `config` CLI are documented — describing only behavior the
runtime implements after Phases 01/03/04.

## Why this matters now

Per the audit, the entire DB surface has **zero** test coverage beyond
`open_memory` (`db/tests.rs`) and a parity test that early-returns when
`MODDE_TEST_PG_URL` is unset (`crates/modde-core/tests/postgres_parity.rs:88-93`)
— so it compiles but never runs in CI (G10). There are **no** tests for the
resolution order, discrete assembly, password-file trim
([db/mod.rs:220-222](../../../../crates/modde-core/src/db/mod.rs)), `DbBackend::parse`
aliases/round-trip ([settings.rs:77-91](../../../../crates/modde-core/src/settings.rs)),
or `[database]`-absent back-compat (G6, G14); **no** test invokes `config
show`/`set-database` (G12); and **no** user docs mention the database backend at
all (G7). The keystone fix (Phase 01) and the CLI/HM honesty fixes are only as
trustworthy as the tests that pin them.

## Out of scope

- Anything that belongs to the code phases (the resolver = Phase 01, the CLI
  surface = Phase 03, the HM eval checks = Phase 04). This phase **tests and
  documents** them; it does not implement them.
- Documenting env vars that the runtime does **not** honor. Sub-04 must describe
  only the post-Phase-01 honored set (BACKEND/URL/NAME/HOST/PORT/USER/
  PASSWORD_FILE). If Phase 01 hasn't landed when sub-04 runs, document only
  BACKEND/URL/PASSWORD_FILE and note the rest as pending.
- Re-litigating the fenced non-gaps (see plan [README](../README.md)).

## Merge plan

The four sub-layers touch disjoint files and can be dispatched in parallel, but
two have upstream phase dependencies the user must respect when dispatching:

- **sub-01** (core tests) and **sub-02** (CLI tests) depend on **Phase 01** (and
  sub-02 on **Phase 03**) — the behavior/surface they assert must exist first.
- **sub-03** (parity CI) and the `DbBackend::parse` slice inside sub-01 are
  **independent** of all code phases — they can be pulled forward into Wave 0.
- **sub-04** (docs) must land **last** (after Phases 01/03/04) so it documents the
  final behavior; it conflicts with nothing else here.

The user runs each sub-layer in a fresh session and merges by simply committing —
no file conflicts between sub-layers. The phase is complete when the
phase-level acceptance below holds.

## Phase-level acceptance criteria

- [ ] `cargo test -p modde-core` and `cargo test -p modde-cli` are green and now
  include: discrete-field resolution, env-over-settings precedence (all fields),
  password-file trim, `DbBackend::parse` round-trip, `[database]`-absent
  back-compat, and `config show`/`set-database` round-trip under an isolated
  config dir.
- [ ] The `MODDE_DATABASE_NAME → dbname` mapping has an explicit test (so a
  `MODDE_DATABASE_DBNAME` reader can't silently re-break it).
- [ ] CI runs the postgres-parity suite against a real PostgreSQL and **fails**
  if `MODDE_TEST_PG_URL` is set but zero parity tests executed.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` stays clean (new
  dev-deps and tests included).
- [ ] `docs/src` documents `[database]` settings keys + a "backend resolution"
  precedence section, the honored env vars, the HM `database` options + the three
  assertions, and `config show`/`set-database`/`test` — all matching the shipped
  behavior, with no mention of unhonored env vars.

## Reference

- Plan README + global constraints + non-gaps: [../README.md](../README.md).
- Upstream phases: [01](../01-core-honor-discrete-env.md),
  [03](../03-cli-show-honesty-and-doctor.md), [04](../04-hm-module-assertions-and-eval-checks.md).
- Existing tests to extend: `crates/modde-core/src/db/tests.rs`,
  `crates/modde-core/tests/postgres_parity.rs`,
  `crates/modde-cli/tests/`.
