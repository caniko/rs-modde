+++
title = "Troubleshooting"
description = "Common issues and how to resolve them"
weight = 100
+++

## Game not detected

modde auto-detects games via Steam and Heroic (GOG, Epic, Sideload).

```bash
modde detect
```

If your game doesn't appear:
- Verify the game is installed via a supported launcher
- For Steam, ensure the game's `appmanifest_*.acf` file exists in your Steam library
- For Heroic, check that the game appears in `~/.config/heroic/GamesConfig/`
- Use `--game-dir` flags to specify the path manually

## Nexus API authentication fails

Check your API key status:

```bash
modde nexus status
```

If invalid, re-authenticate:

```bash
modde nexus auth
```

**API key lookup order**: `NEXUS_API_KEY` env var, system keyring, `NEXUS_API_KEY_FILE` env var, `~/.config/modde/nexus_api_key` file.

**CDN downloads require Premium**: Free Nexus accounts cannot generate CDN download links. You'll see an error if you try to download without Premium.

## Deployment fails

### Symlink errors

If deployment fails with symlink errors:
- Check that the target game directory exists and is writable
- Ensure no other process has locked files in the game directory
- Verify the staging directory at `~/.local/share/modde/staging/<profile>/` is intact

### Rollback

If a deployment left the game in a broken state:

```bash
modde rollback --profile my-skyrim
```

This swaps back to the previous deployment.

## Save vault issues

### "Adoption required"

When switching to a profile for the first time and existing saves are detected, modde requires you to adopt them:

```bash
modde save adopt --game skyrim-se --profile my-skyrim
```

### Restoring from an incompatible snapshot

If `save restore` warns about a fingerprint mismatch, it means the save was created with a different set of save-breaking mods. You can still restore, but the save may not load correctly in-game.

Browse history to find a compatible snapshot:

```bash
modde save history --game skyrim-se --profile my-skyrim
```

## Profile is locked

If you can't reorder mods, the profile may be locked:

```bash
modde profile lock-info my-skyrim
```

To unlock:

```bash
modde profile unlock my-skyrim
```

Or fork the profile with `--unlock` to create a freely editable copy:

```bash
modde profile fork my-skyrim my-skyrim-custom --game skyrim-se --unlock
```

## FOMOD install stuck on "pending user input"

When a mod's install status is `PendingUserInput`, it has a FOMOD installer that needs your selections. Either:

1. Open the GUI (`modde gui`) and complete the wizard
2. Generate and apply a declarative config:

```bash
modde fomod generate /path/to/mod --format toml > choices.toml
# Edit choices.toml to select your options
modde fomod apply /path/to/mod --config choices.toml --dest /path/to/output
```

## Unknown install type (dossier written)

When modde can't determine how to install a mod, it writes a dossier to `~/.local/share/modde/unknown-installers/<slug>/` containing the archive tree, file samples, and metadata.

```bash
modde mod diagnose <mod_id>
```

This prints the dossier path for investigation.

## Missing extractors

modde requires `7z` (7-Zip) and `unrar` for extracting certain archive formats. On NixOS, these are included in the development shell. If running standalone, ensure they are on your `PATH`.

## Database issues

The SQLite database is at `~/.local/share/modde/modde.db`. If it becomes corrupted:

1. Check if a `.db-journal` or `.db-wal` file exists (SQLite recovery files)
2. Back up the database before attempting repairs
3. Try running any modde command — the schema migration system may repair minor issues

## Exporting your mod list

For sharing or debugging, export your profile to CSV:

```bash
modde export --profile my-skyrim --output modlist.csv
```
