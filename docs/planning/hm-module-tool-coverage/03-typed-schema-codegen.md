# Phase 03 — Export typed Nix options for small tools

> **Recommended Codex model: GPT 5.4 / medium**
>
> Genuinely a design step plus production code: a new CLI
> subcommand on the Rust side that walks every tool's
> `settings_schema()` and emits Nix expressions, plus the Nix
> side that imports them into the HM module's `toolType.settings`
> type. The reasoning involves choosing between in-tree static
> codegen (committed Nix file) vs build-time codegen (flake-level
> shell-hook) — a sub-agent complex decision. `5.4/medium` matches
> the matrix; bump to `5.4/high` only if a third option appears
> during step 2 (e.g. compile-time Nix derivation from a JSON
> schema). `5.4-mini` is too light for the design fork.

## Working tree

`/data/nvme0/can/Projects/rs-modde` (this repo). Depends on
[02](./02-tools-submodule-scaffold.md): the `toolType.settings`
type and `knownToolIds` assertion must already exist. Phase 04
depends on this for OptiScaler's typed `profile` enum; Phase 05
documents the resulting option names — both must rebase after this
phase commits.

## Goal

Keep the free-form `settings = attrsOf <oneOf primitives>` fallback
from Phase 02, and add **per-tool typed options only for the small
tools** selected in Phase 01's DECISION.md: `gamemode`, `vkbasalt`,
and `reshade`. The exporter may inventory every tool registered in
[`ALL_TOOLS`](../../crates/modde-games/src/tools/mod.rs#L594-L602).
The typed Nix shape for selected tools becomes:

```nix
profiles.test.tools.vkbasalt = {
  enable = true;
  settings = {
    enableOnLaunch = true;
    casSharpness = 0.4;
    toggleKey = "Home";
  };
};
```

…where `enableOnLaunch` is typed as `bool`, `casSharpness` as a
number, and `toggleKey` as `str`, all derived from the
[`ToolSettingSpec`](../../crates/modde-games/src/tools/mod.rs#L235)
each tool exposes via `settings_schema()`. Unknown keys produce a
Nix evaluation error for typed tools. Heavy tools (`mangohud`,
`optiscaler`, `proton`) remain free-form in this phase.

Booleans, enums (`Select`), numbers (with `min`/`max`/`step`),
text, and paths must all map to native Nix types so users get
auto-completion, evaluation errors on typos, and documentation in
the generated options reference.

## Why this matters now

Phase 02 ships free-form `settings` because typed options is work,
not because every tool needs immediate typing. Phase 01 deliberately
chooses a hybrid: type the small, stable schemas first and keep the
large/dynamic schemas free-form. Free-form has three concrete failure
modes that typed small tools can eliminate immediately:

1. **Typos silently drop.** `settings.enable_on_launch = true`
   (snake_case by habit) is a valid Nix attribute and the CLI accepts unknown
   keys without erroring —
   [`handle_configure`](../../crates/modde-cli/src/commands/tool.rs#L514-L573)
   stores any key into the JSON blob. The tool's `generate_config`
   then ignores it and the user has no signal.
2. **Type confusion.** Nix can render `true` as `1` through
   `toString`; the activation helper in Phase 02 has to special-
   case bools, ints, lists. Each new tool adds new shapes. With
   typed options Nix knows the type and the helper can dispatch on
   `lib.types`.
3. **No discoverability.** Nix users find available keys by
   reading the Rust source. Typed options surface in the
   home-manager options reference and in editor LSP completions.

## Out of scope

- Adding new tools or new `ToolSettingKind` variants. If a tool
  needs a new kind, that's a separate change in
  [tools/mod.rs](../../crates/modde-games/src/tools/mod.rs).
- Validating settings *values* beyond what `ToolSettingKind`
  expresses. E.g. mangohud's `gpu_junction_temp` can take any
  bool; we don't enforce "this GPU supports junction temp".
- Release pinning / OptiScaler per-game profile enum — Phase 04.
- Docs site updates — Phase 05.
- Auto-completion in the Bevy UI — separate concern.

## Plan

1. **Decide codegen lifecycle.** Two concrete options, pick one
   and record the choice in the commit message:
   - **(a) Static committed file.** Add `crates/modde-cli/src/commands/nix_schema.rs`
     exposing `modde dev export-tool-schema --out nix/tool-schema.nix`.
     Run it as a justfile target and commit
     `nix/tool-schema.nix`. The HM module imports it.
     Pro: zero IFD, simple. Con: a stale file is a real
     possibility; CI must enforce regeneration.
   - **(b) Build-time codegen via flake derivation.** A
     `pkgs.runCommand` invokes `modde dev export-tool-schema`
     during flake evaluation. Pro: always fresh. Con: introduces
     IFD (import-from-derivation), which complicates flake checks
     and is generally fragile for HM modules.
   - Recommendation: **(a)**, with a flake check that fails if
     `nix/tool-schema.nix` is stale relative to the Rust source
     (`git diff --exit-code` after re-running the exporter in CI).
2. **Inventory the schema surface.** From Phase 01's DECISION.md
   table, confirm counts:
   - `mangohud` (~114 keys, mostly bool + a few select/text)
   - `vkbasalt` (~6 keys)
   - `gamemode` (1 key)
   - `reshade` (~3 keys)
   - `optiscaler` (20 baseline keys, plus contextual profile/local/dynamic INI fields; includes `optiscaler_profile` —
     strip and let Phase 04 own it as an enum)
   - `proton` (~33 keys)
   The exporter must skip `_game_id`, `_*` underscore-prefixed
   keys, and any `release_*` keys (Phase 04's job). The HM module
   consumes typed output only for `gamemode`, `vkbasalt`, and
   `reshade` in this phase.
3. **Add `crates/modde-cli/src/commands/nix_schema.rs`.**
   Exposes one function: `handle_export(out_path: &Path) ->
   Result<()>`. It iterates
   [`all_tools()`](../../crates/modde-games/src/tools/mod.rs#L605),
   walks each tool's `settings_schema()`, and writes a single
   Nix file with one attrset per tool keyed by `tool_id`. Each
   tool's value is itself an attrset of option specs, e.g.:
   ```nix
   mangohud = {
     fps_limit = { type = "int"; default = null; description = "..."; advanced = false; min = 0; max = 999; step = 1; };
     cpu_temp = { type = "bool"; default = null; description = "..."; advanced = false; };
     fps_limit_method = { type = "enum"; values = ["early" "late"]; default = null; description = "..."; advanced = false; };
   };
   ```
   `null` defaults mean "no value injected unless the user sets
   one" — drives the activation helper to skip the key entirely.
4. **Wire a `modde dev export-tool-schema` subcommand.** Add a
   gated `dev` subcommand in
   [crates/modde-cli/src/main.rs](../../crates/modde-cli/src/main.rs)
   so it doesn't pollute the user-facing help (use clap's `hide =
   true`). Defaults the `--out` to `nix/tool-schema.nix`.
5. **Add a justfile target.** `just export-tool-schema` runs
   `cargo run -p modde-cli --quiet -- dev export-tool-schema`.
   Document in `CONTRIBUTING.md` (one line) that
   `nix/tool-schema.nix` is regenerated, not hand-written.
6. **Add a flake check for staleness.** In `flake.nix` checks:
   `pkgs.runCommand "tool-schema-fresh" {} ''
     cd $src
     cargo run -p modde-cli -- dev export-tool-schema --out /tmp/out.nix
     diff nix/tool-schema.nix /tmp/out.nix
   ''`. Wired through `crane`'s `cargoArtifacts` so it doesn't
   recompile the world. (If `crane` doesn't support this
   ergonomically here, fall back to a one-line `just check-tool-schema-fresh` target invoked from CI.)
7. **Rewrite `toolType` in [nix/hm-module.nix](../../nix/hm-module.nix).**
   Keep the generic `settings = attrsOf (oneOf [bool int float
   str (listOf str)])` from Phase 02 as the fallback for heavy tools,
   and add a per-tool branch for `gamemode`, `vkbasalt`, and
   `reshade`:
   ```nix
   let toolSchema = import ./tool-schema.nix; in
   toolType = name: lib.types.submodule {
     options = {
       enable = ...;
       applyOnActivation = ...;
       settings = lib.types.submodule {
         options = lib.mapAttrs (key: spec:
           lib.mkOption {
             type = nixTypeFromSpec spec;
             default = spec.default;
             description = spec.description;
           }
         ) toolSchema.${name};
       };
     };
   };
   ```
   `nixTypeFromSpec` maps `"bool"` → `lib.types.bool`, `"int"` →
   `lib.types.int`, `"float"` → `lib.types.float`,
   `"text"`/`"path"` → `lib.types.str`, `"enum"` →
   `lib.types.enum spec.values`, `"tri_state_bool"` →
   `lib.types.nullOr lib.types.bool`. `"read_only"` is excluded
   from the Nix surface (it's UI-only).
8. **Update the activation helper.** The Phase 02 helper iterates
   `profile.tools.<id>.settings` and renders `key=value`. With
   typed options, the helper must skip keys whose value is `null`
   (i.e. the user didn't set them — defaults stay implicit and
   the CLI uses its own defaults). For non-null values, render
   per type: bool → `true`/`false`, int/float → bare number,
   string → bare (the helper still passes through
   `lib.escapeShellArg`).
9. **Add evaluator-check fixtures.** Extend the Phase 02 fixture
   in `flake.nix` with a profile that:
   - Sets a known typed key (`tools.vkbasalt.settings.casSharpness =
     0.4`) — must evaluate.
   - Sets an unknown key
     (`tools.vkbasalt.settings.cas_sharpness = 0.4`) — must produce a
     `nix flake check` failure with a recognisable message.
   - Sets a wrong-type value (`tools.vkbasalt.settings.casSharpness =
     "fast"`) — must produce a type-mismatch error.
10. **Re-run the check.** `nix flake check --impure` from the repo
    root. All checks green.

## Acceptance criteria

- [ ] `nix/tool-schema.nix` exists, is regenerable via `just
      export-tool-schema`, and is committed alongside the Rust
      code in this phase.
- [ ] The exporter's output inventories all six tools, the HM module
      consumes typed settings only for `gamemode`, `vkbasalt`, and
      `reshade`, and the generated output omits
      `_game_id`, any `_`-prefixed key, and the `release_*`/
      `optiscaler_profile` keys reserved for Phase 04.
- [ ] `modde dev export-tool-schema --out -` (writing to stdout)
      produces deterministic output (sorted keys per tool, sorted
      tools at top level) — running it twice produces byte-
      identical files.
- [ ] The flake staleness check fails when `nix/tool-schema.nix`
      is hand-edited away from the generated form, and passes
      after a regeneration.
- [ ] Setting `programs.modde.profiles.test.tools.vkbasalt.settings.cas_sharpness
      = 0.4` (typo'd key) fails `nix flake check --impure` with a
      "unexpected option" Nix evaluation error.
- [ ] Setting `tools.vkbasalt.settings.casSharpness = "fast"`
      (wrong type) fails with a type-mismatch error.
- [ ] Tools' bool / int / enum settings round-trip through the
      activation script as expected — verified by the evaluator
      fixture from step 9.
- [ ] `CONTRIBUTING.md` documents that `nix/tool-schema.nix` is
      regenerated and not hand-written.

## Files likely touched

- New: `crates/modde-cli/src/commands/nix_schema.rs` (the exporter).
- `crates/modde-cli/src/main.rs` — register `dev export-tool-schema`.
- `crates/modde-cli/src/commands/mod.rs` — pub-mod the new file.
- New: `nix/tool-schema.nix` (generated, committed).
- `nix/hm-module.nix` — add typed `toolType.settings` branches for
  the small tools while preserving the free-form fallback for heavy
  tools, and update the activation helper to dispatch on typed values.
- `flake.nix` — add the staleness check and extend the evaluator
  fixture.
- `justfile` — `export-tool-schema` target.
- `CONTRIBUTING.md` — one-liner about regeneration.

## Pitfalls

- **`ToolSettingSpec.default` is not a value, it's not present.**
  The trait has `default_config()` returning a `ToolConfig`, but
  the *per-key* default is not exposed in the spec. The exporter
  must either read `default_config()` and project per-key
  defaults (preferred), or emit `default = null` everywhere and
  rely on the activation helper to skip null. Pick one and be
  consistent.
- **Enum/Select labels vs values.** `Select` options have `value`
  and `label` —
  [tools/mod.rs:203](../../crates/modde-games/src/tools/mod.rs#L203).
  The Nix `enum` type wants the *values* only; labels are
  user-facing strings the UI shows. Export `values`, drop
  `labels`. Document in the schema comment.
- **OptiScaler's `optiscaler_profile` is special.** It looks like
  a `Select` but Phase 04 wires it to a typed Nix enum derived
  from
  [`optiscaler_profiles`](../../crates/modde-games/src/tools/mod.rs#L45)
  per game. Strip it in the exporter and reserve the key for
  Phase 04.
- **Forgetting `_game_id` in the filter.** The CLI handlers inject
  `_game_id` —
  [tool.rs:482,561](../../crates/modde-cli/src/commands/tool.rs#L482).
  Any underscore-prefixed key (and explicitly `_game_id`) must be
  excluded from the export, or users see internal plumbing in
  their option reference.
- **Tri-state booleans.** mangohud's
  [`tri_state_bool`](../../crates/modde-games/src/tools/mod.rs#L260)
  maps to `nullOr bool` — `null` means "leave at upstream default",
  `false` means "force off". Don't map it to plain `bool`; that
  loses the third state and changes runtime behaviour.
- **Build-time codegen IFD.** If you tempt yourself with option (b),
  remember `hm-module.nix` is `import`ed at HM evaluation time. IFD
  is a real risk; static committed file is the boring right call
  here.
- **Schema drift in CI.** Without the staleness check, the file goes
  out of sync the first time someone adds a setting and doesn't
  re-run `just export-tool-schema`. The check is not optional.
- **Do not type the heavy tools in this phase.** `mangohud`,
  `optiscaler`, and `proton` stay free-form in the HM module even
  if the exporter can inventory their schema.

## Reference

- Phase 01: [./01-schema-and-activation-contract.md](./01-schema-and-activation-contract.md)
- Phase 02: [./02-tools-submodule-scaffold.md](./02-tools-submodule-scaffold.md)
- Tools trait + `ToolSettingSpec`:
  [crates/modde-games/src/tools/mod.rs](../../crates/modde-games/src/tools/mod.rs)
- Each tool's `settings_schema()`:
  [tools/mangohud.rs](../../crates/modde-games/src/tools/mangohud.rs),
  [tools/vkbasalt.rs](../../crates/modde-games/src/tools/vkbasalt.rs),
  [tools/gamemode.rs](../../crates/modde-games/src/tools/gamemode.rs),
  [tools/reshade.rs](../../crates/modde-games/src/tools/reshade.rs),
  [tools/optiscaler.rs](../../crates/modde-games/src/tools/optiscaler.rs),
  [tools/proton.rs](../../crates/modde-games/src/tools/proton.rs)
- Downstream: [04](./04-release-pinning-and-presets.md) adds the
  `release` option and the OptiScaler profile enum on top of this
  typing; [05](./05-docs-and-nix-check.md) wires docs + VM test.
