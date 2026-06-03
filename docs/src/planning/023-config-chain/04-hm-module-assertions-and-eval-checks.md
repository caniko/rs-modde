# Phase 04 — HM module: relax the discrete assertion, add eval checks, document socket/sslmode

> **Recommended Codex model: GPT 5.5 medium**
>
> Moderate, self-contained Nix work: relax one assertion, add flake `checks`
> that evaluate the database HM block (passing fixtures + one failure fixture per
> assertion), and add a couple of option-description doc notes. The Nix
> eval-harness plumbing is fiddly (you must thread `home.sessionVariables`
> through the existing test modules and `deepSeq` the rendered activation), which
> is why it is `medium` and not `low` — but it is bounded and has no design
> ambiguity, so `high` would be over-routing. A weaker model tends to write
> `checks` that pass vacuously (never forcing the assertion) or that don't
> actually render the activation script.

## Working tree

`/data/nvme0/can/Projects/rs-modde`, branch `trunk`. Independent of the Rust
phases for the eval-harness and assertion work — runs in parallel with Phases 01,
02, 03 (disjoint files: `nix/hm-module.nix`, `flake.nix`). One sequencing note in
the Plan: the "discrete vars are actually consumed by the runtime" claim is only
true once Phase 01 lands, so this phase asserts **export presence**, not runtime
consumption.

## Goal

The HM module's `database` assertions match modde's real resolution contract, the
flake's `checks` evaluate the database configuration (so a regression in the
option wiring or an assertion message fails `nix flake check`), and the option
descriptions tell users how to express socket dirs / TLS (which must go through
`url`). The module stops gating a stricter discrete contract than the runtime
enforces.

## Why this matters now

**G9 — assertion stricter than runtime.** `hasDiscrete = db.host != null &&
db.name != null && db.user != null`
([nix/hm-module.nix:591](../../../../nix/hm-module.nix)) requires host **and** user,
but modde's runtime (post-Phase-01, and even today for the dbname check) requires
only the database name — host/port/user are optional (sqlx defaults to the local
socket / peer user, which is exactly the atlas
`postgres:///...?host=/run/postgresql` style). So a valid `name`-only discrete
config is rejected by the module before it can work.

**G5 — zero eval coverage.** No flake check sets `programs.modde.database`.
Neither HM test harness in [flake.nix:548-559 and 588-600](../../../../flake.nix)
declares `home.sessionVariables`, and none of the three `databaseAssertions`
([hm-module.nix:592-606](../../../../nix/hm-module.nix)) has a failing fixture. The
299d6fa `home.sessionVariables` wiring and the assertions are entirely untested —
a typo or a broken assertion message would ship green.

**G17 — socket/sslmode undocumented.** There is no first-class `sslmode` or
socket-dir option; the discrete `host` can't express a socket directory or TLS
mode, so those require the `url` escape hatch — but nothing tells the user that.

## Out of scope

- Adding a first-class `sslmode` enum or a TCP+TLS option (rejected as a feature;
  `url` covers it). Land **only** the doc note. (G17)
- The runtime change that makes discrete vars consumed (Phase 01). This phase
  asserts the module *exports* them; Phase 05/Phase 01 cover runtime consumption.
- A `dataDir` HM option (explicit non-gap — the `MODDE_DATA_DIR` env hook already
  works).

## Plan

1. **Relax `hasDiscrete`** ([hm-module.nix:591](../../../../nix/hm-module.nix)) to
   require only the database name: `hasDiscrete = db.name != null;`. Keep
   host/port/user optional. Update the assertion message at
   [hm-module.nix:595](../../../../nix/hm-module.nix) to: backend = "postgres"
   requires either `url`, or at least `name` (host/port/user optional, default to
   the local socket). Re-check the XOR assertion (596-600) and the
   "connection-fields-only-when-postgres" assertion (601-605) still read correctly
   with the relaxed definition. (G9)
2. **Thread `home.sessionVariables` through the test harnesses.** The two eval-HM
   modules in [flake.nix:548-559 and 588-600](../../../../flake.nix) (the stub
   modules used by the existing HM checks) don't declare `home.sessionVariables`;
   add the option (or a minimal stub) so the module's
   `home.sessionVariables = databaseEnvVars` assignment evaluates, and extend the
   string-rendering harness's `deepSeq` ([flake.nix:617-618](../../../../flake.nix))
   to force the rendered `modde-deploy` activation text. (G5)
3. **Add passing fixtures** as flake `checks`:
   - url-only: `database = { backend = "postgres"; url = "postgres:///x"; }` →
     assert the rendered activation/sessionVariables contain
     `MODDE_DATABASE_BACKEND` and `MODDE_DATABASE_URL` and **not** the discrete vars.
   - discrete: `database = { backend = "postgres"; name = "modde"; host = "h";
     port = 5432; user = "u"; }` → assert all of `MODDE_DATABASE_BACKEND/NAME/
     HOST/PORT/USER` are present (export-presence, per the sequencing note).
   - sqlite default: assert **no** `MODDE_DATABASE_*` appears (empty attrset case).
   Grep the rendered strings (the harness already renders activation text). (G5)
4. **Add a failing fixture per assertion** via the existing
   `mkHmModuleFailureCheck` helper (the pattern used for `profileAssertions`):
   - postgres with neither url nor name → matches the (relaxed) message at 595.
   - postgres with both url and discrete → matches 599.
   - sqlite with a connection field set → matches 605.
   Each check must assert the build **fails** with the expected message
   substring (so a vacuous pass is impossible). (G5)
5. **Document socket/sslmode in option descriptions** (G17): in the `host` and
   `url` `mkOption` `description`s, note that a socket directory
   (`host = "/run/postgresql"`) and TLS (`sslmode=...`) are expressed through
   `url` (or `host` as a path post-Phase-01), and that the discrete fields target
   the common TCP/socket case.
6. **(Optional, low) symmetry fixes:**
   - Fold `NEXUS_API_KEY_FILE` into `home.sessionVariables` too (it's
     activation-only at [hm-module.nix:716-718](../../../../nix/hm-module.nix), the
     same interactive-session gap the DB vars had before 299d6fa). (Improvement 7)
   - Add a one-line note to the `database` option doc that `home.sessionVariables`
     only affects sessions started **after** activation; `modde config
     set-database` is the immediate bridge. (Improvement 6)

## Acceptance criteria

- [ ] `hasDiscrete` requires only `db.name`; a `name`-only postgres HM config
  passes the assertions (no longer rejected for missing host/user).
- [ ] `nix flake check` runs and **passes** the three new database fixtures
  (url-only, discrete, sqlite-default) — each actually forcing the rendered
  activation/sessionVariables (not vacuous).
- [ ] `nix flake check` includes three failing fixtures, one per
  `databaseAssertion`, each asserting the expected message substring; flipping a
  fixture to a valid config makes that check fail (proving it's real).
- [ ] The `host`/`url` option descriptions document the socket-dir/sslmode →
  `url` guidance.
- [ ] `nix eval` of a `name`-only discrete config shows
  `home.sessionVariables` containing `MODDE_DATABASE_NAME` (+ BACKEND).

## Files likely touched

- `nix/hm-module.nix` — `hasDiscrete` (591) + message (595); option descriptions
  for `host`/`url`; optional NEXUS sessionVariables symmetry + doc notes.
- `flake.nix` — the two eval-HM stub modules (548-559, 588-600), the rendering
  `deepSeq` (617-618), and the new `checks` (passing + failing fixtures, via the
  existing `mkHmModuleFailureCheck`/`evalHm` helpers).

## Pitfalls

- **Symptom:** a new check passes even when the config is invalid. **Cause:** the
  fixture never forces the assertion (HM assertions only fire when the config is
  built/`deepSeq`'d). **Recovery:** use `mkHmModuleFailureCheck` (which expects a
  build failure) and confirm flipping the fixture to valid makes it fail.
- **Symptom:** `evalHm` errors with "option `home.sessionVariables` does not
  exist". **Cause:** the stub test module doesn't declare it. **Recovery:** add
  the option (or import enough of HM's lib) to the stub, per step 2.
- **Symptom:** the relaxed assertion now lets through a config the runtime still
  rejects. **Cause:** relaxed below what Phase 01 requires (dbname). **Recovery:**
  `hasDiscrete = db.name != null` is exactly the runtime's dbname requirement —
  keep them in lockstep; if Phase 01 changes the required field, mirror it here.
- **Symptom:** asserting "discrete vars are consumed at runtime" in a Nix check.
  **Cause:** confusing module export with runtime behavior. **Recovery:** Nix
  checks can only assert the module *exports* the vars; runtime consumption is
  Phase 01 + Phase 05's Rust tests.

## Reference

- Plan README + non-gaps (no `sslmode` enum / no `dataDir` option): [README.md](./README.md).
- Module: [nix/hm-module.nix:588-625](../../../../nix/hm-module.nix) (assertions +
  `databaseEnvVars`/`home.sessionVariables`), [716-722](../../../../nix/hm-module.nix)
  (activation, NEXUS env).
- Flake checks/harness: [flake.nix:548-620](../../../../flake.nix).
- Runtime contract this mirrors: [01-core-honor-discrete-env.md](./01-core-honor-discrete-env.md).
