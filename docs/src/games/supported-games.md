# Supported Games

This table is intentionally conservative:

- `Done` means the game path is shipped end to end.
- `Partial` means some core pieces exist, but major workflows are still missing or not yet trustworthy.
- `Not shipped` means the capability should not be treated as available.
- The canonical status baseline for this page lives in `docs/capability-matrix.toml` in the repository.

## Bethesda titles

| Game | ID | Overall status | Scanner | Conflict detection | Save tracking |
| ---- | -- | -------------- | ------- | ------------------ | ------------- |
| Skyrim Special Edition | `skyrim-se` | `Done` | Yes | Yes | `Done` |
| Skyrim Anniversary Edition | `skyrim-ae` | `Done` | Yes | Yes | `Done` |
| Fallout 4 | `fallout4` | `Done` | Yes | Yes | `Done` |
| Fallout 76 | `fallout76` | `Partial` | Yes | Yes | `Partial` (server-side / local cache only) |
| Starfield | `starfield` | `Partial` | Yes | Yes | `Done` (capture only; no contamination gate) |

## Other games

| Game                                      | ID                    | Overall status | Scanner | Conflict detection | Save tracking |
| ----------------------------------------- | --------------------- | -------------- | ------- | ------------------ | ------------- |
| Cyberpunk 2077                            | `cyberpunk2077`       | `Done`         | Yes     | Yes                | `Done`        |
| Stellar Blade                             | `stellar-blade`       | `Partial`      | Yes     | Yes                | `Done`        |
| Baldur's Gate 3                           | `baldurs-gate3`       | `Partial`      | Yes     | Yes                | `Done`        |
| Stardew Valley                            | `stardew-valley`      | `Partial`      | Yes     | Yes                | `Done`        |
| Fallout: New Vegas                        | `fallout-new-vegas`   | `Partial`      | Yes     | Yes                | `Done`        |
| The Elder Scrolls IV: Oblivion            | `oblivion`            | `Partial`      | Yes     | Yes                | `Done`        |
| The Elder Scrolls IV: Oblivion Remastered | `oblivion-remastered` | `Partial`      | Yes     | Yes                | `Done`        |
| Mount & Blade II: Bannerlord              | `bannerlord`          | `Partial`      | Yes     | Yes                | `Done`        |
| The Witcher 3: Wild Hunt                  | `witcher3`            | `Partial`      | Yes     | Yes                | `Done`        |
| Subnautica 2                              | `subnautica2`         | `Partial`      | Yes     | Yes                | `Done`        |

## Wabbajack game mapping

When installing Wabbajack modlists, the manifest game names are mapped to modde game IDs:

| Wabbajack name         | modde ID              |
| ---------------------- | --------------------- |
| `SkyrimSpecialEdition` | `skyrim-se`           |
| `Fallout4`             | `fallout4`            |
| `Fallout76`            | `fallout76`           |
| `Starfield`            | `starfield`           |
| `Cyberpunk2077`        | `cyberpunk2077`       |
| `FalloutNewVegas`      | `fallout-new-vegas`   |
| `FalloutNV`            | `fallout-new-vegas`   |
| `Oblivion`             | `oblivion`            |
| `OblivionRemastered`   | `oblivion-remastered` |

## Per-game guides

Each shipped title has a dedicated guide with its engine, mod directory, save location, installer quirks, and Linux/Proton notes:

- Creation Engine: [Skyrim SE/AE](skyrim-se.md), [Fallout 4](fallout4.md), [Fallout 76](fallout76.md), [Starfield](starfield.md)
- Gamebryo: [Fallout: New Vegas](fallout-new-vegas.md), [Oblivion](oblivion.md)
- REDengine: [Cyberpunk 2077](cyberpunk2077.md), [The Witcher 3](witcher3.md)
- Unreal 4/5: [Stellar Blade](stellar-blade.md), [Subnautica 2](subnautica2.md), [Oblivion Remastered](oblivion-remastered.md)
- Other engines: [Baldur's Gate 3](baldurs-gate3.md) (Larian), [Stardew Valley](stardew-valley.md) (SMAPI), [Mount & Blade II: Bannerlord](bannerlord.md)

## Adding game support

modde ships built-in game plugins for the titles above. Each implements the `GamePlugin` trait, which provides archive analysis and mod-layout recognition, filesystem scanning and mod discovery, conflict classification, Wine/Proton DLL-override detection, and save-game tracking.

Beyond the built-in list, you can register **user-defined games** at runtime with `modde game add` (or import a shareable `GameSpec` TOML). This is the `Generic game support` capability — it ships today as a `Partial` feature: deployment, conflict classification, and launcher integration work, but there is no bespoke scanner or save tracker for arbitrary titles. See [Generic & user-defined games](generic-games.md) for the workflow and the TOML schema.
