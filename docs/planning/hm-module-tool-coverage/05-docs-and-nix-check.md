# Phase 05 — Document the new options and add a flake VM check

> **Recommended Codex model: GPT 5.4-mini / medium**
>
> Final pass: prose docs for the option reference, an end-to-end
> sample `programs.modde.profiles.<name>.tools.{...}` example, and
> a `nixosTest`-style VM check that asserts the activation script
> renders the expected `modde tool` invocations against a real
> HM build. Mostly mechanical; one design call (use `nixosTest`
> vs a pure-eval check) is bounded enough for `5.4-mini/medium`.
> Bump to `5.4/medium` if the VM check turns out to need a real
> modde binary executed against a stub DB — that's harder than
> rendering-only assertions.

## Working tree

`/data/nvme0/can/Projects/rs-modde` (this repo). Depends on
Phases 02, 03, and 04 — the surface being documented is the one
those phases produce. Touches docs + flake.nix; no further
changes to `nix/hm-module.nix`.

## Goal

Two artefacts:

1. **Updated option reference**: `docs/site/content/docs/configuration/hm-module.md`
   gains a "Per-profile tools" section that documents every
   surface added in 02–04: `tools.<id>.enable`,
   `tools.<id>.settings.<key>` (typed for the small tools,
   free-form for heavy tools), `tools.<id>.release`,
   `tools.<id>.applyOnActivation`, `tools.optiscaler.profile`.
   A worked example shows Skyrim SE with vkBasalt + GameMode and
   OptiScaler (release-pinned, profile selected).
2. **End-to-end Nix evaluation check**: a flake check that
   evaluates a fixture covering all four shapes (typed setting,
   release, profile, applyOnActivation) and asserts the activation
   script body contains every expected `modde tool ...`
   invocation. No real modde binary runs — this is a rendering
   assertion only.

After this phase, a user with no Rust knowledge can read the
option reference, copy the worked example, and end up with a
declarative per-game tool stack.

## Why this matters now

Phases 02–04 grow the module's surface area significantly. Without
docs, every user has to either:

- read the Nix source (low signal-to-noise) or
- copy from this repo's own activation tests (which is what people
  will do anyway, but it leaves "what *can* I configure" implicit).

The CLI's `modde tool` surface is also under-documented today; the
HM module reference becomes the de-facto reference for tool
options, since the option names match the JSON config keys
one-for-one (post-Phase 03).

For the VM check: Phases 02–04 each ship per-fixture evaluator
checks, but no test asserts the *complete* flow renders correctly
end-to-end. A single fixture that exercises typed setting +
release + profile + applyOnActivation in one profile catches
regressions where 03/04 fix their own slice but break composition.

## Out of scope

- Adding a real `nixosTest` VM that boots NixOS, runs HM
  activation, and asserts modde DB rows. That's a heavier
  investment (needs a Steam-like game-dir fixture, modde
  binary, etc.) and belongs in a separate phase if it's worth
  doing at all. This phase's "VM check" is a rendering assertion
  expressed in the flake.
- Documenting tools' internal behaviour (e.g. how MangoHud's
  `fps_limit_method` interacts with V-Sync). The option
  reference describes Nix surface, not gameplay outcomes — link
  to the tools' upstream docs.
- New CHANGELOG entries — they're written when the commits
  land, not from this phase doc.
- A migration guide for users currently using
  `home.activation` hand-rolled tool calls. One-paragraph "if
  you have hand-rolled `modde tool` calls in `home.activation`,
  delete them and use the new options" note suffices.

## Plan

1. **Add the "Per-profile tools" section to the option
   reference.** Edit
   [docs/site/content/docs/configuration/hm-module.md](../../docs/site/content/docs/configuration/hm-module.md)
   to add a `#### profiles.<name>.tools.<id>` subsection. Document
   each option's type and default in the same table-and-bullet
   style as the existing sections at lines 27–84.
2. **Tabulate the supported tool IDs** with a one-line description
   each:
   - `mangohud` — performance HUD overlay
   - `vkbasalt` — Vulkan post-processing
   - `gamemode` — system performance tuning
   - `reshade` — D3D/OpenGL post-processing (Wine-backed)
   - `optiscaler` — DLSS/FSR/XeSS upscaling
   - `proton` — Proton runtime selection + DLL overrides
3. **List which tools support each option.** A small matrix:

   | Option | mangohud | vkbasalt | gamemode | reshade | optiscaler | proton |
   |---|---|---|---|---|---|---|
   | `enable` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
   | `settings.<key>` | free-form | typed | typed | typed | free-form | free-form |
   | `applyOnActivation` | n/a | n/a | n/a | ✓ | ✓ | n/a |
   | `release` | n/a | n/a | n/a | n/a | ✓ | generated |
   | `profile` | n/a | n/a | n/a | n/a | ✓ | n/a |

   Cross-reference each generated option type back to its source
   (`crates/modde-games/src/tools/<tool>.rs` → `settings_schema()`).
4. **Worked example.** Add to the "Example configuration" block
   (currently at
   [hm-module.md:93-121](../../docs/site/content/docs/configuration/hm-module.md#L93-L121))
   a second `profiles.living-skyrim.tools = {...}` block that:
   - Enables `vkbasalt` with `enableOnLaunch = true`,
     `casSharpness = 0.4`.
   - Enables `gamemode`.
   - Enables `optiscaler` with a pinned release and a
     `profile`, plus `applyOnActivation = true`.
   - Note the trade-off: `applyOnActivation = true` writes to
     the game dir on every HM switch.
5. **Sample escape-hatch.** Show how to provide an archive via
   `requireFile` instead of `url`+`hash`:
   ```nix
   tools.optiscaler.release = {
     tag = "v1.0";
     asset = "OptiScaler.7z";
     path = pkgs.requireFile {
       name = "OptiScaler.7z";
       sha256 = "...";
       url = "https://github.com/...";
     };
   };
   ```
6. **Add an end-to-end evaluator check in
   [flake.nix](../../flake.nix).** Sits alongside the existing
   `hm-module` check at line 521. Fixture: one profile with
   game `skyrim-se`, vkBasalt enabled with two typed settings,
   gamemode enabled, plus a *separate* profile with game
   `stellar-blade` that enables optiscaler with a release block
   and a profile enum value (use a real OptiScaler profile ID
   from `nix/optiscaler-profiles.nix`).

   The check evaluates the module, extracts
   `config.home.activation.modde-deploy`, and `grep`s for the
   expected substrings:
   - `modde tool enable vkbasalt --game skyrim-se`
   - `modde tool configure vkbasalt --game skyrim-se -- enableOnLaunch=true casSharpness=0.4`
     (or two invocations if Phase 02 picked one-per-key — match
     however 02 rendered)
   - `modde tool enable gamemode --game skyrim-se`
   - `modde tool install-release-from-path optiscaler --game stellar-blade --tag <tag> --asset <asset> /nix/store/...`
   - `modde tool configure optiscaler --game stellar-blade -- optiscaler_profile=<profile-id>`
   - `modde tool apply optiscaler --game stellar-blade`
7. **Add a negative-path check.** Same fixture pattern, but each
   variant injects one error and asserts the flake check fails
   with a specific message substring:
   - Unknown tool ID → "unknown tool"
   - Unknown setting key → "unexpected option"
   - Wrong-type setting → "type mismatch"
   - `release` on a non-release-supporting tool → "does not support release pinning"
   - Invalid `optiscaler.profile` → enum-mismatch error
8. **Update `TODO.md`.** Move the "HM module tool coverage" entry
   from "in progress" to "shipped" (or whatever convention the
   file uses) and link to the merged plan dir.
9. **Build the docs site to verify rendering.** From the repo:
   ```bash
   cd docs/site
   zola build  # or whatever the existing build target is
   ```
   The new section renders without broken links; the matrix
   table aligns.
10. **Smoke-run flake checks.** `nix flake check --impure` —
    every check from 02 through 05 passes; the new
    negative-path checks fail when run individually with the
    expected error messages (verify with `nix flake check
    --impure 2>&1 | grep <substring>` for each).

## Acceptance criteria

- [ ] `docs/site/content/docs/configuration/hm-module.md` has a
      new "Per-profile tools" section covering every option added
      in Phases 02–04. Section is reachable from the docs site
      navigation.
- [ ] The matrix table maps each option to each tool with ✓/n/a
      consistently.
- [ ] The worked example evaluates with `nix eval --impure` and
      `nix flake check --impure`.
- [ ] One positive-path flake check renders the expected
      activation invocations from a multi-profile fixture.
- [ ] Five negative-path flake checks each fail with a
      specific, recognisable error message substring
      corresponding to one mis-configured fixture.
- [ ] `zola build` (or the equivalent site build) succeeds with
      no broken-link warnings touching `hm-module.md`.
- [ ] `TODO.md` has the HM module tool coverage item closed out
      and references the plan directory.
- [ ] Commit message links to the plan directory and lists the
      five surfaces documented (`enable`, `settings.<key>`,
      `applyOnActivation`, `release`, `profile`).

## Files likely touched

- [docs/site/content/docs/configuration/hm-module.md](../../docs/site/content/docs/configuration/hm-module.md)
  — new "Per-profile tools" section, extended example.
- [flake.nix](../../flake.nix) — positive-path and five
  negative-path checks; sits alongside the existing
  `hm-module` check at line 521.
- `TODO.md` — close-out entry.

## Pitfalls

- **Docs section ordering matters.** Place the new section
  *after* `nexus.apiKeyFile` and *before* the example block so
  the example actually demonstrates the section above it. The
  current file structure at
  [hm-module.md:86](../../docs/site/content/docs/configuration/hm-module.md#L86)
  has the secret-config section near the end — the tools section
  should sit between profile options and the example.
- **Matrix drift.** If a new tool grows release support post-
  Phase 04, the matrix in this doc goes stale. Note in a
  trailing "Maintainer note" line that the matrix is sourced
  from
  `nix/release-supporting-tools.nix` —
  if that file changes, this matrix needs an update.
- **Worked example bloat.** Keep the example to one profile per
  game, two tools max per profile. A maximalist example overwhelms
  new users and obscures the option shape.
- **Negative-path checks that mask bugs.** A `grep`-substring
  assertion that passes for the wrong reason (e.g. the error
  message changed but the substring still matches a different
  error) is worse than no check. Use specific phrases tied to
  the assertion messages emitted by 02–04, not generic Nix error
  text.
- **`requireFile` in checks.** Don't include `requireFile` in
  the flake-check fixture — it requires interactive user input
  on miss. Use `pkgs.writeText` to fake a `.7z` path or a fixed
  `/dev/null`-equivalent.
- **Asset path in store.** The positive-path assertion's
  `/nix/store/...-<asset>` substring should match a stable
  prefix (the trailing hash varies). Match on the asset
  filename suffix only.

## Reference

- Phase 01: [./01-schema-and-activation-contract.md](./01-schema-and-activation-contract.md)
- Phase 02: [./02-tools-submodule-scaffold.md](./02-tools-submodule-scaffold.md)
- Phase 03: [./03-typed-schema-codegen.md](./03-typed-schema-codegen.md)
- Phase 04: [./04-release-pinning-and-presets.md](./04-release-pinning-and-presets.md)
- Existing HM module docs:
  [docs/site/content/docs/configuration/hm-module.md](../../docs/site/content/docs/configuration/hm-module.md)
- Existing flake check:
  [flake.nix:521](../../flake.nix#L521)
