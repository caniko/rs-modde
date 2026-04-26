+++
title = "Nexus Mods"
description = "Set up Nexus API authentication, browse, download, and install mods"
weight = 15
+++

## Overview

modde integrates with [Nexus Mods](https://www.nexusmods.com/) for browsing, downloading, and installing mods. A Nexus Mods account with an API key is required; downloading via CDN links requires a **Premium** subscription.

## Authentication

modde checks for API credentials in this order:

1. `NEXUS_API_KEY` environment variable
2. System keyring (secret-service D-Bus)
3. `NEXUS_API_KEY_FILE` environment variable (path to a file containing the key)
4. `~/.config/modde/nexus_api_key` file

### Saving your API key

The recommended approach is to save your key to the system keyring:

```bash
modde nexus auth
```

This prompts for your API key and stores it securely via D-Bus secret-service.

### Using sops-nix (declarative)

For NixOS/home-manager setups, point to a sops-nix managed secret:

```nix
programs.modde = {
  enable = true;
  nexus.apiKeyFile = "/run/secrets/nexus-api-key";
};
```

### Checking status

```bash
modde nexus status
```

Shows whether your API key is valid and whether you have Premium access.

## Installing mods

### Single mod from Nexus

```bash
modde install mod https://www.nexusmods.com/skyrimspecialedition/mods/12345 --profile my-skyrim
```

modde fetches the mod files, selects the latest main file, downloads via CDN (requires Premium), extracts, and adds it to your profile.

If the mod uses FOMOD, you can provide a declarative config for non-interactive installation:

```bash
modde install mod <url> --profile my-skyrim --fomod-config my-choices.toml
```

### Nexus Collections

Install a curated collection of mods:

```bash
modde install nexus-collection <slug> --profile my-collection
```

Or declaratively:

```nix
programs.modde.profiles.my-collection = {
  game = "skyrim-se";
  nexusCollection = {
    slug = "collection-slug";
    version = "1.0.0";
  };
};
```

Collections auto-lock the profile to preserve the curator's intended load order.

## nxm:// protocol handler

To enable "Download with modde" buttons on the Nexus Mods website:

```bash
modde nxm install
```

This registers a desktop handler for `nxm://` URIs. When you click a download link on Nexus, modde receives the URI and downloads the mod:

```bash
# Manually handle an nxm:// link
modde nxm handle "nxm://skyrimspecialedition/mods/12345/files/67890?key=abc&expires=123"
```

## Checking for updates

```bash
# Check for updates in the last week (default)
modde update check --profile my-skyrim

# Check updates from the last day or month
modde update check --profile my-skyrim --period 1d
modde update check --profile my-skyrim --period 1m
```
