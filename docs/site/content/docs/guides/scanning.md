+++
title = "Mod Scanning"
description = "Discover installed mods by scanning game directories"
weight = 50
+++

## Overview

The `modde scan` command discovers mods already installed on disk by analysing the game directory structure. This is useful for importing existing mod setups into modde or for matching on-disk files against a Wabbajack manifest.

## Basic scanning

```bash
modde scan --game cyberpunk2077
```

modde uses game-specific scanners that understand each game's mod directory layout (e.g., CET scripts, REDscript, REDmod, archive mods for Cyberpunk; Data directory structure for Bethesda games).

## Importing results into a profile

```bash
modde scan --game cyberpunk2077 --import-to my-cyberpunk
```

Discovered mods are added to the specified profile. If the profile doesn't exist, it's created.

## Wabbajack manifest matching

When migrating from a Wabbajack install, match on-disk files against the manifest:

```bash
modde scan --game skyrim-se \
  --manifest /path/to/modlist.wabbajack \
  --import-to my-modlist
```

This:
1. Parses the `.wabbajack` manifest
2. Matches manifest archives against on-disk files
3. Imports matched mods with their Wabbajack metadata
4. Applies a Wabbajack load order lock to the profile

## Threshold filtering

The `--threshold` flag controls how many of a mod's expected files must be present for a match:

```bash
# Require 80% of expected files to be present
modde scan --game skyrim-se --threshold 0.8 --manifest /path/to/modlist.wabbajack

# More lenient matching (default is 0.5)
modde scan --game skyrim-se --threshold 0.3 --manifest /path/to/modlist.wabbajack
```

## Dry-run mode

Preview what would be discovered without writing to the database:

```bash
modde scan --game skyrim-se --dry-run
```

## Pruning duplicates

When combining manifest matching with filesystem scanning, some mods may be detected by both methods. The `--prune-duplicates` flag removes filesystem-scanner entries that are already covered by the manifest:

```bash
modde scan --game skyrim-se \
  --manifest /path/to/modlist.wabbajack \
  --import-to my-modlist \
  --prune-duplicates
```

For existing profiles, use `profile dedup` instead:

```bash
modde profile dedup my-modlist --manifest /path/to/modlist.wabbajack --apply
```
