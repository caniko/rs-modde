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
