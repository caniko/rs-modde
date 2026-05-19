# HM Module Tool Coverage Decision

## Option Shape

One-line answer: `programs.modde.profiles.<name>.tools` is an `attrsOf
toolSubmodule` with `enable`, free-form `settings`, reserved `release`,
and `applyOnActivation`; Phase 03 only types the small tools.

Rationale: `ToolConfig.settings` remains a JSON blob in SQLite, so the
contract is type-safe evaluation, not type-safe storage. Phase 02 ships
`settings = attrsOf anything` for every tool and filters reserved keys
such as CLI-injected `_game_id`. Phase 03 may type `gamemode`,
`vkbasalt`, and `reshade`; `mangohud`, `optiscaler`, and `proton` stay
free-form.

| Tool | Settings count | Kinds | Release-backed now | Phase 03 typed? |
| --- | ---: | --- | --- | --- |
| `mangohud` | 114 | Bool, Select, Number, Path, Text | No | No |
| `vkbasalt` | 6 | Bool, Number, Path, Text | No | Yes |
| `gamemode` | 1 | ReadOnly | No | Yes |
| `reshade` | 3 | Path, Select, ReadOnly | No | Yes |
| `optiscaler` | 20 baseline, plus contextual/dynamic fields | Bool, Select, Number, Path, Text, TriStateBool, ReadOnly | Yes | No |
| `proton` | 33 | Bool, Select, Path, Text, ReadOnly | Not through `modde tool install-release` in this checkout | No |

## Activation Contract

One-line answer: tool activation runs after the existing `modde install`
/ `modde deploy` flow, and every `modde tool` non-zero exit warns and
continues.

Rationale: install/deploy remains the prerequisite surface; tool writes
land last. For each enabled tool, HM always calls idempotent `modde tool
enable`, then `modde tool configure` when settings are non-empty. `modde
tool apply` is guarded by `applyOnActivation = true`; it is repeatable
but mutates the game directory and rewrites the applied-files manifest.
Disabled tools call `modde tool disable` and skip configure/apply. Nix
assertion failures remain fatal before activation.

## Release Fetch

One-line answer: HM-managed releases are eager Nix fetches with pinned
hashes; activation must not call networked `modde tool install-release`.

Rationale: release options use `{ url, hash, tag, asset }`; Nix fetches
the asset and activation hands modde a local path or extracted source.
Fetch/hash failures are fatal before switch-time. Phase 04 must add the
missing local-path handoff where the CLI only supports remote tag/asset.

## Deferred

Phase 02 implements the free-form scaffold. Phase 03 types small tools.
Phase 04 adds release pinning, OptiScaler presets, and resolves Proton's
mismatch: GE-Proton helpers exist, but Proton does not expose
`GameTool::supports_releases`. Phase 05 documents the contract.
