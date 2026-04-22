+++
title = "Tools & Overlays"
description = "Manage MangoHud, vkBasalt, ReShade, OptiScaler, and GameMode"
weight = 35
+++

## Overview

modde can manage gaming tools and overlays that enhance or modify how games run. Supported tools include performance overlays, graphics filters, upscaling frameworks, and performance boosters.
The current truthful scope is narrower than MO2: tool state and tracked file patching are shipped, but full executable-management workflows are not.

## Supported tools

| Tool ID | Description |
|---------|-------------|
| `mangohud` | Performance monitoring overlay (FPS, CPU, GPU, frame times) |
| `vkbasalt` | Vulkan post-processing layer (sharpening, CAS, FXAA) |
| `gamemode` | Feral GameMode daemon for performance optimization |
| `reshade` | ReShade post-processing shader framework |
| `optiscaler` | DLSS/FSR upscaling framework |

## Checking tool status

```bash
# List detected tools for a game
modde tool list --game skyrim-se

# Show enabled/disabled status for all tools
modde tool status --game skyrim-se
```

## Enabling and configuring tools

```bash
# Enable MangoHud
modde tool enable mangohud --game skyrim-se

# Configure it
modde tool configure mangohud --game skyrim-se position=top-left fps_limit=60

# Disable it
modde tool disable mangohud --game skyrim-se
```

Settings are stored per-game in the database and persisted across sessions.

## Applying tool patches

Some tools (ReShade, OptiScaler) need DLLs and config files placed in the game directory:

```bash
# Apply ReShade to the game directory
modde tool apply reshade --game skyrim-se

# Revert the patches
modde tool revert reshade --game skyrim-se
```

modde records which files were patched so that `revert` can cleanly remove them.

## Running external tools with overwrite capture

When you run tools like xEdit or BodySlide that modify mod files, modde can track the changes:

```bash
modde tool run /path/to/xedit -- --game skyrim-se
```

This:
1. Snapshots the mod directory before the tool runs
2. Runs the tool with the game directory as working directory
3. Snapshots after the tool exits
4. Moves new or modified files into an `__overwrite__` mod

The overwrite mod can then be included in your profile to persist tool-generated changes across deployments.
