+++
title = "Supported Games"
description = "Games currently supported by modde"
weight = 10
+++

## Bethesda titles

| Game | ID | Mod scanner | Conflict detection | Save tracking |
|------|----|-------------|--------------------|---------------|
| Skyrim Special Edition | `skyrim-se` | Yes | Yes | Yes |
| Skyrim Anniversary Edition | `skyrim-ae` | Yes | Yes | Yes |
| Fallout 4 | `fallout4` | Yes | Yes | Yes |
| Fallout 76 | `fallout76` | — | Yes | Server-side |
| Starfield | `starfield` | Yes | Yes | Yes |

## Other games

| Game | ID | Mod scanner | Conflict detection | Save tracking |
|------|----|-------------|--------------------|---------------|
| Cyberpunk 2077 | `cyberpunk2077` | Yes | Yes | Yes |
| Stellar Blade | `stellar-blade` | Yes | — | — |

## Wabbajack game mapping

When installing Wabbajack modlists, the manifest game names are mapped to modde game IDs:

| Wabbajack name | modde ID |
|----------------|----------|
| `SkyrimSpecialEdition` | `skyrim-se` |
| `Fallout4` | `fallout4` |
| `Fallout76` | `fallout76` |
| `Starfield` | `starfield` |
| `Cyberpunk2077` | `cyberpunk2077` |

## Adding game support

modde uses a plugin system for game support. Each game implements the `GamePlugin` trait, which provides:

- Archive analysis and mod layout recognition
- File scanning and mod discovery
- Conflict classification
- Save game tracking
