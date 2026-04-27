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
modde does not install Steam or Heroic games; install the game with its launcher
first, then point `gameDir` at the installed game directory.

## Prerequisites

- A Nexus Mods API key (required for downloading mods)
- The game must be installed and detected by modde
- The game install path for lists that reference local vanilla game files
- Sufficient disk space for both downloads and the deployed modlist

Some Skyrim SE lists, including Legends of the Frost, include Wabbajack
`GameFileSourceDownloader` entries. Those entries are not downloads; they point
at files from the local Skyrim installation. Pass `--game-dir` or set
`gameDir` in Home Manager so modde can read and verify those files.

## Declarative installation

```nix
programs.modde = {
  enable = true;
  nexus.apiKeyFile = "/run/secrets/nexus-api-key";

  profiles.my-modlist = {
    game = "skyrim-se";
    installMode = "auto";
    gameDir = "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition";
    wabbajackList = {
      url = "https://example.com/modlist.wabbajack";
      hash = "sha256-...";
    };
  };
};
```

If the modlist is already available locally, for example through
`pkgs.requireFile` or a custom fetcher, use `path` instead of `url` and `hash`:

```nix
programs.modde.profiles.my-modlist = {
  game = "skyrim-se";
  gameDir = "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition";
  wabbajackList = {
    path = /nix/store/...-Legends-of-the-Frost.wabbajack;
  };
};
```

Use `installMode = "await-game"` while the game is not installed yet. Activation
will skip install/deploy and print the next step instead of failing.

## CLI installation

```bash
modde install wabbajack /path/to/modlist.wabbajack \
  --profile my-modlist \
  --game-dir "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition"
```

Wabbajack registry pages usually link through to an authored-files URL for the
actual `.wabbajack` archive. Some authored-files CDN links now resolve through
Wabbajack's chunked download page instead of a plain file response, so
`pkgs.fetchurl` may 404 even when modde's chunk-aware downloader works. In that
case, download the file with `modde wabbajack download` and use
`wabbajackList.path`, or use a dedicated fetcher that reconstructs the chunks.

## Missing authored-files archives

Some Wabbajack modlists reference generated authored-files archives hosted by
Wabbajack. If those upstream entries disappear, modde fails before bulk
downloads and prints every missing archive plus a `curl -fI` validation command.
modde will not substitute similarly named files because the manifest hash is the
only safe identity for an archive.

If you already have the exact missing archives in an old Wabbajack cache or a
backup, import them into the modde store:

```bash
modde wabbajack import-archive /path/to/list.wabbajack \
  /path/to/missing-archive-1.7z \
  /path/to/missing-archive-2.7z
```

The import command hashes each file and imports only archives whose Wabbajack
hash matches an archive referenced by the manifest. Filename-only matches are
reported as mismatches and are not imported.

## Supported games

Not all Wabbajack games are supported yet. See the [Wabbajack game mapping](@/docs/games/supported-games.md#wabbajack-game-mapping) for the current list.
