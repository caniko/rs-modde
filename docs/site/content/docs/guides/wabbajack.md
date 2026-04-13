+++
title = "Wabbajack Modlists"
description = "Installing Wabbajack modlists with modde"
weight = 10
+++

## Overview

[Wabbajack](https://www.wabbajack.org/) is a modlist installer originally built for Windows. modde can parse `.wabbajack` archives and install their modlists on Linux via NixOS.

## How it works

A `.wabbajack` file is a compressed archive containing:

1. A **manifest** describing every mod, its download source, and installation directives
2. **Patches** for files that need binary-level modifications
3. **Installation instructions** specifying how to deploy files into the game directory

modde parses this manifest, resolves download URLs (primarily from Nexus Mods), downloads the required archives, and deploys them according to the instructions.

## Prerequisites

- A Nexus Mods API key (required for downloading mods)
- The game must be installed and detected by modde
- Sufficient disk space for both downloads and the deployed modlist

## Declarative installation

```nix
programs.modde = {
  enable = true;
  nexus.apiKeyFile = "/run/secrets/nexus-api-key";

  profiles.my-modlist = {
    game = "skyrim-se";
    wabbajackList = {
      url = "https://example.com/modlist.wabbajack";
      hash = "sha256-...";
    };
  };
};
```

## CLI installation

```bash
modde install --wabbajack /path/to/modlist.wabbajack --profile my-modlist
```

## Supported games

Not all Wabbajack games are supported yet. See the [Wabbajack game mapping](@/docs/games/supported-games.md#wabbajack-game-mapping) for the current list.
