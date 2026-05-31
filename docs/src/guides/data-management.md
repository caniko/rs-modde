# Data, instances & backups

## Overview

modde keeps all of its state in a single **data directory** plus a small config
file. This guide documents the on-disk layout, how to point modde at a different
location, how to run several isolated **instances**, the built-in **backup** and
**stock snapshot** facilities, and how to reproduce your setup across machines.

Everything here is filesystem and metadata; the conceptual model of a profile
itself lives in [Profile management](profiles.md), and the save vaults referenced
below are covered in [Save management](saves.md).

## Where modde stores data

modde follows the platform's standard base directories. On Linux that means the
XDG specification, with the usual macOS and Windows fallbacks:

| Purpose | Linux | macOS | Windows |
| ------- | ----- | ----- | ------- |
| Data (`modde_data`) | `$XDG_DATA_HOME` or `~/.local/share` | `~/Library/Application Support` | `%APPDATA%` |
| Config | `$XDG_CONFIG_HOME` or `~/.config` | `~/Library/Application Support` | `%APPDATA%` |
| Cache | `$XDG_CACHE_HOME` or `~/.cache` | `~/Library/Caches` | `%LOCALAPPDATA%` |

On Linux, an explicitly-set `XDG_DATA_HOME` / `XDG_CONFIG_HOME` / `XDG_CACHE_HOME`
is honored before the `~/.local/share`-style fallback.

### The data directory layout

All persistent state lives under `<data_dir>/modde/` — for a default Linux setup,
`~/.local/share/modde/`:

```text
~/.local/share/modde/
├── modde.db            # SQLite: profiles, mods, load order, locks,
│                       #   saves index, categories, tools, executables,
│                       #   stock-snapshot metadata, experiment stack
├── store/              # content-addressed mod file store (the staged bytes)
├── staging/            # scratch space used while installing/extracting
├── profiles/           # per-profile on-disk dirs: <name>/overrides, <name>/staging,
│                       #   <name>/ini (per-profile game INIs), etc.
├── downloads/          # downloaded mod archives
├── stock/              # vanilla game snapshots (one subdir per game_id)
├── saves/              # git-backed save vaults (one repo per game_id)
├── wabbajack_cache/    # content-addressed cache of .wabbajack manifest files
└── backups/            # zip mod backups + JSON plugin-order backups
```

A few of these are worth calling out:

- **`modde.db`** is the source of truth for everything *except* file bytes and
  git history. The profile, mod list, load order, locks, categories, tags,
  per-game tool config, named executables, and the experiment stack all live
  here. Its schema is migrated forward automatically on open.
- **`store/`** holds the actual mod files content-addressed; the database's
  `installed_mod_files` manifest maps each profile's mods to the precise files
  they staged, which is what makes uninstall surgical.
- **`saves/<game_id>/`** is a real git repository per game, with one branch per
  profile. See [Save management](saves.md) for the vault mechanics.
- **`wabbajack_cache/<manifest_hash>.wabbajack`** caches manifest files keyed by
  their hash, so re-installs and `dedup` runs don't re-download the list.

Non-essential cache (as opposed to data) lives separately under
`<cache_dir>/modde/` — for example `~/.cache/modde/` on Linux.

The config directory holds modde's small configuration state, including the
instance registry:

```text
~/.config/modde/
└── instances.toml      # the instance registry (see below)
```

### `MODDE_DATA_DIR`

You can override the data directory wholesale with the `MODDE_DATA_DIR`
environment variable, exposed as a global CLI flag:

```bash
# Point this invocation at a portable data directory
modde --data-dir /mnt/usb/modde-portable profile list

# Equivalent via the environment
MODDE_DATA_DIR=/mnt/usb/modde-portable modde profile list
```

When set, this path becomes `<modde_data>` directly — `modde.db`, `store/`,
`profiles/`, `saves/`, and all the rest are created underneath it. The override is
read once at startup and wins over both the default location and any active
instance. This is the simplest way to keep modde's entire footprint on an external
drive or a per-project directory.

> Precedence: an explicit `--data-dir` / `MODDE_DATA_DIR` override beats the
> **active instance**, which in turn beats the default `<data_dir>/modde/`. If you
> use both instances and the override, the override wins for that invocation.

## Multiple instances

An **instance** is a named, self-contained data directory. Instances let you keep
entirely separate modde states — for example one for your daily driver and one for
testing a risky Wabbajack list — without them sharing a database, store, or save
vaults. The active instance is recorded in `~/.config/modde/instances.toml`.

```bash
# Create a new instance backed by its own data directory
modde instance create testing --data-dir ~/modde-instances/testing

# List instances (the active one is marked)
modde instance list

# Switch the active instance
modde instance switch testing
```

The registry file is plain TOML and easy to inspect:

```toml
active = "testing"

[[instances]]
name = "default"
data_dir = "/home/you/.local/share/modde"
is_default = true

[[instances]]
name = "testing"
data_dir = "/home/you/modde-instances/testing"
is_default = false
```

Behavior worth knowing:

- The **first** instance you create becomes the default (`is_default = true`), and
  if no instance was active it is made active automatically.
- Creating an instance creates its `data_dir` for you. Creating one whose name
  already exists is an error.
- Switching to a name that doesn't exist is an error; it never silently creates.
- Once an instance is active, `modde_data` resolves to that instance's `data_dir`
  for every command — unless you override it with `MODDE_DATA_DIR` / `--data-dir`
  for a single invocation (see the precedence note above).

Because each instance is just a directory, you can back one up, move it to another
disk, or delete it by operating on the `data_dir` path directly (and editing
`instances.toml` to drop the entry).

## Backups

modde keeps two kinds of explicit backup under `<modde_data>/backups/`, separate
from the always-on git save vaults.

### Mod backups (zip)

Snapshot a single mod's staged directory into a timestamped zip, then restore the
latest one on demand:

```bash
# Zip up a mod's current files
modde backup create <mod_id>

# List a mod's backups (oldest first)
modde backup list <mod_id>

# Restore the most recent backup for a mod
modde backup restore <mod_id>
```

Backups are stored at `backups/mods/<mod_id>/<mod_id>_<timestamp>.zip`. `restore`
extracts the newest zip for that mod; `list` is sorted oldest-first, so the last
entry is the one `restore` will pick.

### Plugin-order backups (JSON)

The plugin load order (which `.esp`/`.esm`/`.esl` files load, in what order, and
whether each is enabled) can be snapshotted per profile and game:

```bash
# Save the current plugin order
modde backup plugins --profile my-skyrim --game skyrim-se

# Restore the most recent plugin-order backup
modde backup restore-plugins --profile my-skyrim --game skyrim-se
```

These land at `backups/plugins/<game_id>/<profile>_<timestamp>.json` and record
each plugin's name **and** its enabled state, so a restore brings back exactly
what was loaded. Older, name-only backups are still readable — on restore they are
treated as all-enabled — so historical backups keep working across upgrades.

## Stock (vanilla) game snapshots

A **stock snapshot** is a preserved copy of a clean, vanilla game install. modde
uses it as the known-good baseline for deployment and verification, so it can tell
whether the game directory has drifted from vanilla. Snapshots are stored under
`<modde_data>/stock/<game_id>/`.

```bash
# Snapshot the detected vanilla install for a game
modde stock snapshot skyrim-se

# Verify a snapshot still matches what was recorded
modde stock verify skyrim-se
```

How it works:

- The snapshot is built by **hardlinking** each file from the detected install
  into the snapshot directory (falling back to a copy when source and destination
  are on different filesystems). Hardlinks make the snapshot cheap in space — it
  shares bytes with the original install until either side changes.
- A deterministic **tree hash** is computed over the snapshot: every file's
  relative path is paired with its `xxh3-64` content hash, the pairs are sorted by
  path, and the whole concatenation is hashed again. The result and the file count
  are written both into a `.modde-tree-hash.toml` file inside the snapshot and
  into the `stock_snapshots` table in `modde.db`.
- `modde stock verify` recomputes the tree hash and compares it against the
  recorded one. A match prints `OK`; a mismatch prints `MISMATCH` and recommends
  re-snapshotting, because something changed the snapshot's contents (an updated
  game, a stray write, link breakage).

The detector locates the install via the game plugin (Steam library folders, etc.,
as resolved by modde's path layer). If it can't find a vanilla install, the
snapshot command reports that the game wasn't detected rather than snapshotting a
modified directory.

## Multi-machine sync

modde has no built-in cloud sync, but its on-disk layout is designed so you can
reproduce a setup across machines with tools you already use. The reliable pattern
is: **declare profiles reproducibly, and version-control the save vaults.**

### Reproduce profiles with home-manager

The most robust way to get the *same* profiles on another machine is to declare
them in home-manager rather than copy the database. Your `programs.modde.profiles`
definitions are the source of truth; rebuilding on the second machine recreates
the profiles, Wabbajack lists, and tool overlays from the same inputs:

```nix
programs.modde.profiles.my-skyrim = {
  game = "skyrim-se";
  wabbajackList = {
    url = "https://example.com/modlist.wabbajack";
    hash = "sha256-...";
  };
};
```

Because the Wabbajack list is pinned by hash and the home-manager module is the
same flake input on both machines, the resulting profile is reproducible rather
than copied. See [Installation](../getting-started/installation.md) and the
[Home-Manager module reference](../configuration/hm-module.md).

If you maintain profiles imperatively instead, you can move them between machines
as TOML: drop exported `profile.toml` files into the destination's
`<modde_data>/profiles/<name>/` directories and run `modde import`, which scans the
profiles directory and loads any TOML profiles not already in the database.
Imported profiles are lock-stamped with `TomlImport` provenance (preserving any
pre-existing lock), so a shared, authoritative ordering survives the trip — see
[Load order locking](profiles.md#load-order-locking).

### Git the save vaults

Each game's save vault under `<modde_data>/saves/<game_id>/` is already a git
repository, which makes it the natural unit to synchronize. Add a remote and push
it like any other repo:

```bash
cd ~/.local/share/modde/saves/skyrim-se
git remote add origin git@example.com:you/skyrim-saves.git
git push -u origin --all
```

On the second machine, clone (or pull) into the same path and modde will pick up
the per-profile branches and history. This carries your actual save snapshots —
including the mod fingerprints embedded in each commit — so the compatibility
warnings on restore keep working across machines. The save-vault mechanics are
documented in [Save management](saves.md).

### What not to sync directly

- **`modde.db`** is machine-local truth and races badly if two machines write it
  concurrently — prefer reproducing profiles (home-manager or TOML import) over
  copying the database.
- **`store/`, `downloads/`, `staging/`, `wabbajack_cache/`** are large, derivable
  caches. Re-downloading (or re-deploying from a reproduced profile) is safer than
  rsyncing gigabytes of content-addressed blobs.
- **`stock/`** snapshots are hardlink trees tied to a specific install path on a
  specific machine; re-run `modde stock snapshot` on each machine instead.

## See also

- [Profile management](profiles.md) — what lives in `modde.db`, locks, the experiment stack, and `modde import`
- [Save management](saves.md) — the git-backed vaults under `saves/` and how fingerprints ride along
- [Deployment](deployment.md) — how the store and stock snapshot feed the game directory
- [Wabbajack lists](wabbajack.md) — the manifests cached under `wabbajack_cache/`
- [Installation](../getting-started/installation.md) — flake and home-manager setup for reproducible profiles
- [Home-Manager module reference](../configuration/hm-module.md) — declaring profiles in Nix for multi-machine reproduction
