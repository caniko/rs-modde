# MO2 Coverage Truth Audit

This document is intentionally conservative.

- `Done` means the feature is shipped end to end and reachable by users.
- `Partial` means core logic exists, but the UX, integration, or production safety is still incomplete.
- `Not shipped` means the repository should not market it as available yet.
- The canonical status baseline for this audit lives in `docs/capability-matrix.toml`.

## Current baseline

| Area                                     | Status        | Notes                                                                                                                                                                  |
| ---------------------------------------- | ------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Core VFS deployment                      | `Done`        | Symlink-farm deployment, per-file hiding, rollback, and conflict resolution are shipped.                                                                               |
| Profile switching and save vaults        | `Done`        | Profile activation, experiment stack, git-backed save history, and save fingerprints are shipped for games with real save trackers.                                    |
| Instance switching                       | `Partial`     | CLI/runtime instance selection now changes the active data root, but there is no MO2-style portable/global UX yet.                                                     |
| Bethesda plugin management               | `Done`        | Real plugin order backup/restore, `plugins.txt` IO, LOOT parsing, Form 43 detection, and missing-master checks are shipped.                                            |
| Diagnostics                              | `Partial`     | CLI and UI now share a real diagnostics engine with conflict/plugin inputs, but there is still no MO2-style problems button or guided fixes.                           |
| Data tab                                 | `Partial`     | The UI now renders real conflict rows from the resolver/collision engine, but it is not yet a full merged-VFS browser with archive/hidden filters.                     |
| Downloads UI                             | `Partial`     | The Downloads view is connected to real queue state instead of placeholder text, but pause/resume is still UI-side state, not a durable network pipeline.              |
| Tool management                          | `Partial`     | The Tools view now loads DB-backed tool configs and can apply/revert tracked file patches, but executable management and per-executable output mods are still missing. |
| Nexus install pipeline                   | `Done`        | API key auth, browse/search, update checks, `nxm://`, and single-mod Nexus installs are shipped.                                                                       |
| Non-Nexus download backends              | `Partial`     | GitHub, Direct, Google Drive, and MEGA backends exist, but they are mainly exercised by Wabbajack/directive workflows rather than first-class `install mod` UX.        |
| FOMOD                                    | `Done`        | Wizard flow plus declarative config generation/application are shipped.                                                                                                |
| BAIN                                     | `Partial`     | Detection and execution scaffolding exist, but the required user-selection flow is not finished.                                                                       |
| Generic game support                     | `Not shipped` | `GenericGame` exists as an internal helper, not as a polished user-facing “support any game” feature.                                                                  |
| Starfield save tracking                  | `Done`        | Starfield `.sfs` files are tracked through the shared save tracker path.                                                                                               |
| Executables management / mod info dialog | `Not shipped` | This remains the biggest MO2 parity gap.                                                                                                                               |

## Supported games

| Game           | Status    | Notes                                                                                                                                |
| -------------- | --------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Skyrim SE / AE | `Done`    | Plugins, LOOT, diagnostics, VFS, saves, and Wabbajack/Nexus workflows are the strongest path today.                                  |
| Fallout 4      | `Done`    | Plugins, diagnostics, VFS, and save tracking are shipped.                                                                            |
| Fallout 76     | `Partial` | VFS, plugin handling, and BA2 scanning exist; saves are effectively server-side and only lightly represented locally.                |
| Starfield      | `Partial` | Game plugin, plugin handling, VFS, diagnostics, and `.sfs` save tracking exist.                                                      |
| Cyberpunk 2077 | `Done`    | REDmod/CET/TweakXL-aware install and launch flows are shipped.                                                                       |
| Stellar Blade  | `Partial` | UE4/UE5-style deployment, scanning, conflicts, and local save tracking are present, but depth is below the Bethesda/Cyberpunk paths. |

## What changed in this audit pass

- Instance switching now changes the active modde data root instead of only updating a registry entry.
- Plugin order backups now snapshot and restore the real plugin order, including enabled state, and write back to both DB state and native `plugins.txt`.
- Diagnostics no longer run against an empty conflict map or guessed plugin names. The CLI and UI now consume the same analyzed profile state.
- The Downloads, Data Files, Diagnostics, and Tools views are reachable from the sidebar and render live state instead of dead placeholder branches.

## Highest-leverage MO2 parity work next

1. Executables management: named executables, arguments, working directory, per-executable output mod, shortcut generation, and Linux/Wine DLL injection semantics.
2. Mod information dialog: file tree, text/INI editing, image preview, conflict tabs, optional plugin management.
3. Real download pipeline durability: concurrency, pause/resume/cancel at the transport layer, ETA/speed, update-all, and resumable metadata sidecars.
4. Full merged-VFS browser: archive visibility, overwrite actions, hidden-file filters, and “go to conflicting mod” workflows.
5. Clear Linux-first product story: double down on reliable Wabbajack/Collections/Nexus workflows for the games already advertised before chasing broader title count.
