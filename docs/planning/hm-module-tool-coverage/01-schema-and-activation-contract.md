# Phase 01 — Lock the tool-config schema and activation contract

> **Recommended Codex model: GPT 5.5 / medium**
>
> This phase is orchestrator-level: every later phase consumes the
> schema, the activation order, and the idempotency contract decided
> here. The reasoning is bounded — three concrete forks (free-form
> attrs vs typed-per-tool, declarative-only vs imperative-during-
> activation, eager-fetch vs run-time fetch) — so `medium` is enough.
> `high` would only be warranted if the design space were genuinely
> open-ended.

## Working tree

`/data/nvme0/can/Projects/rs-modde` (this repo). No external repos.
Sequential prerequisite for every other phase in this set — do not
start 02–05 until this commit lands and the decision is final.

## Goal

A single, durable answer to three questions, written into
`docs/planning/hm-module-tool-coverage/DECISION.md`:

1. **Option shape.** What does
   `programs.modde.profiles.<name>.tools.<tool-id>` look like? Is
   `settings` a free-form `attrsOf anything`, or do we generate
   per-tool typed options from each tool's
   [`settings_schema()`](../../crates/modde-games/src/tools/mod.rs#L452)?
2. **Activation contract.** In what order does the HM
   activation script call `modde tool enable` / `modde tool
   configure` / `modde tool apply` relative to the existing
   `modde install` / `modde deploy` flow at
   [nix/hm-module.nix:290-296](../../nix/hm-module.nix#L290-L296)?
   Which calls are idempotent, which are guarded by a state check,
   and which non-zero exits are fatal vs warn-and-continue?
3. **Release-backed tools.** Does the HM module fetch
   release assets at Nix evaluation time (via `pkgs.fetchurl` with a
   pinned hash) and hand modde a local path, or does the activation
   script shell out to `modde tool install-release` and let the CLI
   fetch them at switch time? OptiScaler and Proton both have
   release support today; ReShade may grow it later.

After this phase, no further bikeshedding on these three questions
is allowed in 02–05.

## Why this matters now

The current HM module at [nix/hm-module.nix](../../nix/hm-module.nix)
covers profile selection, Wabbajack/Nexus modlist sources, and a
single `modde deploy` activation call. It does not expose any of the
six tools registered at
[crates/modde-games/src/tools/mod.rs:594-602](../../crates/modde-games/src/tools/mod.rs#L594-L602)
(`mangohud`, `vkbasalt`, `gamemode`, `reshade`, `optiscaler`,
`proton`). Tool configuration lives in the SQLite DB managed by
`modde tool enable/configure/apply` —
[crates/modde-cli/src/commands/tool.rs:443-573](../../crates/modde-cli/src/commands/tool.rs#L443-L573)
— and is invisible to Nix.

A Nix-managed setup currently has to wrap `modde tool` calls in
`home.activation` scripts by hand. That's the kind of escape hatch
that says "your module is missing surface area". The fork:

1. **Free-form `settings = { ... }`**: simplest scaffold. One submodule
   type covers all six tools. No typing, no auto-completion. Users
   discover keys by reading
   [`settings_schema()`](../../crates/modde-games/src/tools/mod.rs#L452)
   in the Rust source.
2. **Generated typed options per tool**: nicer ergonomics, type
   checks, Nix-side validation. Requires a build-time step that
   exports each tool's schema as Nix expressions.
3. **Hybrid**: typed for the small tools (gamemode, vkbasalt,
   reshade — total ≤ 10 keys), free-form for the heavy ones
   (mangohud has ~110 keys, optiscaler ~29, proton ~33). Lets us
   ship 02 quickly without committing to a codegen pipeline.

Costs/benefits live in `DECISION.md` after this phase runs. The
recommended default if undecided: **hybrid with free-form everywhere
in Phase 02, then typed export added in Phase 03 for the small
tools** — keeps 02 mechanical and unblocks the rest.

Similar fork on activation order:

1. **Tool calls run after `modde deploy`**: matches today's flow,
   so a broken tool config can't break the modlist install. Tool
   apply (which writes DLLs into the game dir) lands last.
2. **Tool calls run before `modde install`**: lets Wabbajack
   archives reference tool outputs. Risk: a misconfigured tool
   blocks the modlist install entirely.

Recommended default: **after `modde deploy`**, with per-tool
failures logging a warning and continuing (mirroring the existing
`|| echo "modde: deploy failed for '${name}'"` pattern at
[nix/hm-module.nix:253,258](../../nix/hm-module.nix#L253)).

## Out of scope

- Writing any of the actual Nix option types (Phase 02).
- The Rust-side codegen for typed schema export (Phase 03).
- Release pinning + OptiScaler per-game presets (Phase 04).
- Docs site updates (Phase 05).
- Adding new tools to the registry — that's a separate change in
  `crates/modde-games/src/tools/`.

## Plan

1. **Walk the existing surface.** Read in order:
   [nix/hm-module.nix](../../nix/hm-module.nix) (current module),
   [crates/modde-games/src/tools/mod.rs](../../crates/modde-games/src/tools/mod.rs)
   (trait + types),
   [crates/modde-cli/src/commands/tool.rs](../../crates/modde-cli/src/commands/tool.rs)
   (CLI handlers the activation will invoke),
   [crates/modde-cli/src/main.rs:772-898](../../crates/modde-cli/src/main.rs#L772-L898)
   (clap subcommand shape).
2. **Inventory every tool's schema.** For each of the six tools, list:
   number of settings, types (`Bool` / `Select` / `Number` /
   `Path` / `Text` / `TriStateBool`), advanced flag, whether the
   tool supports releases. Source of truth:
   `settings_schema()` in each
   `crates/modde-games/src/tools/<tool>.rs`. Capture the totals so
   Phase 03 has a budget.
3. **Decide option shape.** Pick one of the three forks above. Write
   the decision as: "`profiles.<name>.tools` is `attrsOf
   <toolSubmoduleType>` with options `enable`, `settings`,
   `release`, `applyOnActivation`". Specify whether `settings` is
   typed-per-tool or `attrsOf anything`. If hybrid, list which
   tools get typed options in Phase 03.
4. **Decide activation contract.** Specify exactly: pre-install
   step ordering, idempotency strategy (e.g. always call `modde
   tool enable` — handler is idempotent; only call `modde tool
   apply` when previous applied-files manifest is empty or
   `applyOnActivation` is `true`), and failure handling per call.
5. **Decide release-fetch strategy.** Pick eager (Nix-evaluated
   fetchurl, hash required) or lazy (activation calls `modde tool
   install-release`). Note the consequences: eager requires a
   `release = { tag, asset, hash, url }` option; lazy requires
   network at activation time and runs as a non-pure step.
6. **Write `DECISION.md`.** Four sections matching the four
   questions above (option shape, activation contract,
   release-fetch, deferred-to-later). 200–400 words. Link from the
   commit message.
7. **Skim each follow-up phase doc (02–05).** Verify that the
   recommendations they make do not contradict the decisions
   recorded here. If they do, update them in this commit.

## Acceptance criteria

- [ ] `docs/planning/hm-module-tool-coverage/DECISION.md` exists,
      committed in this phase, and answers the three questions
      (option shape, activation contract, release-fetch strategy)
      with one-line answers each plus a paragraph of rationale.
- [ ] The DECISION.md table lists all six tools (`mangohud`,
      `vkbasalt`, `gamemode`, `reshade`, `optiscaler`, `proton`)
      with their settings count and whether they get typed Nix
      options in Phase 03 or stay free-form.
- [ ] Phases 02–05 phase docs are consistent with the decision — a
      grep for `# Phase 0[2-5]` and the recommendation lines does
      not contradict DECISION.md.
- [ ] Commit message names the chosen option shape, activation
      order, and release-fetch strategy in one sentence each.
- [ ] `nix flake check --impure` still passes (no module changes
      land in this phase — only a doc).

## Files likely touched

- `docs/planning/hm-module-tool-coverage/DECISION.md` (new).
- `docs/planning/hm-module-tool-coverage/02-tools-submodule-scaffold.md` (only if its current recommendations need a touch-up after the decision).
- `docs/planning/hm-module-tool-coverage/03-typed-schema-codegen.md` (same).
- `docs/planning/hm-module-tool-coverage/04-release-pinning-and-presets.md` (same).
- `docs/planning/hm-module-tool-coverage/05-docs-and-nix-check.md` (same).
- `TODO.md` — one line under a "HM module tool coverage" subsection naming the chosen option shape, so it's visible without reading the phase doc.

## Pitfalls

- **Conflating `settings_schema()` typing with serialisation
  format.** The serialised JSON blob on disk (`ToolConfig.settings`)
  stays free-form regardless of whether Nix presents typed options.
  Don't promise "type-safe storage" — promise type-safe *evaluation*.
- **Forgetting `_game_id` is injected by the CLI.** The CLI handlers
  in [crates/modde-cli/src/commands/tool.rs:482,561](../../crates/modde-cli/src/commands/tool.rs#L482)
  set `_game_id` into the config before generating the config file.
  The Nix module must not collide on that key, and 03's typed schema
  must filter it out.
- **Deciding eager-fetch without checking OptiScaler's release
  pipeline.** OptiScaler installs from `.7z` archives extracted on
  disk —
  [crates/modde-games/src/tools/optiscaler.rs:723-738](../../crates/modde-games/src/tools/optiscaler.rs#L723).
  Eager-fetch means we'd hand modde a `.7z` path it can extract,
  which it already does. Verify that path before promising eager-
  fetch in DECISION.md.
- **Skipping the idempotency audit.** Read each `handle_*` in
  [crates/modde-cli/src/commands/tool.rs](../../crates/modde-cli/src/commands/tool.rs)
  and mark idempotent vs not. `handle_enable` is idempotent;
  `handle_apply` re-writes the applied-files manifest each time and
  is *almost* idempotent but does perform writes — note this
  explicitly so 02's activation script gets it right.
- **Promising parity with the GUI.** The Bevy UI shows tools via the
  same trait surface but with different ergonomics (forms vs Nix
  attrs). Don't promise "everything the UI can do, Nix can do" —
  promise "everything stored in `tool_configs` can be expressed in
  Nix".

## Reference

- Current HM module: [nix/hm-module.nix](../../nix/hm-module.nix)
- Tools trait + registry: [crates/modde-games/src/tools/mod.rs](../../crates/modde-games/src/tools/mod.rs)
- CLI handlers the activation will invoke: [crates/modde-cli/src/commands/tool.rs](../../crates/modde-cli/src/commands/tool.rs)
- Originating ask: chat thread "add full tool coverage to the nix HM module so we can configure each game", 2026-05-19.
- Downstream phases: [02](./02-tools-submodule-scaffold.md), [03](./03-typed-schema-codegen.md), [04](./04-release-pinning-and-presets.md), [05](./05-docs-and-nix-check.md) — all consume the decision recorded here.
