+++
title = "Home-Manager Module"
description = "Reference for all modde home-manager module options"
weight = 10
+++

## Module options

### `programs.modde.enable`

Whether to enable the modde mod manager.

- **Type:** `bool`
- **Default:** `false`

### `programs.modde.package`

The modde package to use.

- **Type:** `package`
- **Default:** `modde` from the flake

### `programs.modde.profiles`

An attribute set of mod profiles to manage. Each profile is a submodule with the following options:

#### `profiles.<name>.game`

Game identifier string.

- **Type:** `str`
- **Required:** yes
- **Values:** `"skyrim-se"`, `"skyrim-ae"`, `"fallout4"`, `"fallout76"`, `"starfield"`, `"cyberpunk2077"`, `"stellar-blade"`

#### `profiles.<name>.wabbajackList`

Wabbajack modlist source. Mutually exclusive with `nexusCollection`.

- **Type:** `null` or submodule
- **Default:** `null`

| Option | Type | Description |
|--------|------|-------------|
| `url` | `str` | URL to the `.wabbajack` modlist file |
| `hash` | `str` | SHA-256 hash of the modlist file |

#### `profiles.<name>.nexusCollection`

Nexus Collection source. Mutually exclusive with `wabbajackList`.

- **Type:** `null` or submodule
- **Default:** `null`

| Option | Type | Description |
|--------|------|-------------|
| `slug` | `str` | Nexus Collection slug |
| `version` | `str` | Collection version to install |

### `programs.modde.nexus.apiKeyFile`

Path to a file containing the Nexus Mods API key. Compatible with sops-nix secrets.

- **Type:** `null` or `path`
- **Default:** `null`

## Example configuration

```nix
programs.modde = {
  enable = true;

  nexus.apiKeyFile = "/run/secrets/nexus-api-key";

  profiles = {
    living-skyrim = {
      game = "skyrim-se";
      wabbajackList = {
        url = "https://example.com/living-skyrim.wabbajack";
        hash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
      };
    };

    cyberpunk-mods = {
      game = "cyberpunk2077";
      nexusCollection = {
        slug = "my-cyberpunk-collection";
        version = "2.1.0";
      };
    };
  };
};
```
