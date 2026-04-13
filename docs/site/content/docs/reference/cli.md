+++
title = "CLI Reference"
description = "Complete reference for all modde commands"
weight = 10
+++

## Global flags

| Flag | Description |
|------|-------------|
| `--data-dir <path>` | Override data directory (default: `~/.local/share/modde` or `$MODDE_DATA_DIR`) |

---

## `modde profile`

Manage mod profiles.

### `profile list`

List all profiles.

```bash
modde profile list [--game <id>]
```

| Flag | Description |
|------|-------------|
| `--game` | Filter by game ID |

### `profile create`

Create a new profile.

```bash
modde profile create <name> --game <id>
```

### `profile delete`

Delete a profile.

```bash
modde profile delete <name> [--game <id>]
```

### `profile switch`

Switch to a profile. Automatically swaps saves.

```bash
modde profile switch <name> --game <id>
```

### `profile active`

Show the active profile for a game.

```bash
modde profile active --game <id>
```

### `profile try`

Push a profile onto the experiment stack. Can be stacked multiple times.

```bash
modde profile try <name> --game <id>
```

### `profile rollback`

Pop the experiment stack, returning to the previous profile.

```bash
modde profile rollback --game <id>
```

### `profile commit`

Accept the current experiment and clear the rollback stack.

```bash
modde profile commit --game <id>
```

### `profile fork`

Clone a profile including its mods, load order rules, and saves.

```bash
modde profile fork <source> <name> --game <id> [--unlock]
```

| Flag | Description |
|------|-------------|
| `--unlock` | Strip profile-level lock and all per-mod pins from the fork |

### `profile lock`

Apply a manual load order lock to a profile.

```bash
modde profile lock <name> [--game <id>] [--note <text>]
```

### `profile unlock`

Clear the profile-level load order lock.

```bash
modde profile unlock <name> [--game <id>]
```

### `profile lock-info`

Show lock status and per-mod pins for a profile.

```bash
modde profile lock-info <name> [--game <id>]
```

### `profile lock-mod`

Pin an individual mod in place.

```bash
modde profile lock-mod <name> <mod_id> [--game <id>] [--note <text>]
```

### `profile unlock-mod`

Release a per-mod pin.

```bash
modde profile unlock-mod <name> <mod_id> [--game <id>]
```

### `profile dedup`

Detect filesystem-scanner rows that duplicate Wabbajack manifest entries.

```bash
modde profile dedup <name> [--game <id>] [--manifest <path>] [--apply]
```

| Flag | Description |
|------|-------------|
| `--manifest` | Path to a `.wabbajack` file as the authoritative reference |
| `--apply` | Actually delete leaked rows (without this flag, dry-run only) |

---

## `modde play`

Switch profile, deploy mods, and launch the game.

```bash
modde play [profile] --game <id> [--no-deploy] [--no-switch] [--no-capture]
```

| Flag | Description |
|------|-------------|
| `--no-deploy` | Skip mod deployment |
| `--no-switch` | Skip profile switch |
| `--no-capture` | Skip save auto-capture after game exit |

---

## `modde deploy`

Deploy mods for the active or specified profile.

```bash
modde deploy [--profile <name>] [--game <id>]
```

---

## `modde rollback`

Rollback to the previous deployment.

```bash
modde rollback [--profile <name>] [--game <id>]
```

---

## `modde install`

Install mods from various sources.

### `install wabbajack`

Install from a Wabbajack modlist.

```bash
modde install wabbajack <path> [--profile <name>] [--game-dir <path>] [--force]
```

| Flag | Description |
|------|-------------|
| `--profile` | Target profile (created if it doesn't exist) |
| `--game-dir` | Game installation directory |
| `--force` | Force full reinstall, skipping preflight checks |

### `install nexus-collection`

Install a Nexus Collection.

```bash
modde install nexus-collection <slug> [--version <rev>] [--profile <name>]
```

### `install mod`

Install a single mod from Nexus.

```bash
modde install mod <url> [--profile <name>] [--fomod-config <path>]
```

| Flag | Description |
|------|-------------|
| `--fomod-config` | Path to a FOMOD declarative config (TOML or JSON) for non-interactive installation |

---

## `modde mod`

Manage individual mods.

### `mod remove`

Remove an installed mod from a profile and unlink its staged files.

```bash
modde mod remove <mod_id> [--profile <name>]
```

### `mod diagnose`

Print the dossier path and prompt for a mod whose install type could not be detected.

```bash
modde mod diagnose <mod_id>
```

---

## `modde nexus`

Nexus Mods account management.

### `nexus auth`

Save your API key to the system keyring.

```bash
modde nexus auth
```

### `nexus status`

Show API key validity and premium status.

```bash
modde nexus status
```

---

## `modde nxm`

Handle `nxm://` download links from Nexus Mods.

### `nxm handle`

Handle an `nxm://` download URI.

```bash
modde nxm handle <uri> [--profile <name>]
```

### `nxm install`

Install the `nxm://` URI handler for your desktop environment.

```bash
modde nxm install
```

---

## `modde save`

Manage save files and the git-backed save vault.

### `save assign`

Assign a save file to a profile.

```bash
modde save assign <path> --profile <name> [--game <id>] [--label <text>]
```

### `save unassign`

Remove a save assignment.

```bash
modde save unassign <path>
```

### `save list`

List saves for a profile.

```bash
modde save list --profile <name> [--game <id>]
```

### `save scan`

Scan for unassigned save files.

```bash
modde save scan --game <id>
```

### `save adopt`

Import existing saves from the game directory into a profile's vault.

```bash
modde save adopt --game <id> --profile <name>
```

### `save capture`

Snapshot current saves into the vault.

```bash
modde save capture --game <id> --profile <name> [-m <message>]
```

### `save history`

Show save snapshot history.

```bash
modde save history --game <id> --profile <name> [--limit <n>]
```

### `save restore`

Restore saves from a specific snapshot.

```bash
modde save restore <commit> --game <id> --profile <name>
```

### `save auto-capture`

Detect and capture new saves (called by the launch wrapper on game exit).

```bash
modde save auto-capture --game <id> [--profile <name>]
```

### `save watch`

Watch for save changes and auto-capture via polling.

```bash
modde save watch --game <id> [--profile <name>] [--interval <secs>]
```

Default interval: 30 seconds.

---

## `modde update`

### `update check`

Check for mod updates on Nexus Mods.

```bash
modde update check [--profile <name>] [--game <id>] [--period <period>]
```

Period values: `1d` (1 day), `1w` (1 week, default), `1m` (1 month).

---

## `modde loot`

LOOT masterlist integration for Bethesda plugin sorting.

### `loot sort`

Sort plugins using LOOT masterlist rules.

```bash
modde loot sort --game <id> [--data-dir <path>]
```

### `loot validate`

Validate plugins for Form 43 and missing master errors.

```bash
modde loot validate --game <id>
```

---

## `modde tool`

External tool and overlay management.

### `tool run`

Run an external tool with overwrite capture.

```bash
modde tool run <executable> [--profile <name>] [--game <id>] [-- args...]
```

### `tool list`

List detected tools for a game.

```bash
modde tool list --game <id>
```

### `tool status`

Show status of all gaming tools and overlays.

```bash
modde tool status --game <id>
```

### `tool enable`

Enable a tool or overlay.

```bash
modde tool enable <tool_id> --game <id>
```

Tool IDs: `mangohud`, `vkbasalt`, `gamemode`, `reshade`, `optiscaler`.

### `tool disable`

Disable a tool or overlay.

```bash
modde tool disable <tool_id> --game <id>
```

### `tool configure`

Configure tool settings.

```bash
modde tool configure <tool_id> --game <id> <key=value...>
```

### `tool apply`

Apply tool patches to the game directory (DLLs, configs).

```bash
modde tool apply <tool_id> --game <id>
```

### `tool revert`

Revert tool patches from the game directory.

```bash
modde tool revert <tool_id> --game <id>
```

---

## `modde fomod`

FOMOD installer utilities.

### `fomod generate`

Generate a declarative FOMOD config template from a mod's `ModuleConfig.xml`.

```bash
modde fomod generate <mod_path> [--all] [--format <fmt>]
```

| Flag | Description |
|------|-------------|
| `--all` | Include all plugins (not just defaults) |
| `--format` | Output format: `toml` (default), `json`, or `nix` |

### `fomod apply`

Apply a declarative FOMOD config non-interactively.

```bash
modde fomod apply <mod_path> --config <path> --dest <dir>
```

### `fomod inspect`

Inspect a mod's FOMOD steps, groups, and plugins.

```bash
modde fomod inspect <mod_path>
```

---

## `modde scan`

Scan a game directory for installed mods.

```bash
modde scan --game <id> [--game-dir <path>] [--manifest <path>] [--import-to <profile>] [--threshold <0.0-1.0>] [--dry-run] [--prune-duplicates]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--game-dir` | auto-detected | Game installation path |
| `--manifest` | — | `.wabbajack` file for manifest matching |
| `--import-to` | — | Import discovered mods into this profile |
| `--threshold` | `0.5` | Minimum file presence fraction for a match |
| `--dry-run` | — | Report only, don't write to database |
| `--prune-duplicates` | — | Remove filesystem-scanner rows covered by the manifest (requires `--manifest` and `--import-to`) |

---

## `modde collisions`

Analyse mod file collisions.

```bash
modde collisions [--profile <name>] [--game <id>] [--all] [--suggest-hides]
```

| Flag | Description |
|------|-------------|
| `--all` | Show all collisions including cosmetic ones |
| `--suggest-hides` | Suggest hide commands for redundant files |

---

## `modde diagnostics`

Run diagnostic checks for common modding issues.

```bash
modde diagnostics --game <id> [--profile <name>]
```

---

## `modde export`

Export a profile's mod list to CSV.

```bash
modde export [--profile <name>] [--game <id>] [--columns <col,col,...>] [--output <path>]
```

Outputs to stdout if `--output` is omitted.

---

## `modde backup`

Manage mod and plugin order backups.

### `backup create`

Create a backup of a mod's staged files.

```bash
modde backup create <mod_id>
```

### `backup restore`

Restore a mod from its latest backup.

```bash
modde backup restore <mod_id>
```

### `backup list`

List available backups for a mod.

```bash
modde backup list <mod_id>
```

### `backup plugins`

Backup the current plugin load order.

```bash
modde backup plugins --profile <name> --game <id>
```

### `backup restore-plugins`

Restore plugin load order from a backup.

```bash
modde backup restore-plugins --profile <name> --game <id>
```

---

## `modde stock`

Manage vanilla game snapshots.

### `stock snapshot`

Capture a snapshot of the vanilla game installation.

```bash
modde stock snapshot <game_id>
```

### `stock verify`

Verify snapshot integrity against the current installation.

```bash
modde stock verify <game_id>
```

---

## `modde verify`

Verify installed file integrity for a profile.

```bash
modde verify [--profile <name>] [--game <id>]
```

---

## `modde detect`

Detect installed games across Steam and Heroic launchers.

```bash
modde detect
```

---

## `modde instance`

Manage modde instances (multiple data directories).

### `instance create`

Create a new instance.

```bash
modde instance create <name> --data-dir <path>
```

### `instance list`

List all instances.

```bash
modde instance list
```

### `instance switch`

Switch to a different instance.

```bash
modde instance switch <name>
```

---

## `modde import`

Import existing TOML-format profiles into the database.

```bash
modde import
```

---

## `modde gui`

Launch the graphical user interface.

```bash
modde gui
```
