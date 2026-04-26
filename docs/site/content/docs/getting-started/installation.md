+++
title = "Installation"
description = "Install modde on NixOS"
weight = 10
+++

## Flake input

Add modde to your flake inputs:

```nix
{
  inputs.modde = {
    url = "codeberg:caniko/rs-modde";
    inputs.nixpkgs.follows = "nixpkgs";
  };
}
```

## Home-Manager module

The recommended way to use modde is through the home-manager module:

```nix
{ inputs, ... }:
{
  imports = [ inputs.modde.homeManagerModules.modde ];

  programs.modde = {
    enable = true;
  };
}
```

You can declare Wabbajack profiles before the game is installed. Use
`installMode = "await-game"` or leave `gameDir` unset, install the game through
Steam or Heroic, then set `gameDir` and rebuild Home Manager:

```nix
programs.modde.profiles.lotf = {
  game = "skyrim-se";
  installMode = "await-game";
  wabbajackList = {
    url = "https://example.com/lotf.wabbajack";
    hash = "sha256-...";
  };
};
```

modde waits for launcher-managed game installs; it does not install the base
game itself.

## Standalone package

You can also install modde as a standalone package:

```nix
environment.systemPackages = [
  inputs.modde.packages.x86_64-linux.modde
];
```

Or run it directly:

```bash
nix run codeberg:caniko/rs-modde
```
