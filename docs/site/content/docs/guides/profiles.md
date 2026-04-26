+++
title = "Profile Management"
description = "Create, switch, fork, lock, and experiment with mod profiles"
weight = 5
+++

## Overview

A **profile** is a named, per-game collection of mods with its own load order, save assignments, and deployment state. Profiles are the central organizing concept in modde.

## Creating profiles

### Via home-manager (declarative)

```nix
programs.modde.profiles.my-skyrim = {
  game = "skyrim-se";
};
```

For Wabbajack profiles, Home Manager can also manage the first install. If the
game is not installed yet, keep the profile in an awaiting state:

```nix
programs.modde.profiles.my-skyrim = {
  game = "skyrim-se";
  installMode = "await-game";
  wabbajackList = {
    url = "https://example.com/modlist.wabbajack";
    hash = "sha256-...";
  };
};
```

After installing the game through Steam or Heroic, set `gameDir` and switch back
to the default `installMode = "auto"`.

### Via CLI (imperative)

```bash
modde profile create my-skyrim --game skyrim-se
```

## Listing and switching

```bash
# List all profiles
modde profile list

# List profiles for a specific game
modde profile list --game skyrim-se

# See which profile is active
modde profile active --game skyrim-se

# Switch to a profile (automatically swaps saves)
modde profile switch my-skyrim --game skyrim-se
```

When you switch profiles, modde captures the current profile's saves and restores the target profile's saves automatically.

## Forking profiles

Fork creates a full clone of a profile — mods, load order rules, locks, and saves.

```bash
modde profile fork my-skyrim my-skyrim-experimental --game skyrim-se
```

If the source profile was locked (e.g., from a Wabbajack install), the fork inherits that lock. To create a freely editable copy:

```bash
modde profile fork my-skyrim my-skyrim-custom --game skyrim-se --unlock
```

This strips both the profile-level lock and all per-mod pins.

## Load order locking

Profiles can be locked to prevent accidental reordering.

### Automatic locks

Locks are applied automatically when installing from:
- **Wabbajack modlists** — locked with the manifest hash for provenance
- **Nexus Collections** — locked with the collection slug and version
- **TOML imports** — locked with the source file path

### Manual locks

```bash
# Lock a profile
modde profile lock my-skyrim --note "tested and stable"

# Check lock status
modde profile lock-info my-skyrim

# Unlock
modde profile unlock my-skyrim
```

### Per-mod pins

Pin individual mods while leaving the rest freely reorderable:

```bash
# Pin a mod in place
modde profile lock-mod my-skyrim "skyrim_12345_67890" --note "load order sensitive"

# Unpin
modde profile unlock-mod my-skyrim "skyrim_12345_67890"
```

When a profile is locked, you can still add new mods (they append to the end), but you cannot reorder or remove locked mods.

## Experiment stack

The experiment stack lets you try profile changes non-destructively, like git branches for your mod setup.

```bash
# Push a profile onto the stack
modde profile try experimental-skyrim --game skyrim-se

# Try another on top (stacks)
modde profile try ultra-graphics --game skyrim-se

# Something broke? Roll back one level
modde profile rollback --game skyrim-se

# Happy with the result? Commit (clears the stack)
modde profile commit --game skyrim-se
```

The stack works like this:
```
Active = Profile A
try B   → Stack = [A],    Active = B
try C   → Stack = [B, A], Active = C
rollback → Stack = [A],    Active = B
rollback → Stack = [],     Active = A
commit   → Stack = [],     Active = current (clears history)
```

Each try/rollback automatically swaps saves along with the profile.

## Deduplication

When combining Wabbajack installs with filesystem scanning, some mods may be detected twice. The `dedup` command finds and removes these duplicates:

```bash
# Dry-run to see what would be removed
modde profile dedup my-modlist --manifest /path/to/modlist.wabbajack

# Actually remove duplicates
modde profile dedup my-modlist --manifest /path/to/modlist.wabbajack --apply
```

## Deleting profiles

```bash
modde profile delete my-skyrim
```
