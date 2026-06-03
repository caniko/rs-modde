# Phase 05 · Sub-layer 04 — document the database configuration across the chain

> **Recommended Codex model: GPT 5.5 low**
>
> Mechanical doc authoring against already-decided behavior — adding tables,
> a precedence section, an option reference, and a CLI section to existing
> mdBook pages. `low` is right: no design, no code. The one bit of care —
> documenting **only** the env vars the runtime actually honors (post-Phase-01)
> and not resurrecting the fenced non-gaps — is a correctness constraint, not a
> reason to inflate the tier; it's spelled out below so a `low`-tier run can't
> miss it.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. **Land last** — after Phases
01, 03, and 04 — so the docs describe the final, shipped behavior. Touches
`docs/src/**` only; disjoint from every other sub-layer.

## Goal

A user can configure modde's database backend from the docs alone: the
`settings.toml` `[database]` keys and a precedence section, the **honored**
environment variables, the Home Manager `database` options + the three
assertions, and the `modde config` subcommands are all documented and match the
shipped behavior.

## Why this matters now

There are **zero** user docs for the database backend (G7): `settings-file.md`'s
Keys table and env table omit all `[database]` keys and `MODDE_DATABASE_*`/
`MODDE_DB_PASSWORD_FILE`; `hm-module.md` has no database mention; `cli.md` has no
`config` section. The backend is shipped and now (post-Phase-01) fully wired, but
undiscoverable.

## Out of scope

- Documenting env vars the runtime does **not** honor. Document the post-Phase-01
  honored set only: `MODDE_DATABASE_BACKEND`, `MODDE_DATABASE_URL`,
  `MODDE_DATABASE_NAME`, `MODDE_DATABASE_HOST`, `MODDE_DATABASE_PORT`,
  `MODDE_DATABASE_USER`, `MODDE_DB_PASSWORD_FILE`. (If Phase 01 hasn't landed when
  this runs, document only BACKEND/URL/PASSWORD_FILE and mark the rest "pending
  Phase 01".)
- Inventing a `sslmode` option or a `dataDir` HM option in docs (fenced non-gaps).
  You may note (per Phase 04) that socket dirs / TLS go through `url`.
- Code changes of any kind.

## Plan

1. **Locate the docs.** Find the mdBook pages: `docs/src/**` — `settings-file.md`
   (the settings reference), `hm-module.md` (HM module reference), and `cli.md`
   (CLI reference). Confirm exact paths and the `SUMMARY.md` structure.
2. **`settings-file.md`:**
   - Add the `[database]` keys (`backend`, `url`, `host`, `port`, `dbname`,
     `user`, `password_file`) to the Keys table, with types/defaults and the note
     that the password is read from `password_file` at runtime, never stored.
   - Add a TOML example showing a sqlite default and a postgres block.
   - Add a **"Database backend resolution"** subsection stating the precedence
     **env → settings.toml → sqlite default**, that `url` wins over the discrete
     fields, and that `password_file`/`MODDE_DB_PASSWORD_FILE` overrides any
     password embedded in `url`. List the honored env vars (the in-scope set).
3. **`hm-module.md`:**
   - Document the `programs.modde.database` option block (backend/url/host/port/
     name/user/passwordFile), mapping `name` → the `dbname` settings key.
   - Document the three `databaseAssertions` and when they fire (post-Phase-04
     wording: postgres needs `url` or at least `name`; `url` XOR discrete;
     connection fields only when postgres).
   - Note that the module sets `home.sessionVariables` so the backend applies to
     sessions started **after** activation, and that `modde config set-database`
     is the immediate bridge (matches the runtime-gap memory).
   - Note (per Phase 04) that socket dirs / sslmode go through `url`.
4. **`cli.md`:** add a `config` section documenting `config show` (reports the
   resolved backend + connection, flagging env overrides), `config set-database`
   (set/clear fields, revert to sqlite), and `config test` (connection doctor).
5. **Build the book.** `mdbook build docs` (or the flake's `docs` output) is warning-free;
   update `docs/src/SUMMARY.md` if a new page/anchor was added.

## Acceptance criteria

- [ ] `settings-file.md` documents all `[database]` keys, a TOML example, and a
  precedence subsection naming **only** the honored env vars.
- [ ] `hm-module.md` documents the `database` option block, the `name`→`dbname`
  mapping, the three assertions (post-Phase-04 wording), and the
  sessionVariables/activation timing note.
- [ ] `cli.md` documents `config show`/`set-database`/`test`.
- [ ] No doc mentions an unhonored env var, a `sslmode` enum, or a `dataDir` HM
  option as existing features.
- [ ] The mdBook builds without warnings; `SUMMARY.md` updated if needed.

## Files likely touched

- `docs/src/**/settings-file.md`, `docs/src/**/hm-module.md`, `docs/src/**/cli.md`
  (confirm exact paths in step 1).
- `docs/src/SUMMARY.md` (only if a page/anchor was added).

## Pitfalls

- **Symptom:** docs promise discrete env vars but they don't work. **Cause:** this
  sub-layer ran before Phase 01. **Recovery:** land last (after 01/03/04); if you
  must write early, scope to BACKEND/URL/PASSWORD_FILE and mark the rest pending.
- **Symptom:** docs contradict the HM assertion messages. **Cause:** documented
  the pre-Phase-04 (host-required) wording. **Recovery:** mirror the relaxed
  `name`-only assertion text from Phase 04.
- **Symptom:** resurrecting a fenced non-gap (claiming `sslmode`/`dataDir`
  options exist). **Cause:** inferring from the audit's improvement list.
  **Recovery:** only the `url`-escape-hatch note is in scope; no invented options.
- **Symptom:** `name` vs `dbname` confuses readers. **Cause:** the three spellings.
  **Recovery:** state explicitly that HM `database.name` / env `MODDE_DATABASE_NAME`
  set the settings `dbname` key.

## Reference

- Phase README + merge plan + fenced non-gaps: [README.md](./README.md),
  [../README.md](../README.md).
- Behavior to document: [../01-core-honor-discrete-env.md](../01-core-honor-discrete-env.md),
  [../03-cli-show-honesty-and-doctor.md](../03-cli-show-honesty-and-doctor.md),
  [../04-hm-module-assertions-and-eval-checks.md](../04-hm-module-assertions-and-eval-checks.md).
- Settings shape: [crates/modde-core/src/settings.rs:42-92](../../../../../crates/modde-core/src/settings.rs).
