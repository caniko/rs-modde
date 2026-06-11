# Executables & external tools

## Overview

modde manages **named executable launch targets** — the modding tools you run *against* a game install, such as xEdit, BodySlide, Nemesis, FNIS, the Creation Kit, or LOOT. Each target stores a name, an executable path, default arguments, a working directory, environment variables, optional Wine DLL overrides, and an output mod. When you run a target, modde snapshots the mod directory, runs the tool, and **captures any new files** the tool wrote into an output mod so they survive future deploys.

This feature is shipped end to end and user-reachable from both the CLI and the GUI, so it is marked **Done** in the [capability matrix](../reference/parity.md). It is distinct from [Tools & Overlays](tools.md), which manages the fixed set of six gaming overlays (MangoHud, OptiScaler, Proton, …).
It is also distinct from [Patcher pipelines](patcher-pipelines.md): executables
are user-invoked tools with overwrite capture, while patcher stages are
profile-scoped deployment steps that run automatically during `modde deploy`.

Configurations are stored in modde's database (the `executable_configs` table), keyed uniquely by `(game_id, name)`. Saving a target with an existing name overwrites it, so **add doubles as edit**.

## Two equivalent command surfaces

The same storage is exposed through two CLI namespaces. `modde exec ...` is a thin alias for the executable subset of `modde tool ...`; `modde exec list` and `modde tool list-executables` print the same rows.

| Short form (`modde exec`) | Long form (`modde tool`) | Action |
| --- | --- | --- |
| `modde exec add` | `modde tool add-executable` | Save or update a named target |
| `modde exec list` | `modde tool list-executables` | List configured targets for a game |
| `modde exec remove` | `modde tool remove-executable` | Delete a target |
| `modde exec run` | `modde tool run-executable` | Run a saved target with overwrite capture |

There is also a one-shot form, `modde tool run <executable> [-- args]`, which runs an arbitrary executable with overwrite capture **without** saving a configuration.

## Fields

Every saved target has the following fields. On the CLI they map to positional arguments and flags on `modde exec add` / `modde tool add-executable`:

| Field | CLI | Required | Default | Notes |
| --- | --- | --- | --- | --- |
| Name | positional `<name>` | yes | — | Display name, e.g. `xEdit`. Unique per game. Cannot be blank. |
| Executable | positional `<executable>` | yes | — | Path to the program to run |
| Game | `--game <id>` | yes | — | Must be a supported game id |
| Working directory | `--working-dir <dir>` | no | detected game install dir | The cwd the tool runs in |
| Output mod | `--output-mod <name>` | no | `__overwrite__` | Mod that captures newly written files. Cannot be blank. |
| Wine DLL overrides | `--wine-dll-overrides <str>` | no | none | Exported as `WINEDLLOVERRIDES`, e.g. `dinput8=n,b;winmm=n,b` |
| Environment | `--env KEY=VALUE` (repeatable) | no | none | Each must be `KEY=VALUE` with a non-empty key |
| Default args | `-- ARGS...` | no | none | Everything after `--` is the tool's default argument list |

`add-executable` validates eagerly: an unsupported game id, a blank name, a blank output mod, or a malformed `--env` entry (missing `=` or empty key) is rejected before anything is written.

## Adding a target

```bash
modde tool add-executable xEdit /games/skyrim/SSEEdit.exe \
  --game skyrim-se \
  -- -quickautoclean
```

The equivalent short form:

```bash
modde exec add xEdit /games/skyrim/SSEEdit.exe --game skyrim-se -- -quickautoclean
```

Both store the same row. The arguments after `--` are saved as the tool's defaults; when you later run the target you can append more arguments after another `--` and they are concatenated onto the saved defaults.

## Listing and removing

```bash
modde exec list --game skyrim-se
# Executables for skyrim-se:
#   xEdit
#     path: /games/skyrim/SSEEdit.exe
#     args: -quickautoclean
#     output mod: __overwrite__

modde exec remove xEdit --game skyrim-se
```

`remove` reports an error if no target with that name exists for the game.

## Running with overwrite capture

When you run a saved target (or `modde tool run <exe>` directly), modde performs the same four-step capture used for any external tool:

1. **Snapshot** the game's mod directory (the deployed VFS tree) before the tool runs.
2. **Run** the tool with the configured working directory (defaulting to the detected install dir), saved `--env` variables, `WINEDLLOVERRIDES` (if set), and the saved arguments plus any you appended.
3. **Snapshot again** after the tool exits and diff against the before-snapshot.
4. **Move** every newly written file into the output mod under modde's store directory (`~/.local/share/modde/store/<output_mod>/`).

```bash
# Run the saved xEdit target, appending an extra flag this time
modde exec run xEdit --game skyrim-se -- -autoload

# Run against a specific profile's deployed tree
modde exec run xEdit --game skyrim-se --profile my-skyrim
```

If the tool exits non-zero, modde warns but still captures whatever was written. If nothing new appeared, modde prints `No new files written to mod directory.` and stops. Otherwise it lists each captured file and reminds you to add the output mod to your profile:

```
Tool wrote 3 new file(s). Moving to output mod '__overwrite__':
  meshes/actors/character/...
  ...
Output mod: ~/.local/share/modde/store/__overwrite__
Add '__overwrite__' to your profile mod list to include these files in future deploys.
```

### What lands in the output mod

Only files that are **new** relative to the pre-run snapshot of the mod directory are captured. Files the tool modified in place that already existed in the deployed tree are not moved (they belong to whichever mod owns them). The captured files are *moved* (renamed, or copied-then-removed across filesystems) into the output mod, so they leave the deployed tree and become a first-class mod you can enable, disable, and order like any other. The default `__overwrite__` mod mirrors Mod Organizer 2's "Overwrite" pseudo-mod; you can target a different mod with `--output-mod` to keep, say, BodySlide output separate from xEdit output.

## The GUI Executables view

The graphical UI ships an **Executables** view that manages the same database rows. It lists each configured target with its name, executable path, and a metadata line summarising args, working directory, Wine DLL overrides, and output mod. Per row you get **Run**, **Edit**, and **Remove** buttons (Run and Remove are disabled while that target is already running). A **Refresh** button reloads the list, and an **Add executable** button opens an editor form with fields for name, executable path (with a **Browse** picker), arguments, output mod, working directory (with a **Browse** picker), `WINEDLLOVERRIDES`, and `KEY=VALUE` env lines. Saving an existing name updates it in place (the button reads "Save / update").

## Worked examples (Proton)

Windows modding tools run under Proton/Wine on Linux. The patterns below assume the tool's `.exe` lives in or near the game install and that you have a usable Wine/Proton runner. Use `--wine-dll-overrides` when a tool needs native DLLs, `--working-dir` when it must run from a specific folder, and `--output-mod` to keep generated assets out of the catch-all overwrite.

### xEdit (SSEEdit) — quick auto clean

```bash
modde tool add-executable xEdit /games/skyrim/SSEEdit.exe \
  --game skyrim-se \
  --output-mod xedit-output \
  -- -quickautoclean

modde exec run xEdit --game skyrim-se
```

xEdit writes cleaned plugins and backups; capturing them into `xedit-output` lets you review and order them as a dedicated mod.

### BodySlide — built meshes to a dedicated mod

```bash
modde tool add-executable BodySlide "/games/skyrim/Data/CalienteTools/BodySlide/BodySlide x64.exe" \
  --game skyrim-se \
  --working-dir "/games/skyrim/Data/CalienteTools/BodySlide" \
  --output-mod bodyslide-output \
  --wine-dll-overrides "dinput8=n,b"

modde exec run BodySlide --game skyrim-se
```

BodySlide builds meshes into the game's `Data` tree; the new `.nif`/`.tri` files are captured into `bodyslide-output`, which you then place after the body/armor mods in your load order.

### Nemesis — generated behavior files

```bash
modde tool add-executable Nemesis \
  "/games/skyrim/Data/Nemesis_Engine/Nemesis Unlimited Behavior Engine.exe" \
  --game skyrim-se \
  --working-dir "/games/skyrim/Data/Nemesis_Engine" \
  --output-mod nemesis-output \
  --wine-dll-overrides "vcruntime140=n,b"

modde exec run Nemesis --game skyrim-se
```

Nemesis regenerates behavior files under `meshes/`; capturing them into `nemesis-output` keeps the generated animation data as a single, re-buildable mod that wins over the source animation mods.

## See also

- [Tools & Overlays](tools.md) — the separate gaming-overlay manager (MangoHud, OptiScaler, Proton)
- [Deployment & VFS](deployment.md) — how the mod directory you capture from is built
- [Profiles](profiles.md) — enabling and ordering the captured output mod
- [Conflict resolution](conflicts.md) — where the output mod sits in priority
- [CLI reference](../reference/cli.md) — full command and flag listing
