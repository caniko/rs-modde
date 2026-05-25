+++
title = "Tools & Overlays"
description = "Manage MangoHud, vkBasalt, ReShade, OptiScaler, GameMode, and Proton"
weight = 35
+++

## Overview

modde can manage gaming tools and overlays that enhance or modify how games run. Supported tools include performance overlays, graphics filters, upscaling frameworks, performance boosters, and per-game Proton launch settings.
The current truthful scope is narrower than MO2: tool state, generated configs, launcher environment integration, and tracked file patching are shipped, but full executable-management workflows are not.

## Supported tools

| Tool ID      | Description                                                          |
| ------------ | -------------------------------------------------------------------- |
| `mangohud`   | Performance monitoring overlay (FPS, CPU, GPU, frame times)          |
| `vkbasalt`   | Vulkan post-processing layer (sharpening, CAS, FXAA)                 |
| `gamemode`   | Feral GameMode daemon for performance optimization                   |
| `reshade`    | ReShade post-processing shader framework                             |
| `optiscaler` | DLSS/FSR upscaling framework                                         |
| `proton`     | Per-game Proton, Wine prefix, environment, and DLL override settings |

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

Settings are stored per-game in the database and persisted across sessions. Enabled tools can contribute launch environment variables, wrapper commands, generated config files, and Wine DLL overrides during deployment.

## Proton settings

The `proton` tool stores launch compatibility settings for a game. It does not install Steam or the game itself. The UI can load the upstream GE-Proton version catalog from `GloriousEggroll/proton-ge-custom` and merges those versions with locally installed Steam compatibility tools. Installing a selected version still requires `protonup-rs`.

```bash
# Keep the launcher's default Proton runner
modde tool configure proton --game skyrim-se version_mode=launcher_default

# Export extra variables through modde's launcher integration
modde tool configure proton --game skyrim-se extra_env='PROTON_LOG=1'

# Force DLL overrides when a tool needs them
modde tool configure proton --game skyrim-se dll_override_mode=forced forced_dll_overrides=dxgi,winmm
```

Useful Proton settings include `version_mode`, `selected_version`, `prefix_path_override`, `extra_env`, `dll_override_mode`, `forced_dll_overrides`, and `wrapper_order`.

## Applying tool patches

Some tools (ReShade, OptiScaler) need DLLs and config files placed in the game directory:

```bash
# Apply ReShade to the game directory
modde tool apply reshade --game skyrim-se

# Revert the patches
modde tool revert reshade --game skyrim-se
```

modde records which files were patched so that `revert` can cleanly remove them. Proton, MangoHud, vkBasalt, and GameMode are launch/config integrations and do not normally apply files into the game directory.

For Stellar Blade, OptiScaler is managed in `SB/Binaries/Win64` and defaults to official release `official:v0.9.1` with `dxgi.dll` as the proxy. If an existing install is detected as `unmanaged; version v0.9.1; proxy dxgi.dll`, adopt it instead of reinstalling so modde records the working files. The Stellar Blade profile enables OptiPatcher and deploys `plugins/OptiPatcher.asi` so DLSS and DLSS-FG inputs can be unlocked without spoofing.

The OptiScaler source selector separates upstream releases from GOverlay packaging. `Official GitHub releases` lists `official:*` entries from `optiscaler/OptiScaler`. `GOverlay builds` lists GOverlay's external `benjamimgois/OptiScaler-builds` feed and exposes a channel selector for stable, bleeding-edge, master, or any-release builds. Bleeding-edge FSR 4.0.2-capable setups are commonly distributed through that GOverlay builds source rather than official OptiScaler releases. Existing local GOverlay installs remain available through the `GOverlay fgmod directory` source mode.

## Running external tools with overwrite capture

When you run tools like xEdit or BodySlide that modify mod files, modde can track the changes:

```bash
modde tool run /path/to/xedit --game skyrim-se -- -quickautoclean
```

This:

1. Snapshots the mod directory before the tool runs
2. Runs the tool with the game directory as working directory
3. Snapshots after the tool exits
4. Moves new or modified files into an `__overwrite__` mod

The overwrite mod can then be included in your profile to persist tool-generated changes across deployments.
