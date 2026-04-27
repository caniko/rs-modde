+++
title = "Quick Start"
description = "Get up and running with modde"
weight = 20
+++

## Create a profile

A profile ties a set of mods to a specific game. The simplest way to define one is in your home-manager configuration:

```nix
programs.modde = {
  enable = true;

  profiles.my-skyrim = {
    game = "skyrim-se";
  };
};
```

## Install from a Wabbajack modlist

To install a Wabbajack modlist, add the `wabbajackList` option to your profile:

```nix
programs.modde = {
  enable = true;

  nexus.apiKeyFile = "/run/secrets/nexus-api-key";  # sops-nix compatible

  profiles.living-skyrim = {
    game = "skyrim-se";
    gameDir = "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition";
    wabbajackList = {
      url = "https://example.com/modlist.wabbajack";
      hash = "sha256-...";
    };
  };
};
```

`hash` is the Nix fetch hash for the `.wabbajack` file. Skyrim SE lists that
reference vanilla game files need `gameDir` so modde can verify those local
files during installation.

For Wabbajack files you already have in the Nix store, use a local path source
instead:

```nix
programs.modde.profiles.living-skyrim = {
  game = "skyrim-se";
  gameDir = "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition";
  wabbajackList = {
    path = /nix/store/...-Living-Skyrim.wabbajack;
  };
};
```

If Skyrim is not installed yet, you can declare the profile first and let Home
Manager wait:

```nix
programs.modde.profiles.living-skyrim = {
  game = "skyrim-se";
  installMode = "await-game";
  wabbajackList = {
    url = "https://example.com/modlist.wabbajack";
    hash = "sha256-...";
  };
};
```

Install Skyrim through Steam or Heroic, set `gameDir`, change `installMode` back
to `"auto"` or remove it, and rebuild Home Manager. modde will then install and
deploy the profile.

## Install from a Nexus Collection

Alternatively, install from a Nexus Collection:

```nix
programs.modde.profiles.my-collection = {
  game = "skyrim-se";
  nexusCollection = {
    slug = "collection-slug";
    version = "1.0.0";
  };
};
```

## Deploy

After rebuilding your NixOS/home-manager configuration, modde automatically deploys your profiles via a home-manager activation script.

You can also deploy manually:

```bash
modde deploy --profile my-skyrim
```
