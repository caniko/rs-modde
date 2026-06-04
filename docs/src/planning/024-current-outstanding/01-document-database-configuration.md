# Phase 01 — Document the database configuration surface

> **Recommended Codex model: GPT 5.5 medium**
>
> This is a docs-only phase, but it must reconcile behavior across core
> settings, CLI output, and the Home Manager module without inventing options.
> Medium is appropriate for a leaf task with several source-of-truth files and a
> real risk of documenting stale planning assumptions.

## Working tree

`/data/nvme0/can/Projects/rs-modde`. This phase touches stable docs only.

## Goal

Users and contributors can configure and inspect SQLite/PostgreSQL database
settings from the stable documentation without reading planning files.

## Why this matters now

The config-chain implementation is present and targeted tests pass, but
`docs/src/configuration/settings-file.md`, `docs/src/configuration/hm-module.md`,
and `docs/src/reference/cli.md` do not mention the database backend, the honored
`MODDE_DATABASE_*` variables, or `modde config`. Retiring the old plan without
this docs work would lose the only user-facing description of the feature.

## Out of scope

- Do not add runtime behavior or CLI flags.
- Do not document unimplemented options such as a first-class `sslmode` enum or
  Home Manager `dataDir` database option.
- Do not claim password contents are stored; only paths are configured.

## Plan

1. Verify the current source contract:
   `crates/modde-core/src/settings.rs`, `crates/modde-core/src/db/mod.rs`,
   `crates/modde-cli/src/commands/config.rs`, and `nix/hm-module.nix`.
2. Update `docs/src/configuration/settings-file.md` with a `[database]` table,
   example TOML for SQLite/default and PostgreSQL, and precedence rules:
   environment, then settings, then SQLite defaults. State that
   `MODDE_DATABASE_URL` wins over discrete host/port/name/user fields.
3. Update `docs/src/configuration/hm-module.md` with
   `programs.modde.database`, the `name` to `dbname` mapping, the three HM
   assertions, and the fact that HM exports session variables after activation.
4. Update `docs/src/reference/cli.md` with `modde config show`,
   `set-database`, `reset-database`, and `test`, including clear/unset behavior.
5. Build docs with `nix build .#docs`.

## Acceptance criteria

- [ ] Stable docs list only honored env vars:
  `MODDE_DATABASE_BACKEND`, `MODDE_DATABASE_URL`, `MODDE_DATABASE_HOST`,
  `MODDE_DATABASE_PORT`, `MODDE_DATABASE_NAME`, `MODDE_DATABASE_USER`, and
  `MODDE_DB_PASSWORD_FILE`.
- [ ] Stable docs explain `url` versus discrete-field precedence and
  `password_file`/`MODDE_DB_PASSWORD_FILE` behavior.
- [ ] HM docs include `backend`, `url`, `host`, `port`, `name`, `user`, and
  `passwordFile`, plus all three assertion shapes.
- [ ] CLI docs include `config show`, `config set-database`,
  `config reset-database`, and `config test`.
- [ ] `nix build .#docs` succeeds.

## Files likely touched

- `docs/src/configuration/settings-file.md`
- `docs/src/configuration/hm-module.md`
- `docs/src/reference/cli.md`
- `docs/src/SUMMARY.md` only if a new page is created.

## Pitfalls

- **Symptom:** docs mention `MODDE_DATABASE_DBNAME`. **Cause:** copying internal
  struct names. **Recovery:** document `MODDE_DATABASE_NAME`; settings uses
  `dbname`, HM uses `name`.
- **Symptom:** docs say HM changes affect already-running shells. **Cause:**
  confusing activation scripts with current process environment. **Recovery:**
  state that new sessions see `home.sessionVariables`; CLI config can update
  settings immediately.
- **Symptom:** docs imply TLS/socket options have dedicated fields. **Cause:**
  extrapolating from PostgreSQL concepts. **Recovery:** document the URL escape
  hatch only.

## Reference

- `crates/modde-core/src/settings.rs`
- `crates/modde-core/src/db/mod.rs`
- `crates/modde-cli/src/commands/config.rs`
- `nix/hm-module.nix`
- Predecessor planning docs remain available in git history if historical
  context is needed.
