# CLI Reference

This is the exhaustive reference for the `modde` command-line interface. Every
top-level command group, every subcommand, every flag, and the environment
variables modde reads are documented here. The binary is invoked as `modde`
(installed via the [Nix flake](../getting-started/installation.md)); the GUI is
the same binary under `modde gui`.

Run `modde --help` or `modde <command> --help` at any time for the
clap-generated usage of a specific command.

## Global flags

These flags are accepted on every command (clap `global = true`).

| Flag                | Environment          | Description                                                                          |
| ------------------- | -------------------- | ------------------------------------------------------------------------------------ |
| `--data-dir <path>` | `MODDE_DATA_DIR`     | Override the data directory for a single invocation (default: `~/.local/share/modde`) |
| `--heap-profile <path>` | `MODDE_HEAP_PROFILE` | **Feature-gated.** Write a DHAT heap profile. Requires building with `--features heap-profile`; errors out otherwise |
| `--debug-panic`     | —                    | **Feature-gated, hidden.** Panic after startup to smoke-test the `remote-telemetry` crash-capture path. Only compiled in with `--features remote-telemetry` |

`--data-dir` overrides the active instance (see [`modde instance`](#modde-instance))
for that one command only — it does not change the persisted active instance. The
release builds shipped through the flake do **not** enable `heap-profile` or
`remote-telemetry`, so `--heap-profile` returns an explanatory error and
`--debug-panic` is absent unless you build those features yourself.

## Environment variables

modde reads the following environment variables in addition to the `--data-dir`
override above.

| Variable                 | Default     | Purpose                                                                                                       |
| ------------------------ | ----------- | ------------------------------------------------------------------------------------------------------------ |
| `MODDE_DATA_DIR`         | `~/.local/share/modde` | Data directory (DB, store, downloads, backups). Equivalent to `--data-dir`.                       |
| `NEXUS_API_KEY`          | —           | Nexus Mods API key, used when no modde-owned config-file key is present.                                      |
| `NEXUS_API_KEY_FILE`     | —           | Path to a file containing the Nexus API key (read after the keyring is consulted).                            |
| `GITHUB_TOKEN`           | —           | Bearer token for the GitHub API; raises rate limits for release-backed tools (`reshade`, `optiscaler`).       |
| `RUST_LOG`               | —           | `tracing`/`env_logger`-style filter controlling log verbosity (e.g. `RUST_LOG=modde=debug`).                  |
| `MODDE_BYTE_CACHE_MIB`   | `512`       | Size (MiB) of the in-memory LRU byte cache used by the download/source layer.                                 |
| `MODDE_ZSTD_MIN_BYTES`   | `1048576`   | Minimum file size (bytes) before Wabbajack staging entries are zstd-compressed. Default 1 MiB.                |
| `MODDE_ZSTD_LEVEL`       | `9`         | zstd compression level for staging (clamped to the 1–22 range).                                              |
| `MODDE_ARCHIVE_RETENTION`| `keep`      | Default source-archive retention after a Wabbajack archive batch integrates. Accepts `keep`, `prune-applied` (aliases `prune`/`delete`), or `auto`. The `--archive-retention` flag on `install wabbajack` overrides this. |
| `MODDE_HEAP_PROFILE`     | —           | Same as the `--heap-profile` flag; only meaningful in a `heap-profile` build.                                 |
| `MODDE_DATABASE_BACKEND`  | `sqlite`    | Database backend override. Accepts `sqlite`, `postgres`, `postgresql`, or `pg`.                              |
| `MODDE_DATABASE_URL`      | —           | PostgreSQL connection URL. Wins over the discrete PostgreSQL fields.                                          |
| `MODDE_DATABASE_HOST`     | —           | PostgreSQL host when no URL is set.                                                                          |
| `MODDE_DATABASE_PORT`     | `5432`      | PostgreSQL port when no URL is set.                                                                          |
| `MODDE_DATABASE_NAME`     | —           | PostgreSQL database name when no URL is set.                                                                 |
| `MODDE_DATABASE_USER`     | backend default | PostgreSQL user when no URL is set.                                                                     |
| `MODDE_DB_PASSWORD_FILE`  | —           | Path to a file containing the PostgreSQL password; the password contents are read at runtime.                 |

### Nexus API key resolution order

When a command needs a Nexus key, modde resolves it in this order and uses the
first source that yields a non-empty value:

1. The modde-owned config file (written by [`modde nexus auth`](#nexus-auth)).
2. `NEXUS_API_KEY`.
3. The system keyring.
4. `NEXUS_API_KEY_FILE`.
5. A legacy key stored in app settings.

An OAuth token (if present and unexpired) takes precedence over the API-key
chain. Under Home Manager, prefer `programs.modde.nexus.apiKeyFile`, which feeds
`NEXUS_API_KEY_FILE`.

---

## `modde config`

Inspect, validate, and update modde configuration. Database resolution uses the
same precedence as runtime startup: environment variables first, then
`settings.toml`, then SQLite defaults. For PostgreSQL, `MODDE_DATABASE_URL` or
`database.url` wins over the discrete host/port/name/user fields.

### `config show`

Show the resolved database backend, source of each displayed field, and config
file path. URL passwords are redacted before printing.

```bash
modde config show
```

For SQLite, the command prints the selected backend and local SQLite path. For
PostgreSQL, it prints either the redacted URL plus resolved connection summary,
or the resolved discrete fields, plus the configured password-file path if one
is set.

### `config set-database`

Set the stored database backend and PostgreSQL connection fields in
`settings.toml`.

```bash
modde config set-database --backend sqlite
modde config set-database --backend postgres --url postgres://modde@localhost/modde \
  --password-file /run/secrets/modde-postgres-password
modde config set-database --backend postgres --host localhost --port 5432 \
  --name modde --user modde --password-file /run/secrets/modde-postgres-password
```

| Flag                 | Description                                                       |
| -------------------- | ----------------------------------------------------------------- |
| `--backend <name>`   | Required. `sqlite` or `postgres`; aliases include `postgresql` and `pg` |
| `--url <url>`        | Full PostgreSQL connection URL; overrides discrete fields at runtime |
| `--host <host>`      | PostgreSQL host when no URL is set                                |
| `--port <port>`      | PostgreSQL port when no URL is set                                |
| `--name <name>`      | PostgreSQL database name when no URL is set                       |
| `--user <user>`      | PostgreSQL user when no URL is set                                |
| `--password-file <path>` | Path to the PostgreSQL password file; contents are not stored |
| `--clear <field>`    | Clear a stored PostgreSQL field from `settings.toml`               |

`--clear` accepts `url`, `host`, `port`, `dbname`, `user`, and
`password-file`. Passing an empty string to a string/path field also clears that
field. Setting `--backend sqlite` clears all stored PostgreSQL fields.

When `backend = postgres`, the stored configuration must provide either `--url`
or at least `--name`. URL and discrete fields are mutually exclusive in stored
settings. Environment variables can still override the stored values at runtime.

### `config reset-database`

Reset stored database settings to SQLite and clear all stored PostgreSQL
connection fields.

```bash
modde config reset-database
```

This updates `settings.toml`; it does not unset environment variables already
present in the current process.

### `config test`

Open the resolved database, run a trivial probe query, and print `OK` on
success.

```bash
modde config test
```

This command uses the same environment/settings/default resolution as normal
startup, so it is the quickest way to validate a PostgreSQL URL, discrete
connection fields, or `MODDE_DB_PASSWORD_FILE` path.

---

## `modde profile`

Manage mod profiles.

### `profile list`

List all profiles.

```bash
modde profile list [--game <id>]
```

| Flag     | Description       |
| -------- | ----------------- |
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

| Flag       | Description                                                 |
| ---------- | ----------------------------------------------------------- |
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

| Flag         | Description                                                   |
| ------------ | ------------------------------------------------------------- |
| `--manifest` | Path to a `.wabbajack` file as the authoritative reference    |
| `--apply`    | Actually delete leaked rows (without this flag, dry-run only) |

Without `--manifest`, this runs the layer-1 heuristic: it lists filesystem-scanner
rows (`cet/*`, `reds/*`, `tweak/*`, `archive/*`, `redmod/*`) on a locked profile as
read-only "suspects". With `--manifest`, the manifest's install directives classify
each suspect as LEAKED (safe to delete) or GENUINE (a user addition to keep), and
`--apply` deletes the LEAKED rows.

---

## `modde play`

Switch profile, deploy mods, and launch the game.

```bash
modde play [profile] --game <id> [--no-deploy] [--no-switch] [--no-capture]
```

| Flag           | Description                            |
| -------------- | -------------------------------------- |
| `--no-deploy`  | Skip mod deployment                    |
| `--no-switch`  | Skip profile switch                    |
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

Install from a Wabbajack modlist. This is the largest single command in the CLI;
the defaults below match a normal full install.

```bash
modde install wabbajack <path> [--profile <name>] [--game-dir <path>] [flags...]
```

| Flag                       | Default  | Description                                                                                       |
| -------------------------- | -------- | ------------------------------------------------------------------------------------------------ |
| `--profile`                | —        | Target profile (created if it doesn't exist)                                                     |
| `--game-dir`               | —        | Game installation directory to deploy mods into                                                  |
| `--force`                  | `false`  | Force full reinstall, skipping preflight checks                                                  |
| `--no-deploy`              | `false`  | Stage into modde's data dir but skip the final copy into `--game-dir` (Stock-Game lists)         |
| `--continue-on-error`      | `false`  | Log per-archive failures instead of aborting; install proceeds as far as possible               |
| `--reset-staging`          | `false`  | Explicitly discard existing Wabbajack staging before installing                                 |
| `--skip-validate`          | `false`  | Skip staging validation before deploy                                                           |
| `--diagnostics-dir`        | —        | Write apply diagnostics JSONL to this directory (consumed by `wabbajack analyze-diagnostics`)    |
| `--diagnostics-interval`   | `30`     | Diagnostics heartbeat interval in seconds                                                        |
| `--stall-warn-seconds`     | `600`    | Warn when apply makes no batch/sentinel progress for this many seconds                           |
| `--stall-abort-seconds`    | `1800`   | Abort when stalled this long and cgroup memory/swap are saturated                                |
| `--archive-retention`      | `keep`   | Source-archive retention after successful integration: `keep`, `prune-applied`, or `auto`        |
| `--missing-archive-policy` | `fail`   | Behavior for missing optional manual/Nexus archives: `fail`, `omit-files`, or `omit-mods`        |
| `--acquire-missing`        | `false`  | Frontload assisted manual-archive acquisition before applying                                    |
| `--acquire-download-dir`   | —        | Browser download directory to watch during assisted acquisition                                 |
| `--acquire-timeout`        | `900`    | Per-archive assisted acquisition timeout in seconds                                             |
| `--acquire-include-nexus`  | `false`  | Include Nexus archives in frontloaded acquisition                                               |
| `--acquire-browser-controller` | `false` | Use controlled Chromium tabs for frontloaded acquisition                                     |
| `--no-acquire-missing`     | `false`  | Disable automatic frontloaded manual-archive acquisition                                        |

```bash
# Smoke-run a large list without writing into the live game install
modde install wabbajack ./lotf.wabbajack --profile lotf \
  --no-deploy --continue-on-error --skip-validate \
  --diagnostics-dir ./wj-diag --missing-archive-policy omit-mods
```

See the [Wabbajack guide](../guides/wabbajack.md) for the full readiness workflow.

### `install nexus-collection`

Install a Nexus Collection.

```bash
modde install nexus-collection <slug> [--version <rev>] [--profile <name>]
```

| Flag        | Description                                  |
| ----------- | -------------------------------------------- |
| `--version` | Pin a specific collection revision           |
| `--profile` | Target profile (created if it doesn't exist) |

### `install mod`

Install a single mod from Nexus.

```bash
modde install mod <url> [--profile <name>] [--fomod-config <path>]
```

| Flag             | Description                                                                        |
| ---------------- | ---------------------------------------------------------------------------------- |
| `--fomod-config` | Path to a FOMOD declarative config (TOML or JSON) for non-interactive installation |

---

## `modde mod`

Manage individual mods.

### `mod remove`

Remove an installed mod from a profile and unlink its staged files. Removal uses
the V8 `installed_mod_files` manifest, so it is precise — no orphaned files, no
collateral damage.

```bash
modde mod remove <mod_id> [--profile <name>]
```

| Flag        | Description                                                            |
| ----------- | --------------------------------------------------------------------- |
| `--profile` | Profile to remove from. Defaults to the active / unambiguous profile. |

The `mod_id` is the id stored in the profile — usually
`<domain>_<mod_id>_<file_id>` for Nexus installs.

### `mod diagnose`

Print the skill-dossier path and an inline prompt for a mod whose install type
could not be detected. Handy for piping into an agent or pasting into a chat.

```bash
modde mod diagnose <mod_id>
```

---

## `modde wabbajack`

Search, download, inspect, prepare, and support Wabbajack modlists. These are the
read-only and supporting utilities; the actual install runs through
[`modde install wabbajack`](#install-wabbajack).

### `wabbajack search`

Search public Wabbajack modlist catalogs.

```bash
modde wabbajack search [query] [--game <id>] [--source official|authored|both] [--json]
```

| Flag       | Default | Description                                        |
| ---------- | ------- | -------------------------------------------------- |
| `--game`   | —       | Filter by game ID                                  |
| `--source` | `both`  | Catalog source: `official`, `authored`, or `both`  |
| `--json`   | —       | Emit machine-readable JSON                          |

### `wabbajack download`

Download a `.wabbajack` file by URL, machine URL, or title. Authored-files URLs
are downloaded through modde's chunk-aware downloader when required.

```bash
modde wabbajack download <url-or-machine-url> [--output <dir>]
```

| Flag       | Description                          |
| ---------- | ------------------------------------ |
| `--output` | Output directory for the downloaded file |

### `wabbajack hm-snippet`

Generate a Home Manager profile snippet for a `.wabbajack` source.

```bash
modde wabbajack hm-snippet <url-or-file> --profile <name> --game <id> [--game-dir <path>] [--output <path>]
```

| Flag         | Description                                       |
| ------------ | ------------------------------------------------- |
| `--profile`  | Profile name to emit in the snippet (required)    |
| `--game`     | Game ID to emit in the snippet (required)         |
| `--game-dir` | Game install directory to emit                    |
| `--output`   | Write the snippet to a file instead of stdout     |

### `wabbajack import-archive`

Import local archives into the modde store by matching the Wabbajack manifest
hash. Useful when an upstream authored-files archive disappeared but you still
have the exact archive in an old Wabbajack cache.

```bash
modde wabbajack import-archive <path.wabbajack> <archive-path>...
```

The command hashes each archive and imports only files whose Wabbajack hash is
referenced by the manifest. Matching by filename is intentionally refused — only
exact hash matches are accepted.

### `wabbajack acquire-missing`

Open manual Wabbajack archive pages and import matching browser downloads.

```bash
modde wabbajack acquire-missing <manifest> [flags...]
```

| Flag                   | Default | Description                                                       |
| ---------------------- | ------- | ----------------------------------------------------------------- |
| `--download-dir`       | —       | Browser download directory to watch                               |
| `--data-dir`           | —       | Override the data directory for this acquisition                  |
| `--browser-profile`    | —       | Browser profile directory to use                                  |
| `--include-nexus`      | `false` | Include Nexus archives in acquisition                             |
| `--browser-controller` | `false` | Drive controlled Chromium tabs instead of just watching downloads |
| `--timeout`            | `900`   | Per-archive acquisition timeout in seconds                        |
| `--json`               | `false` | Emit machine-readable JSON                                        |

### `wabbajack missing-impact`

Report missing manual/Nexus archives and their install impact (how many files /
mods would be omitted).

```bash
modde wabbajack missing-impact <manifest> [--data-dir <path>] [--json] [--nix-snippet]
```

| Flag           | Description                                                         |
| -------------- | ------------------------------------------------------------------ |
| `--data-dir`   | Override the data directory                                        |
| `--json`       | Emit machine-readable JSON                                         |
| `--nix-snippet`| Print Home Manager `manualArchives` entries for the missing archives |

### `wabbajack manual-links`

Print missing manual-archive URLs that require an operator to visit each page.

```bash
modde wabbajack manual-links <manifest> [--data-dir <path>] [--json]
```

### `wabbajack assess`

Assess readiness for a large Wabbajack install without mutating any state.

```bash
modde wabbajack assess <manifest> [--profile <name>] [--game-dir <path>] [--json]
```

| Flag         | Description                                |
| ------------ | ------------------------------------------ |
| `--profile`  | Profile context for the assessment         |
| `--game-dir` | Game install directory to assess against   |
| `--json`     | Emit machine-readable JSON                  |

### `wabbajack analyze-diagnostics`

Summarize the Wabbajack diagnostics JSONL written by a previous install run (the
directory passed to `--diagnostics-dir`).

```bash
modde wabbajack analyze-diagnostics <diagnostics-dir> [--json]
```

```bash
# Assess, resolve missing archives, then analyze the run afterwards
modde wabbajack assess ./lotf.wabbajack --game-dir ~/Games/skyrim
modde wabbajack missing-impact ./lotf.wabbajack --nix-snippet
modde wabbajack analyze-diagnostics ./wj-diag
```

---

## `modde game`

Manage user-defined games. Built-in games are resolved from the internal registry;
these commands register and inspect generic / user-defined games via a GameSpec
TOML. (User-defined game support is **Partial** — deployment, conflict analysis,
and launchers work, but there is no bespoke per-game scanner or save tracker. See
the [generic games guide](../games/generic-games.md).)

### `game add`

Add or overwrite a user-defined game registration.

```bash
modde game add <id> --display-name <name> --executable-dir <path> [flags...]
```

| Flag                 | Description                                                          |
| -------------------- | ------------------------------------------------------------------- |
| `--display-name`     | Human-readable name (required)                                      |
| `--executable-dir`   | Directory containing the game executable, relative to the install (required) |
| `--steam-app-id`     | Steam App ID, for launcher detection                                |
| `--install-dir-name` | Steam `steamapps/common` directory name                             |
| `--mod-dir`          | Mod deployment subdirectory                                         |
| `--nexus-domain`     | Nexus Mods domain slug for this game                                |
| `--proxy-dll <name>` | Proxy DLL name (repeatable) used by OptiScaler/ReShade-style hooks  |
| `--force`            | Overwrite an existing user-defined game TOML                        |

```bash
modde game add stalker2 \
  --display-name "S.T.A.L.K.E.R. 2" \
  --executable-dir "Stalker2/Binaries/Win64" \
  --steam-app-id 1643320 \
  --nexus-domain stalker2heartofchornobyl \
  --proxy-dll dinput8.dll
```

### `game list`

List user-defined games.

```bash
modde game list
```

### `game remove`

Remove a user-defined game registration.

```bash
modde game remove <id> [--yes]
```

| Flag    | Description                  |
| ------- | ---------------------------- |
| `--yes` | Skip the confirmation prompt |

### `game detect`

Detect executable-bearing directories under a game install (helps you pick the
right `--executable-dir` for `game add`).

```bash
modde game detect <install-path>
```

### `game show`

Show a resolved game registration (user-defined first, then the built-in registry).

```bash
modde game show <id>
```

### `game export`

Export a game registration to TOML.

```bash
modde game export <id> [--with-optiscaler] [--output <path>]
```

| Flag                | Description                                                |
| ------------------- | ---------------------------------------------------------- |
| `--with-optiscaler` | Append resolved OptiScaler profiles to the exported TOML   |
| `--output`          | Write to a file instead of stdout                          |

### `game import`

Import a game registration from TOML.

```bash
modde game import <path> [--force]
```

| Flag      | Description                              |
| --------- | ---------------------------------------- |
| `--force` | Overwrite an existing destination TOML   |

### `game import-profile`

Import OptiScaler profiles from TOML for a game.

```bash
modde game import-profile <path> --for <game-id> [--force]
```

| Flag      | Description                              |
| --------- | ---------------------------------------- |
| `--for`   | Game ID the profiles apply to (required) |
| `--force` | Overwrite an existing destination TOML   |

---

## `modde exec`

Manage named launch targets (`xEdit`, `BodySlide`, `FNIS`, etc.). This is a thin
alias for the executable subset of [`modde tool`](#modde-tool) — the underlying
storage is shared, so `modde exec list` and `modde tool list-executables` print
the same rows. Executable management is **Done** end to end (named executables
with args, working dir, env, Wine DLL overrides, a configurable output mod, and
overwrite capture). See the [executables guide](../guides/executables.md).

### `exec add`

Save (or update) a named launch target. Re-running with the same name overwrites
the existing entry, so `add` doubles as `edit`.

```bash
modde exec add <name> <executable> --game <id> [flags...] [-- <args>...]
```

| Flag                   | Default         | Description                                                  |
| ---------------------- | --------------- | ----------------------------------------------------------- |
| `--game`               | —               | Game ID (required)                                          |
| `--working-dir`        | game install    | Working directory for the launched process                 |
| `--output-mod`         | `__overwrite__` | Mod that captures files written during the run              |
| `--wine-dll-overrides` | —               | Wine DLL overrides, e.g. `dinput8=n,b;winmm=n,b`            |
| `--env KEY=VALUE`      | —               | Environment variable assignment (repeatable)               |
| `-- <args>...`         | —               | Default arguments to pass when running this executable     |

```bash
modde exec add xEdit "~/tools/SSEEdit/SSEEdit.exe" --game skyrim-se \
  --wine-dll-overrides "dinput8=n,b" --env "SSE_GAME_PATH=Z:\\game" -- -IKnowWhatImDoing
```

### `exec list`

List configured launch targets for a game.

```bash
modde exec list --game <id>
```

### `exec remove`

Remove a launch target.

```bash
modde exec remove <name> --game <id>
```

### `exec run`

Run a saved launch target with overwrite capture.

```bash
modde exec run <name> --game <id> [--profile <name>] [-- <args>...]
```

| Flag           | Description                                            |
| -------------- | ----------------------------------------------------- |
| `--profile`    | Profile context for the run                           |
| `-- <args>...` | Additional arguments appended after the saved defaults |

---

## `modde nexus`

Nexus Mods account management.

### `nexus auth`

Save your API key to modde's config (and the system keyring where available).

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

Manage save files and the git-backed save vault. See the
[saves guide](../guides/saves.md) for the conceptual model.

### `save assign`

Assign a save file to a profile.

```bash
modde save assign <path> --profile <name> [--game <id>] [--label <text>]
```

| Flag        | Description                  |
| ----------- | ---------------------------- |
| `--profile` | Target profile (required)    |
| `--game`    | Game ID                      |
| `--label`   | Optional label for this save |

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

Import existing saves from the game directory into a profile's vault. If no
profile is active for the game, the adopted profile becomes active.

```bash
modde save adopt --game <id> --profile <name>
```

### `save capture`

Snapshot current saves into the vault (creates a new snapshot).

```bash
modde save capture --game <id> --profile <name> [-m <message>]
```

| Flag             | Description                       |
| ---------------- | --------------------------------- |
| `-m`, `--message`| Optional message for the snapshot |

### `save history`

Show save snapshot history.

```bash
modde save history --game <id> --profile <name> [--limit <n>]
```

| Flag      | Default | Description           |
| --------- | ------- | --------------------- |
| `--limit` | `20`    | Max entries to show   |

### `save restore`

Restore saves from a specific snapshot.

```bash
modde save restore <commit> --game <id> --profile <name>
```

The `<commit>` may be a full commit ID or an unambiguous prefix.

### `save auto-capture`

Detect and capture new saves (called by the launch wrapper on game exit; defaults
to the active profile).

```bash
modde save auto-capture --game <id> [--profile <name>]
```

### `save watch`

Watch for save changes and auto-capture via polling.

```bash
modde save watch --game <id> [--profile <name>] [--interval <secs>]
```

| Flag         | Default | Description               |
| ------------ | ------- | ------------------------- |
| `--interval` | `30`    | Poll interval in seconds  |

---

## `modde update`

Check for, and apply, updates. With no flags, `update check` checks for a new
**modde** release; with `--mods`, `--profile`, or `--game`, it checks tracked
profile mods on Nexus instead.

### `update check`

```bash
modde update check [--profile <name>] [--game <id>] [--period <period>] [--mods]
```

| Flag        | Default | Description                                                         |
| ----------- | ------- | ------------------------------------------------------------------- |
| `--profile` | —       | Check this profile's tracked Nexus mods                             |
| `--game`    | —       | Restrict to this game                                               |
| `--period`  | `1w`    | Time window: `1d`, `1w`, or `1m`                                     |
| `--mods`    | —       | Check Nexus-tracked profile mods instead of modde itself            |

If none of `--mods`, `--profile`, or `--game` is given, this performs the product
(modde) update check.

### `update apply`

Download and install the latest MAIN file for any tracked mod in the profile that
has a newer Nexus version.

```bash
modde update apply [--profile <name>] [--game <id>] [flags...]
```

| Flag                | Description                                                                                          |
| ------------------- | --------------------------------------------------------------------------------------------------- |
| `--period <period>` | Time window to scan: `1d`, `1w`, or `1m` (default `1w`)                                              |
| `--dry-run`         | Print the mods that would be updated without downloading                                            |
| `--confirm-locked`  | Acknowledge that the profile is locked (Wabbajack / Collection / TOML import); **required** for any locked profile, since updating drifts it away from its authoritative source |
| `--accept-breaking` | Permit applying updates that look like breaking semver bumps (major version change)                 |
| `--yes`             | Skip interactive prompts (assume "yes"); still refuses breaking updates unless `--accept-breaking` is also set |

Breaking (major semver) updates always require an interactive y/N confirmation
per mod even with `--accept-breaking`, unless `--yes` is also passed.

```bash
# Preview, then apply on a locked Wabbajack profile, allowing major bumps
modde update apply --profile lotf --period 1m --dry-run
modde update apply --profile lotf --period 1m --confirm-locked --accept-breaking
```

---

## `modde loot`

LOOT masterlist integration for Bethesda plugin sorting.

### `loot sort`

Sort plugins using LOOT masterlist rules.

```bash
modde loot sort --game <id> [--data-dir <path>]
```

| Flag         | Description                                   |
| ------------ | --------------------------------------------- |
| `--data-dir` | Path to the game `Data` directory (auto-detected if omitted) |

### `loot validate`

Validate plugins for Form 43 and missing master errors.

```bash
modde loot validate --game <id>
```

---

## `modde tool`

External tool, executable, and overlay management. This command exposes both the
overlay/patch surface (MangoHud, vkBasalt, GameMode, ReShade, OptiScaler, Proton)
and the full named-executable surface (the same storage `modde exec` aliases).

### `tool run`

Run an external tool by path with overwrite capture.

```bash
modde tool run <executable> [--profile <name>] [--game <id>] [-- <args>...]
```

### `tool add-executable`

Save a named executable launch target for a game. Equivalent to
[`modde exec add`](#exec-add).

```bash
modde tool add-executable <name> <executable> --game <id> [flags...] [-- <args>...]
```

| Flag                   | Default         | Description                                          |
| ---------------------- | --------------- | ---------------------------------------------------- |
| `--game`               | —               | Game ID (required)                                  |
| `--working-dir`        | game install    | Working directory                                   |
| `--output-mod`         | `__overwrite__` | Mod that captures files written during the run      |
| `--wine-dll-overrides` | —               | Wine DLL overrides, e.g. `dinput8=n,b;winmm=n,b`    |
| `--env KEY=VALUE`      | —               | Environment variable assignment (repeatable)        |
| `-- <args>...`         | —               | Default arguments                                   |

### `tool list-executables`

List saved executable launch targets for a game.

```bash
modde tool list-executables --game <id>
```

### `tool remove-executable`

Remove a saved executable launch target.

```bash
modde tool remove-executable <name> --game <id>
```

### `tool run-executable`

Run a saved executable launch target with overwrite capture. Equivalent to
[`modde exec run`](#exec-run).

```bash
modde tool run-executable <name> --game <id> [--profile <name>] [-- <args>...]
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

Tool IDs: `mangohud`, `vkbasalt`, `gamemode`, `reshade`, `optiscaler`, `proton`.

### `tool disable`

Disable a tool or overlay.

```bash
modde tool disable <tool_id> --game <id>
```

### `tool configure`

Configure tool settings as `key=value` pairs.

```bash
modde tool configure <tool_id> --game <id> -- <key=value>...
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

### `tool releases`

List releases for a release-backed tool (`reshade`, `optiscaler`). Set
`GITHUB_TOKEN` to raise the GitHub API rate limit.

```bash
modde tool releases <tool_id> --game <id>
```

### `tool install-release`

Install a specific release asset for a release-backed tool, downloading it from
GitHub.

```bash
modde tool install-release <tool_id> --game <id> --tag <tag> --asset <asset>
```

| Flag      | Description                  |
| --------- | ---------------------------- |
| `--tag`   | Release tag (required)       |
| `--asset` | Asset filename (required)    |

### `tool install-release-from-path`

Install a specific release asset from an already-downloaded local file.

```bash
modde tool install-release-from-path <tool_id> --game <id> --tag <tag> --asset <asset> <path>
```

| Flag      | Description                            |
| --------- | -------------------------------------- |
| `--tag`   | Release tag the asset corresponds to   |
| `--asset` | Asset filename                         |
| `<path>`  | Local path to the downloaded asset     |

```bash
modde tool releases optiscaler --game cyberpunk2077
modde tool install-release optiscaler --game cyberpunk2077 \
  --tag v0.7.7 --asset OptiScaler_v0.7.7.7z
```

See the [tools guide](../guides/tools.md) for what each overlay does.

---

## `modde fomod`

FOMOD installer utilities. See the [FOMOD guide](../guides/fomod.md).

### `fomod generate`

Generate a declarative FOMOD config template from a mod's `ModuleConfig.xml`.

```bash
modde fomod generate <mod_path> [--all] [--format <fmt>]
```

| Flag       | Description                                       |
| ---------- | ------------------------------------------------- |
| `--all`    | Include all plugins (not just defaults)           |
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

Scan a game directory for installed mods. See the
[scanning guide](../guides/scanning.md).

```bash
modde scan --game <id> [--game-dir <path>] [--manifest <path>] [--import-to <profile>] [--threshold <0.0-1.0>] [--dry-run] [--prune-duplicates]
```

| Flag                 | Default       | Description                                                                                       |
| -------------------- | ------------- | ------------------------------------------------------------------------------------------------ |
| `--game-dir`         | auto-detected | Game installation path                                                                           |
| `--manifest`         | —             | `.wabbajack` file for manifest matching                                                          |
| `--import-to`        | —             | Import discovered mods into this profile                                                         |
| `--threshold`        | `0.5`         | Minimum file presence fraction for a match                                                       |
| `--dry-run`          | —             | Report only, don't write to database                                                             |
| `--prune-duplicates` | —             | Remove filesystem-scanner rows covered by the manifest (requires `--manifest` and `--import-to`) |

---

## `modde collisions`

Analyse mod file collisions. See the [conflicts guide](../guides/conflicts.md).

```bash
modde collisions [--profile <name>] [--game <id>] [--all] [--suggest-hides]
```

| Flag              | Description                                 |
| ----------------- | ------------------------------------------- |
| `--all`           | Show all collisions including cosmetic ones |
| `--suggest-hides` | Suggest hide commands for redundant files   |

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

## `modde verify`

Verify installed file integrity for a profile.

```bash
modde verify [--profile <name>] [--game <id>]
```

---

## `modde backup`

Manage mod and plugin order backups.

### `backup create`

Create a backup of an installed mod's content-store directory.

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

Backup the real plugin order for a profile. modde reads the DB-backed order first
and falls back to native `plugins.txt` when needed.

```bash
modde backup plugins --profile <name> --game <id>
```

### `backup restore-plugins`

Restore plugin load order from a backup, writing back to both the profile DB state
and native `plugins.txt` when the game uses one.

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

## `modde detect`

Detect installed games across Steam and Heroic launchers.

```bash
modde detect
```

---

## `modde instance`

Manage modde instances (multiple data directories). `instance switch` changes the
active modde data root used by the database, store, downloads, and backups unless
`--data-dir` overrides it for a single command.

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

## `modde skill`

Install and update the built-in modde-maintained agent skills (written under
`~/.agents/skills/`). The built-in set is `modde-hm-integration`,
`wabbajack-readiness`, and `manual-archive-curation`.

### `skill list`

List built-in modde skills and their install status (`missing`, `installed`,
`outdated`, or `newer`).

```bash
modde skill list
```

### `skill path`

Print the target skill directory (`$HOME/.agents/skills`).

```bash
modde skill path
```

### `skill install`

Install or update one built-in skill, or `all`.

```bash
modde skill install <name|all> [--force]
```

| Flag      | Description                                                       |
| --------- | ---------------------------------------------------------------- |
| `--force` | Replace installed skills even when the installed version is newer |

```bash
modde skill install all
modde skill install wabbajack-readiness --force
```

---

## `modde import`

Import existing TOML-format profiles into the database.

```bash
modde import
```

---

## `modde gui`

Launch the graphical user interface (the same binary, with its own iced runtime).

```bash
modde gui
```

---

## Common workflows

### First install and play (Nexus collection)

```bash
modde nexus auth                                  # store the API key once
modde detect                                      # find installed games
modde install nexus-collection my-collection --profile main
modde play main --game skyrim-se                  # switch + deploy + launch + capture
```

### Prepare and run a large Wabbajack list

```bash
modde wabbajack assess ./lotf.wabbajack --game-dir ~/Games/skyrim
modde wabbajack missing-impact ./lotf.wabbajack --nix-snippet
modde wabbajack import-archive ./lotf.wabbajack ~/Downloads/manual-archive.7z
modde install wabbajack ./lotf.wabbajack --profile lotf \
  --game-dir ~/Games/skyrim --continue-on-error --diagnostics-dir ./wj-diag
modde wabbajack analyze-diagnostics ./wj-diag
```

### Experiment safely with the rollback stack

```bash
modde profile fork main experiment --game skyrim-se --unlock
modde profile try experiment --game skyrim-se     # push onto the stack
modde update apply --profile experiment --period 1m --accept-breaking
modde profile rollback --game skyrim-se           # undo if it broke
modde profile commit --game skyrim-se             # or keep it
```

### Wire up a modding tool

```bash
modde exec add xEdit "~/tools/SSEEdit/SSEEdit.exe" --game skyrim-se \
  --wine-dll-overrides "dinput8=n,b"
modde exec run xEdit --game skyrim-se --profile main   # output captured to __overwrite__
modde collisions --profile main --game skyrim-se --suggest-hides
```

### Back up before and capture after a session

```bash
modde backup plugins --profile main --game skyrim-se
modde save capture --game skyrim-se --profile main -m "before the dungeon"
modde play main --game skyrim-se                  # auto-captures saves on exit
modde save history --game skyrim-se --profile main
```

## See also

- [Installation](../getting-started/installation.md) — what install channel is live today
- [Home-Manager module reference](../configuration/hm-module.md) — the declarative surface
- [Parity audit](parity.md) — what is `Done` vs `Partial`
- [Supported games](../games/supported-games.md) — the 15 built-in games
- [Generic games guide](../games/generic-games.md) — user-defined game support
- [Wabbajack guide](../guides/wabbajack.md) — readiness and install workflow
- [Executables guide](../guides/executables.md) — named launch targets
- [Saves guide](../guides/saves.md) — the git-backed save vault
