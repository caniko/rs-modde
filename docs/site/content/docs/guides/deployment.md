+++
title = "Deployment & VFS"
description = "How modde deploys mods via the virtual filesystem"
weight = 25
+++

## Overview

modde uses a **symlink farm** to deploy mods without modifying the original game files. Mods are staged in an intermediate directory and then symlinked into the game directory, keeping your game installation clean and reversible.

## How deployment works

The deployment pipeline has three phases:

### 1. Build

modde resolves the load order and builds a map of relative file paths to their source locations. For each file path, the mod with the highest priority (latest in load order) wins. Hidden files and profile-level overrides are applied during this phase.

**Priority order** (highest to lowest):
1. Profile overrides directory
2. Later mods in the load order
3. Earlier mods in the load order

### 2. Materialize

The symlink farm is written to disk in the staging directory (`~/.local/share/modde/staging/<profile>/`). Each file becomes a symlink pointing to the winning mod's content in the store.

### 3. Deploy

The materialized staging directory is symlinked into the game directory. Each relative path in the staging directory becomes a symlink under the game's mod directory.

## Deploying

### Automatic deployment

After rebuilding your NixOS/home-manager configuration, modde deploys profiles automatically via an activation script when their prerequisites are present. Wabbajack profiles can wait non-fatally for the game install: set `installMode = "await-game"` or leave `gameDir` unset until Steam or Heroic has installed the game, then set `gameDir` and rebuild.

### Manual deployment

```bash
# Deploy the active profile
modde deploy

# Deploy a specific profile
modde deploy --profile my-skyrim --game skyrim-se
```

### Deploy via play

The `modde play` command deploys before launching the game:

```bash
modde play --game skyrim-se
```

## Rollback

Every deployment preserves the previous state in `staging.bak/`. To revert:

```bash
modde rollback --profile my-skyrim
```

This atomically swaps the staging directories, restoring the previous deployment.

## Wabbajack profiles

Wabbajack-installed profiles use a different deployment strategy: files are hardlinked (or copied) directly from the staging directory to the game directory, bypassing the VFS symlink farm. This is because Wabbajack modlists include pre-built file layouts that don't need conflict resolution.

For Home Manager profiles, Wabbajack install happens before deployment only when the configured `gameDir` exists and contains the expected game content directory, such as `Data/` for Skyrim SE. Otherwise activation prints an awaiting message and continues.

## Verifying integrity

Check that deployed files match their expected sources:

```bash
modde verify --profile my-skyrim
```

## Vanilla snapshots

Capture and verify the original game installation to detect unexpected modifications:

```bash
# Capture a vanilla snapshot
modde stock snapshot skyrim-se

# Verify against the snapshot later
modde stock verify skyrim-se
```
