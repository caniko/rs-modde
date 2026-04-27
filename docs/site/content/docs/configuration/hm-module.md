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

#### `profiles.<name>.gameDir`

Runtime path to the game installation. This is required for Wabbajack modlists
that reference local vanilla game files, including Skyrim SE lists such as
Legends of the Frost.

- **Type:** `null`, path, or string
- **Default:** `null`

#### `profiles.<name>.installMode`

Controls what Home Manager activation does for this profile.

- **Type:** `"auto"`, `"await-game"`, or `"disabled"`
- **Default:** `"auto"`

| Value | Behavior |
|-------|----------|
| `"auto"` | Install/deploy when prerequisites are present; otherwise print an awaiting message |
| `"await-game"` | Always skip install/deploy and print the next setup step |
| `"disabled"` | Skip all activation work for the profile |

#### `profiles.<name>.wabbajackList`

Wabbajack modlist source. Mutually exclusive with `nexusCollection`.

- **Type:** `null` or submodule
- **Default:** `null`

| Option | Type | Description |
|--------|------|-------------|
| `url` | `null` or `str` | URL to the `.wabbajack` modlist file |
| `hash` | `null` or `str` | Nix fetch hash for the `.wabbajack` file |
| `path` | `null`, `path`, or `str` | Local or Nix store path to an already available `.wabbajack` file |

Set exactly one source: either `path`, or both `url` and `hash`. Use `path`
when composing with `requireFile` or another fetcher that already materializes
the `.wabbajack` file.

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
      installMode = "auto";
      gameDir = "/home/me/.local/share/Steam/steamapps/common/Skyrim Special Edition";
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

## First install flow

modde does not install Steam or Heroic games. You can declare the profile before
the game exists by omitting `gameDir` or setting `installMode = "await-game"`.
Home Manager activation will print the next step and continue without failing.

After installing Skyrim SE through Steam or Heroic, set `gameDir` to the game
installation directory and use `installMode = "auto"`. The next Home Manager
activation installs the Wabbajack profile if it is missing, then deploys it.
