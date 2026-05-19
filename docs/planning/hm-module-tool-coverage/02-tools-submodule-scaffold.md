# Phase 02 — Scaffold the per-profile `tools` submodule and activation calls

> **Recommended Codex model: GPT 5.4-mini / medium**
>
> Mechanical Nix scaffold + activation-script edits with one
> moderate design decision (failure handling per `modde tool` call).
> The shape is locked by Phase 01's DECISION.md, so this is
> sub-agent-level execution against a concrete spec, not orchestrator
> work. `5.4-mini/medium` matches the routing matrix for a moderate
> sub-agent task. Bump to `5.4/medium` only if the decision in 01
> diverges meaningfully from the recommended hybrid default.

## Working tree

`/data/nvme0/can/Projects/rs-modde` (this repo). Depends on
[01](./01-schema-and-activation-contract.md) — read
`DECISION.md` first to confirm the option shape and activation
contract before editing the module. Touches the same file
([nix/hm-module.nix](../../nix/hm-module.nix)) that Phases 03–04 will
edit; if 03/04 fan out concurrently they must rebase on this
phase's commit before starting.

## Goal

Every profile in `programs.modde.profiles.<name>` can declare a
`tools` attrset whose keys are tool IDs (`mangohud`, `vkbasalt`,
`gamemode`, `reshade`, `optiscaler`, `proton`) and whose values
configure each tool. Home Manager activation translates that
declaration into `modde tool enable` / `modde tool configure` /
optional `modde tool apply` calls per profile, in the order locked
by Phase 01. After this phase, every key recognised by
[`settings_schema()`](../../crates/modde-games/src/tools/mod.rs#L452)
is settable from Nix via a free-form `settings = { ... }` attrset.

The release-pinning option (`release.tag` / `release.asset`) and
OptiScaler's typed per-game profile selector are deferred to Phase
04. Typed per-tool options (replacing `settings = { ... }` with
named options derived from each tool's schema) are deferred to
Phase 03.

## Why this matters now

Without this scaffold, every Phase 03/04 edit has to invent its own
option location and activation hook. Landing the generic shape
first means 03/04 are localised, additive edits — no module-wide
restructures.

The originating symptom: a user wants to declare in Nix that
`profiles.skyrim-se` has MangoHud enabled with
`fps_limit=60`. Today this requires hand-writing a
`home.activation` block that shells out to `modde tool enable
mangohud --game skyrim-se` and `modde tool configure mangohud
--game skyrim-se -- fps_limit=60`. Nothing about that is hard,
which is exactly why it should be in the module.

## Out of scope

- Generating typed per-tool options from `settings_schema()` — Phase
  03 owns that.
- `release.{tag,asset}` first-class option and Nix-eval-time fetch —
  Phase 04.
- OptiScaler's typed `profile` enum from
  [`optiscaler_profiles`](../../crates/modde-games/src/tools/mod.rs#L45)
  — Phase 04.
- New tools in the Rust registry.
- Docs site (`docs/site/...`) edits — Phase 05.
- A NixOS VM test — Phase 05.

## Plan

1. **Re-read [01's DECISION.md](./DECISION.md).** Pull out the exact
   option shape and activation order. The rest of this phase
   assumes the recommended default (free-form `settings`, tool
   calls after `modde deploy`, failures warn-and-continue). If
   DECISION.md picks differently, adjust steps 3 and 5 to match.
2. **Define the tool submodule type in
   [nix/hm-module.nix](../../nix/hm-module.nix).** Add a
   `toolType` `lib.types.submodule` near `profileType` at
   [nix/hm-module.nix:8](../../nix/hm-module.nix#L8). Options:
   - `enable` — `bool`, default `false`. Drives whether activation
     calls `modde tool enable` vs `modde tool disable`.
   - `settings` — `attrsOf (oneOf [bool int float str (listOf str)])`,
     default `{}`. Free-form key/value pairs translated to
     `modde tool configure <tool> --game <id> -- key=value ...`.
   - `applyOnActivation` — `bool`, default `false` for tools that
     write files into the game directory (`reshade`, `optiscaler`)
     and `false` for everything else. Drives whether activation
     invokes `modde tool apply <tool> --game <id>` after configure.
3. **Add `tools` to `profileType`.** New option:
   ```nix
   tools = lib.mkOption {
     type = lib.types.attrsOf toolType;
     default = {};
     description = "Per-tool configuration for this profile.";
   };
   ```
   Place it after `nexusCollection` at
   [nix/hm-module.nix:99-114](../../nix/hm-module.nix#L99-L114).
4. **Add a known-tool-id assertion.** Hard-code the set
   `["mangohud" "vkbasalt" "gamemode" "reshade" "optiscaler"
   "proton"]` in the module file as `knownToolIds` (mirroring
   `bethesdaGames` at
   [nix/hm-module.nix:195](../../nix/hm-module.nix#L195)). Add an
   assertion per profile: every key in `profile.tools` must be in
   `knownToolIds`. Message: `programs.modde.profiles.<name>.tools.<id>:
   unknown tool '<id>'. Known tool IDs: <list>`.
5. **Emit per-tool activation snippets.** Add a `toolActivation`
   helper alongside `profileActivation` at
   [nix/hm-module.nix:188](../../nix/hm-module.nix#L188). For each
   `(toolId, toolCfg)` in `profile.tools`:
   - If `toolCfg.enable == false`: emit `modde tool disable <toolId>
     --game <game>` and continue.
   - Else emit, in order:
     - `modde tool enable <toolId> --game <game>`
     - For each `(key, value)` in `toolCfg.settings`, append to a
       single `modde tool configure <toolId> --game <game> --
       key=value [key=value ...]` invocation. Use `lib.escapeShellArg`
       on the rendered `key=value` strings. Booleans render as
       `true`/`false`, lists join with `,`.
     - If `toolCfg.applyOnActivation == true`: emit `modde tool
       apply <toolId> --game <game> || echo "modde: tool apply
       failed for <toolId>/<name>"`.
   - Wrap each `modde tool` invocation with `|| echo "modde: tool
     <verb> failed for <toolId>/<name>"` to match the existing
     warn-and-continue style at
     [nix/hm-module.nix:253,258](../../nix/hm-module.nix#L253).
6. **Call `toolActivation` from `profileActivation`.** Append the
   tool snippets after the `modde deploy` line in both branches of
   the if/else chain at
   [nix/hm-module.nix:243-258](../../nix/hm-module.nix#L243-L258).
   Both the Wabbajack and the no-Wabbajack path must invoke the
   tool snippets — the Wabbajack branch already deploys, so tools
   come last.
7. **Sanity-check rendering.** Add a minimal evaluator-only check
   to [flake.nix's `checks` block](../../flake.nix#L521) that
   evaluates the module against a fixture with a profile that
   enables `mangohud` with one `bool` setting and one `int`
   setting, and asserts the rendered activation string contains
   the expected `modde tool` invocations. Reuse the existing
   `hm-module` check stanza pattern at
   [flake.nix:521](../../flake.nix#L521).
8. **Smoke-run.** From the repo root:
   ```bash
   nix flake check --impure
   nix eval --impure --raw .#homeManagerModules.modde \
     2>/dev/null | head -c 200 # smoke: module evaluates
   ```
   No `modde` binary should run — this is eval-only.

## Acceptance criteria

- [ ] `nix flake check --impure` passes with the new module
      changes.
- [ ] The new evaluator check in `flake.nix` succeeds and rejects
      a fixture that names an unknown tool ID (`programs.modde.profiles.x.tools.notatool`).
- [ ] Evaluating a fixture with
      `profiles.test.tools.mangohud.enable = true` and
      `profiles.test.tools.mangohud.settings.fps_limit = 60`
      produces an activation string containing
      `modde tool enable mangohud --game ...`,
      `modde tool configure mangohud --game ... -- fps_limit=60`,
      and **no** `modde tool apply` call (default for mangohud).
- [ ] Evaluating the same fixture with `reshade.applyOnActivation =
      true` adds `modde tool apply reshade --game ...` to the
      activation string.
- [ ] Setting `tools.<id>.enable = false` emits `modde tool disable
      <id> --game ...` and skips `configure`/`apply`.
- [ ] No new direct dependencies on `pkgs.fetchurl` for tool data
      (that's Phase 04's concern).
- [ ] All `modde tool` invocations in the activation script use
      `lib.escapeShellArg` on tool IDs, game IDs, and `key=value`
      pairs.

## Files likely touched

- [nix/hm-module.nix](../../nix/hm-module.nix) — add `toolType`,
  `tools` option on `profileType`, `toolActivation` helper,
  `knownToolIds` assertion, and the activation invocation in both
  if/else branches.
- [flake.nix](../../flake.nix) — extend the existing `hm-module`
  check at line 521 with the fixtures from step 7.

## Pitfalls

- **Shell-quoting bool/int values.** Nix renders `true`/`false`/
  integers as bare tokens; the CLI parses them in
  [`handle_configure`](../../crates/modde-cli/src/commands/tool.rs#L514-L573).
  Use `builtins.toJSON` or explicit type matches when converting
  Nix values to `key=value` strings — naïvely interpolating an
  attrs value like `true` produces the string `1` from `toString`.
- **Forgetting `--game`.** Every `modde tool` subcommand requires
  `--game <id>`. The activation helper closes over `profile.game`;
  if you forget to pass it, the CLI errors with "unsupported
  game". Cover with the evaluator check fixture.
- **Mixing `tools.<id>.enable = false` with `applyOnActivation =
  true`.** Disable should override apply. Verify in the
  rendering helper before emitting `apply`.
- **Setting `_game_id` from Nix.** The CLI handlers inject
  `_game_id` themselves at
  [crates/modde-cli/src/commands/tool.rs:482,561](../../crates/modde-cli/src/commands/tool.rs#L482).
  The module must not let users set it (filter the key in step 5
  with a warning), or DB rows end up with two different
  `_game_id` values.
- **Activation ordering.** Tool calls must come *after* `modde
  deploy`. Putting them before deploy means OptiScaler's
  apply-into-game-dir runs while the deploy is mid-flight on the
  same files — race condition for the same paths.

## Reference

- Phase 01 DECISION: [./DECISION.md](./DECISION.md)
- HM module: [nix/hm-module.nix](../../nix/hm-module.nix)
- Tools trait + registry:
  [crates/modde-games/src/tools/mod.rs](../../crates/modde-games/src/tools/mod.rs)
- CLI handlers:
  [crates/modde-cli/src/commands/tool.rs](../../crates/modde-cli/src/commands/tool.rs)
- Flake checks block:
  [flake.nix:521](../../flake.nix#L521)
- Downstream: [03](./03-typed-schema-codegen.md) keeps the
  free-form fallback and adds typed options for the selected small
  tools; [04](./04-release-pinning-and-presets.md) adds release
  pinning and OptiScaler profile enum; [05](./05-docs-and-nix-check.md)
  is the docs + VM test pass.
