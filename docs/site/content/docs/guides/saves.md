+++
title = "Save Management"
description = "Git-backed save vaults, fingerprinting, auto-capture, and restore"
weight = 20
+++

## Overview

modde uses a **git-backed save vault** to manage save files per profile. Each game has its own git repository, and each profile is a branch. This gives you full history, snapshots, and the ability to restore saves to any previous point.

## How it works

- **Per-game vault**: A git repository at `~/.local/share/modde/saves/<game_id>/`
- **Per-profile branches**: Each profile gets its own branch in the vault
- **Automatic swapping**: When you switch profiles, saves are captured from the current profile and restored from the target profile
- **Steam Cloud aware live directory**: Steam's `steam_autocloud.vdf` marker is preserved, and outgoing root saves are parked under `.modde/profiles/<profile>/` before the next profile is restored
- **Fingerprinting**: SHA-256 hash of save-breaking mods is embedded in each snapshot, enabling compatibility warnings on restore

## Adopting existing saves

If you already have save files before setting up modde, adopt them into a profile:

```bash
modde save adopt --game skyrim-se --profile my-skyrim
```

This creates the initial vault branch and captures all existing saves as the first snapshot.
If no profile is active for that game yet, the adopted profile becomes active so the next profile switch can safely put those saves away.

## Steam Cloud

modde no longer treats the live save directory as disposable. During profile switches it keeps Steam Cloud metadata in place and moves inactive root saves into a modde-owned `.modde/` directory before restoring the incoming profile's saves at the root. Skyrim only sees the root save files, while the git vault remains the authoritative history for capture and restore.

This avoids deleting Steam's cloud marker and makes profile isolation compatible with cloud-synced save directories. If Steam later downloads stale root saves behind modde's back, switch profiles again or run `modde save restore` for the intended profile snapshot.

## Capturing saves

Saves are captured automatically in several situations:

- When you switch profiles (`profile switch`, `profile try`)
- When a game exits after `modde play`
- When you run `save auto-capture`

You can also capture manually at any time:

```bash
modde save capture --game skyrim-se --profile my-skyrim -m "before dragon fight"
```

### Auto-capture on game exit

The `modde play` command automatically captures saves when the game exits (unless `--no-capture` is passed).

### Continuous watching

For games launched outside of `modde play`, set up a watcher:

```bash
modde save watch --game skyrim-se --interval 60
```

This polls for save changes every 60 seconds and captures new snapshots.

## Browsing history

```bash
modde save history --game skyrim-se --profile my-skyrim --limit 10
```

Each entry shows the commit hash (short ID), timestamp, message, and mod fingerprint.

## Restoring saves

Restore saves from a specific snapshot:

```bash
modde save restore abc12345 --game skyrim-se --profile my-skyrim
```

If the mod fingerprint of the snapshot doesn't match your current profile, modde warns you about which save-breaking mods were added or removed since that snapshot. This helps you avoid loading a save that's incompatible with your current mod setup.

## Save fingerprinting

modde computes a SHA-256 fingerprint from the sorted list of enabled mods classified as "save-breaking" for each game. Two profiles with the same save-breaking mods produce the same fingerprint.

This fingerprint is stored in each save snapshot as a git commit trailer:

```
Mod-Fingerprint: a1b2c3d4e5f6
Save-Breaking-Mods: mod_a, mod_b, mod_c
```

When restoring, modde compares fingerprints and reports:

- **Compatible**: Fingerprints match — safe to restore
- **No fingerprint**: Pre-fingerprint snapshot — restore at your own risk
- **Mismatch**: Lists which mods were added or removed

## Managing save assignments

Track individual save files:

```bash
# Assign a save to a profile
modde save assign ~/saves/quicksave.ess --profile my-skyrim --label "quick save"

# List assigned saves
modde save list --profile my-skyrim

# Find unassigned saves
modde save scan --game skyrim-se

# Remove a save assignment
modde save unassign ~/saves/quicksave.ess
```
