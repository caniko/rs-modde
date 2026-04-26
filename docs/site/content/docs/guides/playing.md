+++
title = "Playing a Game"
description = "The modde play command: deploy, launch, and capture saves"
weight = 30
+++

## Overview

The `modde play` command is the primary way to launch a modded game. It orchestrates the full workflow: switch profile, deploy mods, launch the game, and capture saves on exit.

## Basic usage

```bash
modde play --game skyrim-se
```

This uses the active profile. To specify a profile:

```bash
modde play my-skyrim --game skyrim-se
```

## What happens

1. **Profile switch** — If the specified profile isn't already active, modde switches to it (capturing current saves and restoring target saves)
2. **Mod deployment** — Builds and deploys the symlink farm
3. **Game launch** — Detects the launcher (Steam or Heroic) and launches the game
4. **Save capture** — When the game exits, automatically captures saves into the vault with a mod fingerprint

## Skipping steps

| Flag | Effect |
|------|--------|
| `--no-switch` | Skip profile switch (deploy + launch the active profile) |
| `--no-deploy` | Skip deployment (switch + launch without redeploying) |
| `--no-capture` | Skip save auto-capture after game exit |

## Steam vs Heroic

- **Heroic/direct launch**: modde waits for the game process to exit, then captures saves
- **Steam launch**: Steam returns immediately (fire-and-forget). modde informs you that the launch wrapper will handle save capture. If needed, run manually:

```bash
modde save auto-capture --game skyrim-se
```

## Game detection

modde auto-detects game installations via Steam and Heroic (GOG, Epic, Sideload). To see detected games:

```bash
modde detect
```
