+++
title = "Supported Games"
description = "Games currently supported by modde"
weight = 10
+++

This table is intentionally conservative:

- `Done` means the game path is shipped end to end.
- `Partial` means some core pieces exist, but major workflows are still missing or not yet trustworthy.
- `Not shipped` means the capability should not be treated as available.
- The canonical status baseline for this page lives in `docs/capability-matrix.toml` in the repository.

## Bethesda titles

| Game | ID | Overall status | Scanner | Conflict detection | Save tracking |
|------|----|----------------|---------|--------------------|---------------|
| Skyrim Special Edition | `skyrim-se` | `Done` | Yes | Yes | `Done` |
| Skyrim Anniversary Edition | `skyrim-ae` | `Done` | Yes | Yes | `Done` |
| Fallout 4 | `fallout4` | `Done` | Yes | Yes | `Done` |
| Fallout 76 | `fallout76` | `Partial` | Yes | Yes | `Partial` (server-side / local cache only) |
| Starfield | `starfield` | `Partial` | Yes | Yes | `Done` |

## Other games

| Game | ID | Overall status | Scanner | Conflict detection | Save tracking |
|------|----|----------------|---------|--------------------|---------------|
| Cyberpunk 2077 | `cyberpunk2077` | `Done` | Yes | Yes | `Done` |
| Stellar Blade | `stellar-blade` | `Partial` | Yes | Yes | `Done` |
| Baldur's Gate 3 | `baldurs-gate3` | `Partial` | Yes | Yes | `Done` |
| Stardew Valley | `stardew-valley` | `Partial` | Yes | Yes | `Done` |
| Fallout: New Vegas | `fallout-new-vegas` | `Partial` | Yes | Yes | `Done` |
| The Elder Scrolls IV: Oblivion | `oblivion` | `Partial` | Yes | Yes | `Done` |
| The Elder Scrolls IV: Oblivion Remastered | `oblivion-remastered` | `Partial` | Yes | Yes | `Done` |
| Mount & Blade II: Bannerlord | `bannerlord` | `Partial` | Yes | Yes | `Done` |
| The Witcher 3: Wild Hunt | `witcher3` | `Partial` | Yes | Yes | `Done` |

## Wabbajack game mapping

When installing Wabbajack modlists, the manifest game names are mapped to modde game IDs:

| Wabbajack name | modde ID |
|----------------|----------|
| `SkyrimSpecialEdition` | `skyrim-se` |
| `Fallout4` | `fallout4` |
| `Fallout76` | `fallout76` |
| `Starfield` | `starfield` |
| `Cyberpunk2077` | `cyberpunk2077` |
| `FalloutNewVegas` | `fallout-new-vegas` |
| `FalloutNV` | `fallout-new-vegas` |
| `Oblivion` | `oblivion` |
| `OblivionRemastered` | `oblivion-remastered` |

## Adding game support

modde uses built-in game plugins for shipped game support. `GenericGame` exists as an internal helper, but it is not a polished catch-all feature for arbitrary titles. Each real shipped game implements the `GamePlugin` trait, which provides:

- Archive analysis and mod layout recognition
- File scanning and mod discovery
- Conflict classification
- Save game tracking
