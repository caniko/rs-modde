+++
title = "FOMOD Installer"
description = "Interactive and declarative FOMOD mod installation"
weight = 40
+++

## Overview

[FOMOD](https://wiki.nexusmods.com/index.php/FOMOD) is a mod installer format that presents users with choices (e.g., texture resolution, compatibility patches). modde supports FOMOD both interactively (via the GUI wizard) and declaratively (via config files).

## Interactive installation

When you install a mod that contains a FOMOD installer (`fomod/ModuleConfig.xml`), modde detects it automatically. In the GUI, a step-by-step wizard presents the available options. Your selections are saved to the profile and replayed on future deployments.

## Declarative installation

For reproducible, non-interactive installs, you can define your FOMOD choices in a config file.

### Generating a config template

```bash
# Generate a template with default selections
modde fomod generate /path/to/mod --format toml

# Include all plugins (not just defaults)
modde fomod generate /path/to/mod --all --format toml
```

Output formats: `toml` (default), `json`, `nix`.

### Applying a config

```bash
modde fomod apply /path/to/mod --config my-choices.toml --dest /path/to/output
```

### During mod installation

Pass a config when installing from Nexus:

```bash
modde install mod https://nexusmods.com/skyrimspecialedition/mods/12345 \
  --profile my-skyrim \
  --fomod-config my-choices.toml
```

## Inspecting FOMOD structure

See what options a mod offers without installing it:

```bash
modde fomod inspect /path/to/mod
```

This prints the module name, installation steps, groups, and available plugins with their file counts and descriptions.
