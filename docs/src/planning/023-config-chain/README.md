# Plan: Fix and harden the modde configuration chain (core resolution → CLI → HM module → canix)

> **Recommended Codex model for plan-set orchestration: GPT 5.5 medium**
>
> Coordinating this set is bounded: five mostly file-disjoint phases across three
> repos (rs-modde, canix, docs/CI) with one clear keystone dependency — the
> modde-core resolver (Phase 01) must land before the CLI honesty fixes (Phase
> 03) and before the docs can describe accurate behavior (Phase 05). The
> per-phase files carry the detail and the frontier-free risk; the coordination
> itself is a clean dependency chain with separable repos, so `medium` holds the
> bar. Not `high`: there is no cross-cutting design ambiguity to hold in one head
> — the audit already settled the shape of every fix.

## Scope and current state

modde gained a PostgreSQL backend (alongside SQLite) configurable via
`settings.toml`, env vars, and the Home Manager module. An evidence-backed audit
of the **entire configuration chain** — modde-core resolution → modde-cli
`config` → `nix/hm-module.nix` → canix wiring — found the chain is **correct only
on the `url` path** (which is exactly what atlas uses today, so every defect is
currently latent, not breaking production).

**The keystone defect (chain-wide).** The HM module exports
`MODDE_DATABASE_HOST/PORT/NAME/USER` into both `home.sessionVariables` and the
activation script ([nix/hm-module.nix:620-623](../../../../nix/hm-module.nix)), but
modde's runtime **never reads those four env vars**. `open_postgres`
([crates/modde-core/src/db/mod.rs:188-230](../../../../crates/modde-core/src/db/mod.rs))
consults the environment only for `MODDE_DATABASE_URL` (line 192) and
`MODDE_DB_PASSWORD_FILE` (line 216); the discrete `host/port/dbname/user` come
**exclusively from `settings.toml`** (lines 199-213). So a discrete-only postgres
HM config flips the backend on (`BACKEND` *is* honored) but builds a connection
from an empty settings struct and **hard-fails** at
`dbname.ok_or_else(... "postgres backend selected but no database name configured")`
(lines 206-208). The same dead-env contract is mirrored by `config show`, whose
module doc falsely claims it "reports both" env and settings. This is a true
precedence asymmetry: `url` and `password_file` get env-over-settings; the four
discrete fields do not.

Fixing modde-core to honor the discrete fields (Phase 01) is the **enabler** that
makes the HM options, the CLI `show`, the assertions, and the docs honest.

Beyond the keystone: the CLI `set-database` is write-only-additive (cannot unset
stale postgres fields when reverting to sqlite; `--url ""` persists `Some("")`);
the canix flake pins modde at an unpublished local `git+file://` rev; modde and
skillnet co-tenant the `can` database; and the whole DB surface has **zero** test
or eval-check coverage and **zero** user docs.

This plan implements the audit's **confirmed** fixes (G1–G17, adversarially
verified — none refuted). Severity, evidence, and the confirmed fix for each gap
live in the phase that owns it.

## Phase table

| Phase | File | Gaps | Depends on | Touches | Can parallel with |
|---|---|---|---|---|---|
| 01 | [01-core-honor-discrete-env.md](./01-core-honor-discrete-env.md) | G1, G13, G15 | — | `modde-core/src/{db/mod.rs,settings.rs,error.rs}` | 02, 04 |
| 02 | [02-canix-publish-repin-dedicated-db.md](./02-canix-publish-repin-dedicated-db.md) | G4, G8 | — (publish P1 first if you want the fix in the first repin) | canix `flake.nix`/`flake.lock`/`postgres.nix`/`can.nix` (+ rs-modde push) | 01, 03, 04 |
| 03 | [03-cli-show-honesty-and-doctor.md](./03-cli-show-honesty-and-doctor.md) | G2, G3, G11 | **01** | `modde-cli/src/commands/config.rs`, `src/main.rs` | 02, 04 |
| 04 | [04-hm-module-assertions-and-eval-checks.md](./04-hm-module-assertions-and-eval-checks.md) | G5, G9, G17 | — | `nix/hm-module.nix`, `flake.nix` | 01, 02, 03 |
| 05 | [05-tests-and-docs/](./05-tests-and-docs/README.md) | G6, G7, G10, G12, G14 | **01, 03, 04** | modde-core tests, modde-cli tests, `.forgejo/`, `docs/src/**` | internally parallel sub-layers |

## Parallelism layer

- **Wave 0 — Phase 01, Phase 02, Phase 04.** All start from the current tree and
  touch disjoint files/repos: 01 is modde-core (`db/`, `settings.rs`); 04 is Nix
  (`hm-module.nix`, `flake.nix`); 02 is the separate canix repo (+ an outward
  rs-modde push). 02 is *most useful* if 01 is pushed first so the first repin
  carries the discrete-env fix — otherwise repin to current HEAD now and re-bump
  after 01 (cheap). Phase 05's two independent slices (the `DbBackend::parse`
  tests of G14 and the postgres-parity CI job of G10) can also be pulled into
  this wave — they don't depend on 01's resolver.
- **Wave 1 — Phase 03.** Unlocked by Phase 01: the CLI reuses 01's shared
  resolver helper for `config show` discrete-env overlay, so it must follow.
- **Wave 2 — Phase 05 (remaining slices).** Unlocked by 01 (behavior to test),
  03 (CLI surface to test), and 04 (assertion messages to document). Its
  sub-layers (core-resolution tests, CLI integration tests, docs) are mutually
  disjoint and fan out; the docs sub-layer must come last because it describes
  the post-01/03/04 behavior. **Plan exhausted** after this wave.

## Whole-set acceptance criteria

- [ ] A discrete-only postgres config (`MODDE_DATABASE_HOST/PORT/NAME/USER`, no
  `url`) connects successfully — the keystone defect is gone. Proven by a
  resolution test (Phase 05 sub-01) asserting the assembled `PgConnectOptions`
  host/port/database/username, and end-to-end against a throwaway PG.
- [ ] Env-over-settings precedence is **uniform** across `url`, `password_file`,
  **and** the four discrete fields; `config show` reports the resolved value and
  flags which fields are env-overridden.
- [ ] `modde config set-database` can both set and **clear** every field; reverting
  to sqlite leaves no stale postgres fields; `--url ""` normalizes to unset.
- [ ] `cargo build` (default + `--no-default-features`), `cargo clippy --workspace
  --all-targets -- -D warnings`, and `cargo test` are green across the workspace.
- [ ] `nix flake check` includes passing fixtures for the database HM block and a
  failing fixture for **each** of the three `databaseAssertions`.
- [ ] The canix `modde` input resolves from a **published** ref (no `git+file://`
  to an unpublished rev), and modde no longer shares the `can` database.
- [ ] `docs/src` documents the `[database]` settings keys, the **honored** env
  vars, the HM `database` options + assertions, and the `config` CLI — describing
  only behavior the runtime actually implements post-Phase-01.

## Global constraints (apply to every phase)

- **Implement the audit's option (a), not (b).** Make the discrete fields *work*
  end-to-end. Do **not** delete the discrete HM options/assertions/CLI flags in
  favor of url-only — that was considered and rejected (it discards shipped
  surface and would have the module rewrite mutable `settings.toml` on every
  rebuild, racing the UI's writes).
- **Do not resurrect the fenced non-gaps** (see "Out of scope" below).
- **`MODDE_DATABASE_NAME` maps to the `dbname` field** (settings/CLI). Three
  spellings bridge one concept (HM `name`, env `NAME`, core `dbname`, CLI
  `--name`); keep the env reader on `MODDE_DATABASE_NAME` and pin the mapping
  with a test, or a future `MODDE_DATABASE_DBNAME` reader will silently re-break it.
- **CI parity:** `cargo clippy -p <crate> --all-targets -- -D warnings` (warnings
  are errors). Every phase ends clean.
- **The password is never written to `settings.toml` or the Nix store** — only
  the *path* (`password_file` / `MODDE_DB_PASSWORD_FILE`) is configured; modde
  reads the secret at runtime.

## Out of scope (verified non-gaps — do not implement)

- **`settings.rs` docstring is not lying.** Lines 36-41 deliberately list only
  `MODDE_DATABASE_BACKEND/URL/MODDE_DB_PASSWORD_FILE` as env overrides — accurate
  today. (The *`config.rs` module doc* overclaims; that's the real doc bug, owned
  by Phase 03.) Don't conflate the two.
- **No "silent wrong DB" framing.** The discrete-only failure is a *hard* error
  (`dbname.ok_or_else`), not a silent misconnect. Frame G1 as a hard failure.
- **The `can` DB/role are already declarative** (`postgres.nix` `ensureDatabases`/
  `ensureUsers`/`ensureDBOwnership`). The issue is *co-tenancy* (G8 → "split modde
  out"), not missing provisioning. Don't write a "provision the DB from scratch"
  task.
- **No `sslmode` enum / first-class TCP+TLS option now.** The `url` escape hatch
  covers it; land only the doc note (G17).
- **No `dataDir` HM option as a "fix".** It's a coverage nicety (the
  `MODDE_DATA_DIR` env hook already works); not a defect — leave out unless asked.

## Reference

- Audit dossier (this plan's brief): workflow `w1vd9w13d` synthesis output.
- Keystone evidence: [crates/modde-core/src/db/mod.rs:188-230](../../../../crates/modde-core/src/db/mod.rs),
  [nix/hm-module.nix:608-625](../../../../nix/hm-module.nix),
  [crates/modde-core/src/settings.rs:42-92](../../../../crates/modde-core/src/settings.rs).
- Prior plan sets for shape/convention:
  [022-iced-async-db](../022-iced-async-db/README.md),
  [021-green-release](../021-green-release/README.md).
- Propagating the rs-modde change into canix: the `update-canix` skill.
