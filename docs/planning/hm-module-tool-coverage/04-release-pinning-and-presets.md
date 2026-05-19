# Phase 04 — Add `release` pinning and the OptiScaler per-game profile enum

> **Recommended Codex model: GPT 5.4-mini / medium**
>
> Two concrete additions on top of the typing locked in Phase 03:
> a `release = { tag, asset, hash, url }` option that fetches at
> Nix-eval time via `pkgs.fetchurl`, and a typed
> `tools.optiscaler.profile` enum keyed by game. Both are
> moderate sub-agent work with clear specs — Phase 03 already
> wrote the codegen machinery, Phase 04 only adds two new
> hand-written option shapes and a small CLI integration to
> consume the eager-fetched archive. `5.4-mini/medium` matches a
> moderate sub-agent task. Bump to `5.4/medium` only if the
> "eager-fetch vs lazy-fetch" call in Phase 01's DECISION.md was
> *lazy* — in which case the activation-side fetching is more
> involved.

## Working tree

`/data/nvme0/can/Projects/rs-modde` (this repo). Depends on
[02](./02-tools-submodule-scaffold.md) for the `toolType` shape
and [03](./03-typed-schema-codegen.md) for the typed `settings`
attrs. Touches the same file
([nix/hm-module.nix](../../nix/hm-module.nix)) — must rebase on
03's commit before starting.

## Goal

Two surfaces grow on `profiles.<name>.tools.<id>`:

1. **`release = { tag, asset, hash, url }`** (or `null`) for
   tools where
   [`supports_releases()`](../../crates/modde-games/src/tools/mod.rs#L559)
   is `true`. In this checkout only `optiscaler` reports release
   support through the `GameTool` contract; this phase must either
   wire `proton` into that contract or keep `tools.proton.release`
   rejected until that Rust support exists. The Nix
   module fetches the asset eagerly via `pkgs.fetchurl` (or
   `requireFile` if the user prefers manual provisioning), hands
   the local path to `modde tool install-release-from-path`, and
   the CLI takes it from there. No network at activation time.
2. **`tools.optiscaler.profile = "stellar-blade/fsr3"`** as a
   typed `nullOr (enum [...])` whose values are computed from
   the registered
   [`optiscaler_profiles`](../../crates/modde-games/src/tools/mod.rs#L45)
   of `profile.game`. Setting it injects the matching
   `optiscaler_profile` config key (mirroring the CLI behaviour
   at
   [tool.rs:551-553](../../crates/modde-cli/src/commands/tool.rs#L551-L553)).

Once both surfaces land, a Nix configuration can declaratively
pin OptiScaler to a specific GitHub release, select the per-game
profile, and let activation produce a reproducible apply — with
no network calls and no manual `modde tool install-release` step.

## Why this matters now

- **Pinning matters for reproducibility.** Without it, the
  GitHub releases endpoint at
  [optiscaler.rs:723](../../crates/modde-games/src/tools/optiscaler.rs#L723)
  is queried every activation, producing
  unreproducible installs and breaking offline / pure-Nix
  environments. The CLI's `install_release` does the fetch and
  the install in one motion — fine for interactive use, wrong
  for Nix-managed setups.
- **OptiScaler profiles are per-game.** Stellar Blade has its
  own profile list at
  [unreal/](../../crates/modde-games/src/unreal/), distinct from
  Subnautica 2's. Phase 03 deliberately excluded
  `optiscaler_profile` from the generic schema because it's the
  one key whose valid-value set depends on which game the profile
  configures. Without typed support, users have to hand-copy
  profile IDs from Rust source.
- **`requireFile` for manual archives.** Some users will want to
  hand-place the `.7z` (e.g. when the GitHub mirror is down or
  the user is on a constrained network). `requireFile` is the
  HM-friendly escape hatch; the option must support both URL +
  hash and a local `path`.

## Out of scope

- Adding release support to tools that don't have it (e.g.
  ReShade — that's a Rust-side change in
  [reshade.rs](../../crates/modde-games/src/tools/reshade.rs)).
- Tracking GitHub release listings from Nix. The user supplies
  `{ tag, asset, hash }` (or `{ tag, asset, path }`); the
  module does not auto-discover.
- New OptiScaler profiles. Profile additions land in the Rust
  registry first.
- Docs site updates — Phase 05.
- A NixOS VM test — Phase 05.

## Plan

1. **Add a CLI install-from-path subcommand.** New variant in
   [`ToolAction`](../../crates/modde-cli/src/main.rs#L772) (after
   `InstallRelease`):
   ```rust
   /// Install a release asset from a local path (Nix-friendly)
   InstallReleaseFromPath {
       tool_id: String,
       #[arg(long)] game: String,
       #[arg(long)] tag: String,
       #[arg(long)] asset: String,
       /// Local path to the already-downloaded asset
       path: PathBuf,
   },
   ```
   And the handler in
   [crates/modde-cli/src/commands/tool.rs](../../crates/modde-cli/src/commands/tool.rs):
   `handle_install_release_from_path` mirrors
   `handle_install_release` but does not call the network-fetching
   path of
   [`install_release`](../../crates/modde-games/src/tools/mod.rs#L576-L589).
   It copies the archive into the modde data dir under the same
   destination the regular installer uses, then re-uses the
   extract-and-record code path.
2. **Surface an extract-from-path entry point on the trait.** Add
   a default trait method `install_release_from_path` to
   [`GameTool`](../../crates/modde-games/src/tools/mod.rs#L436)
   with the same signature as
   `install_release` but taking a `PathBuf`. Override it in
   [optiscaler.rs](../../crates/modde-games/src/tools/optiscaler.rs)
   and
   [proton.rs](../../crates/modde-games/src/tools/proton.rs) only if
   this phase also adds Proton to the release-backed `GameTool`
   surface (`supports_releases`, release listing, and install handler
   backed by the existing GE-Proton helpers). Default impl bails with
   "this tool does not support release pinning".

2a. **Fix Proton's `supports_releases()` mismatch (DECISION.md
    deferred item).** Today
    [`Proton`](../../crates/modde-games/src/tools/proton.rs)
    has GE-Proton install helpers
    (`protonup_rs_install_args` at
    [tools/proton.rs](../../crates/modde-games/src/tools/proton.rs),
    `is_ge_proton_version`, `merge_proton_version_options`) but
    does **not** override
    [`supports_releases()`](../../crates/modde-games/src/tools/mod.rs#L559),
    so the trait default returns `false`. As part of this phase
    (per DECISION.md "Deferred"), override it to return `true`
    and implement `list_releases` + `install_release_from_path`
    using the existing GE-Proton helpers. Without this, the
    `release` option on `tools.proton` would render an
    "unsupported" assertion error in step 3's guard.
3. **Add the `release` option to `toolType` in
   [nix/hm-module.nix](../../nix/hm-module.nix).** Submodule:
   ```nix
   release = lib.mkOption {
     type = lib.types.nullOr (lib.types.submodule {
       options = {
         tag = lib.mkOption { type = lib.types.str; };
         asset = lib.mkOption { type = lib.types.str; };
         url = lib.mkOption { type = lib.types.nullOr lib.types.str; default = null; };
         hash = lib.mkOption { type = lib.types.nullOr lib.types.str; default = null; };
         path = lib.mkOption { type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str); default = null; };
       };
     });
     default = null;
     description = "Pinned release asset (mutually exclusive: { url, hash } OR path).";
   };
   ```
   Add a guard assertion: `release.url` + `release.hash` set
   together xor `release.path` set; `release` set on a non-
   release-supporting tool fails with a message naming the
   tool. The "tools that support releases" list is generated from
   the Rust trait surface; it starts as `["optiscaler"]` in the
   current checkout and includes `proton` only after this phase wires
   Proton into `GameTool::supports_releases`.
4. **Resolve the asset path inside the module.** When `release !=
   null`, compute:
   ```nix
   assetSrc =
     if profile.tools.<id>.release.path != null
     then profile.tools.<id>.release.path
     else pkgs.fetchurl {
       inherit (profile.tools.<id>.release) url hash;
       name = profile.tools.<id>.release.asset;
     };
   ```
   Render an activation snippet that calls `modde tool
   install-release-from-path <tool> --game <game> --tag <tag>
   --asset <asset> <assetSrc>` *before* the `modde tool enable`
   call (so the install is in place when enable runs).
5. **Add the OptiScaler `profile` option.** Per DECISION.md,
   OptiScaler keeps free-form `settings = attrsOf anything` in
   Phase 03 — the typed `profile` enum therefore lives at the
   **per-tool submodule level** (sibling of `enable`, `settings`,
   `release`, `applyOnActivation`), not nested inside `settings`.
   In `toolType` (or in a tool-id-conditional submodule branch),
   add for OptiScaler only:
   ```nix
   profile = lib.mkOption {
     type = lib.types.nullOr (lib.types.enum profilesForGame);
     default = null;
     description = "OptiScaler per-game profile preset.";
   };
   ```
   `profilesForGame` is computed by reading the new
   `nix/optiscaler-profiles.nix` file (generated alongside
   `tool-schema.nix` from Phase 03) keyed by `profile.game`. If
   the game has no profiles, the option's type becomes
   `nullOr (enum [])`, which Nix evaluates to "must be null"
   (acceptable default).
6. **Generate `nix/optiscaler-profiles.nix`.** Extend the
   exporter from
   [Phase 03 step 3](./03-typed-schema-codegen.md) to also emit a
   `nix/optiscaler-profiles.nix` file:
   ```nix
   {
     "stellar-blade" = [ "fsr3-frame-gen" "dlss" ... ];
     "subnautica2" = [ ... ];
     ... # game_id → profile_id list, sourced from
     ... # GameRegistration.optiscaler_profiles
   }
   ```
   The exporter sources this by iterating
   [`GAME_REGISTRY`](../../crates/modde-games/src/registry.rs#L140)
   and collecting `.optiscaler_profiles[].id`. Wire it through
   the same `just export-tool-schema` target.
7. **Wire the activation helper for `profile`.** When
   `tools.optiscaler.profile != null`, render
   `modde tool configure optiscaler --game <game> --
   optiscaler_profile=<profile>` immediately before the regular
   configure call (or fold both into one configure invocation —
   the CLI handler at
   [tool.rs:514-573](../../crates/modde-cli/src/commands/tool.rs#L514-L573)
   applies them in order, and
   [tool.rs:551-553](../../crates/modde-cli/src/commands/tool.rs#L551-L553)
   already applies the profile-derived defaults when the key is
   set).
8. **Add a `release_supporting_tools.nix` source-of-truth check.**
   Extend the `nix_schema.rs` exporter to also emit
   `nix/release-supporting-tools.nix` (a list of strings). The
   HM module imports it and uses it for the assertion in step 3
   instead of hardcoding the list — keeps Rust as the source of
   truth.
9. **Extend evaluator-check fixtures.** Add fixtures that:
   - Set `tools.optiscaler.release = { tag = "v1.0"; asset =
     "OptiScaler.7z"; url = "https://example.test/o.7z"; hash =
     "sha256-..."; };` — must evaluate, must render an
     `install-release-from-path` invocation with the asset's
     store path.
   - Set `tools.mangohud.release = { ... };` — must fail with a
     message naming "mangohud does not support release pinning".
   - Set `tools.optiscaler.profile = "nonexistent"` on a
     Stellar Blade profile — must fail with an enum-type error.
   - Set `tools.optiscaler.profile = "stellar-blade/<valid>"`
     (whatever the real ID is from the generated file) on a
     Stellar Blade profile — must render
     `optiscaler_profile=<id>` in the configure invocation.
10. **Smoke-run.** `nix flake check --impure`; then `nix eval
    --impure .#homeManagerModules.modde` to confirm the module
    still imports cleanly. No real fetch happens during
    `nix eval` (fetchurl is in
    `config.home.activation` rendering, not evaluated until the
    HM build actually consumes it).

## Acceptance criteria

- [ ] `modde dev export-tool-schema` regenerates
      `nix/tool-schema.nix`, `nix/optiscaler-profiles.nix`, and
      `nix/release-supporting-tools.nix`. All three are
      committed.
- [ ] Setting `tools.optiscaler.release = { ... }` produces an
      activation snippet that invokes `modde tool
      install-release-from-path optiscaler --game <game> --tag
      <tag> --asset <asset> /nix/store/...-<asset>` before the
      `modde tool enable` call.
- [ ] Setting `tools.proton.release = { ... }` works the same
      way only if this phase wires Proton into the release-backed
      `GameTool` contract; otherwise it fails with the same
      unsupported-release assertion as non-release tools.
- [ ] Setting `tools.mangohud.release = { ... }` fails `nix
      flake check` with a message naming
      "mangohud does not support release pinning".
- [ ] Setting `release.url` and `release.path` simultaneously
      fails with a "mutually exclusive" assertion message.
- [ ] Setting `tools.optiscaler.profile = "<unknown>"` on a
      profile whose game is Stellar Blade fails with an
      enum-type error and the valid values are listed in the
      error message.
- [ ] Setting `tools.optiscaler.profile = "<valid>"` produces a
      configure call containing `optiscaler_profile=<valid>` in
      the activation script.
- [ ] All existing checks from Phases 02–03 still pass.
- [ ] `cargo test -p modde-cli` and `cargo test -p modde-games`
      pass (the new
      `install_release_from_path` trait method has at least one
      test per implementor).

## Files likely touched

- `crates/modde-cli/src/main.rs` — register the new
  `InstallReleaseFromPath` variant.
- `crates/modde-cli/src/commands/tool.rs` — add
  `handle_install_release_from_path`.
- `crates/modde-cli/src/commands/nix_schema.rs` — extend exporter
  to emit `optiscaler-profiles.nix` and
  `release-supporting-tools.nix`.
- `crates/modde-games/src/tools/mod.rs` — add
  `install_release_from_path` trait method with a default impl.
- `crates/modde-games/src/tools/optiscaler.rs` and, if Proton is
  promoted to the release-backed trait surface in this phase,
  `crates/modde-games/src/tools/proton.rs` — implement
  `install_release_from_path`.
- New: `nix/optiscaler-profiles.nix`,
  `nix/release-supporting-tools.nix` (generated, committed).
- `nix/hm-module.nix` — add the `release` option to `toolType`,
  the `profile` option to the OptiScaler typed branch, the
  assertion that release pinning is only allowed on tools where
  `releaseSupportingToolIds` says so, and the activation
  rendering for both.
- `flake.nix` — extend the evaluator check from Phase 03 with the
  fixtures in step 9.

## Pitfalls

- **`fetchurl` and IFD.** `pkgs.fetchurl` is fine to call from a
  module — the derivation is built when HM activates, not when
  the flake evaluates. But if you nest `fetchurl` inside a
  `pkgs.runCommand` that's read via `builtins.readFile`, you've
  introduced IFD. Keep the fetched store path inline in the
  activation rendering only.
- **OptiScaler's `.7z` extraction needs `p7zip` on PATH.** The
  existing
  [optiscaler.rs install path](../../crates/modde-games/src/tools/optiscaler.rs)
  shells out to `7z` —
  verify the `modde` derivation in
  [flake.nix](../../flake.nix) already provides `p7zip` as a
  runtime input. If not, that's a separate package-input fix that
  should land in this phase.
- **Proton is not release-backed through `GameTool` yet.** The local
  code has GE-Proton catalogue/install helpers, but no
  `supports_releases()` override. Do not add `tools.proton.release`
  to the HM allow-list until the Rust trait implementation exists.
- **Proton's release archive is multi-GB.** A naïve fetchurl
  blocks the HM build for minutes. Don't add a flake check that
  actually fetches — use a small fake archive in the evaluator
  fixtures and only verify the rendering, not the install.
- **Profile validity is per-game.** A profile valid for Stellar
  Blade is not valid for Subnautica 2. The `enum` type must be
  computed from `profile.game` at module evaluation time. Don't
  cache `profilesForGame` at the top of the module — it's a
  function of the profile's `game`, not a module-level constant.
- **`tools.<id>.profile` only exists when `<id>` is OptiScaler.**
  Other tools' typed `settings` submodule must not have a
  `profile` key. The codegen in step 6 must emit it conditionally
  inside the OptiScaler branch only.
- **Empty `optiscaler_profiles` for a game.** Games with no
  profiles (most of them today) get
  `optiscaler_profiles: []` in the registry. `lib.types.enum []`
  evaluates a non-null value to "no valid options" — that
  produces a noisy Nix error if the user tries to set it. Default
  to `null` and validate in an assertion that "no profiles
  registered for game X" is the error, not the enum-empty one.
- **Stale `optiscaler_profile` key in `tool-schema.nix`.** Phase
  03 reserved this key for Phase 04. Verify the exporter still
  strips it from the *generic* schema (it now lives as a typed
  Nix option, not a generic settings key), or users get two
  options for the same thing.

## Reference

- Phase 01: [./01-schema-and-activation-contract.md](./01-schema-and-activation-contract.md)
- Phase 02: [./02-tools-submodule-scaffold.md](./02-tools-submodule-scaffold.md)
- Phase 03: [./03-typed-schema-codegen.md](./03-typed-schema-codegen.md)
- Game registry + OptiScaler profile field:
  [crates/modde-games/src/registry.rs](../../crates/modde-games/src/registry.rs)
- OptiScaler release install path:
  [crates/modde-games/src/tools/optiscaler.rs](../../crates/modde-games/src/tools/optiscaler.rs)
- Proton release install path:
  [crates/modde-games/src/tools/proton.rs](../../crates/modde-games/src/tools/proton.rs)
- Existing CLI install-release flow:
  [crates/modde-cli/src/commands/tool.rs:737-763](../../crates/modde-cli/src/commands/tool.rs#L737-L763)
- Downstream: [05](./05-docs-and-nix-check.md) writes the docs +
  VM test that exercises this surface end-to-end.
