# Playing a Game

## Overview

`modde play` is the one command you run to actually play a modded game. It orchestrates the full session lifecycle end to end:

1. **Switch** to the requested profile (swapping saves automatically).
2. **Deploy** the profile's mods (build the symlink farm and wire up tools).
3. **Launch** the game through whichever launcher owns it (Steam or Heroic).
4. **Capture** saves into the vault when the game exits.

Each of those stages can be turned off independently with a flag, so `modde play` doubles as a precise tool when you only want part of the workflow.

## Basic usage

```bash
modde play --game skyrim-se
```

With no profile argument, modde uses the game's **active** profile. If there is no active profile, it errors and tells you to name one explicitly. To play a specific profile:

```bash
modde play my-skyrim --game skyrim-se
```

## What happens, step by step

### 1. Profile switch

If the named profile isn't already active, modde activates it. Activation captures the current profile's saves, parks them so Steam Cloud stays happy, and restores the target profile's saves to the live save directory — the same swap described in [Save Management](saves.md). The current mod fingerprint is embedded in the capture commit so future restores can warn about mismatches.

If the profile is already active, the switch is skipped silently.

There is one guardrail here: if modde finds saves in the game directory but no profile is active to own them, it **refuses to overwrite** and asks you to adopt them first:

```text
Found 3 unadopted save(s) in the game directory.
Run `modde save adopt --game skyrim-se --profile my-skyrim` first.
```

Run the suggested `save adopt` and then `modde play` again. See [Adopting existing saves](saves.md#adopting-existing-saves).

### 2. Mod deployment

modde builds and deploys the profile's symlink farm and applies tool integration (Wine DLL overrides, the launch wrapper, tool config files, and so on). This is the same work `modde deploy` does on its own; see [Deployment](deployment.md) and [Tools](tools.md) for the details.

### 3. Launch

modde detects which launcher owns the game and launches it (see [Launcher detection](#launcher-detection) below). It prints the source it's using, e.g. `Launching via Steam (1716740)...`.

### 4. Save auto-capture

When the game exits, modde captures the resulting saves into the vault for the active profile, with the mod fingerprint attached. Whether modde can do this synchronously depends on the launcher — see [Steam vs Heroic](#steam-vs-heroic).

## Skipping steps

Each flag disables one stage. They compose, so you can combine them.

| Flag | Effect |
| ---- | ------ |
| `--no-switch` | Skip the profile switch; deploy and launch the **already-active** profile. |
| `--no-deploy` | Skip deployment; switch and launch without rebuilding the symlink farm (use when nothing changed since the last deploy). |
| `--no-capture` | Skip save auto-capture after the game exits. |

Some useful combinations:

```bash
# I already deployed and the right profile is active — just launch it
modde play --game skyrim-se --no-switch --no-deploy

# Switch + deploy but launch read-only (don't touch my saves on exit)
modde play my-skyrim --game skyrim-se --no-capture
```

## Steam vs Heroic

The crucial difference between launchers is whether modde can *wait* for the game to exit, which determines how saves get captured.

### Heroic (and other waited launches)

Heroic launches (GOG, Epic/Legendary, and sideloaded apps) are run as a child process with `--no-gui --launch <app_id>`. modde **waits** for that process to exit, then captures saves directly:

```text
Game exited (status: 0).
Saves auto-captured for profile 'my-skyrim'.
```

This is the simplest path: capture happens inline, in the same `modde play` invocation.

### Steam (fire-and-forget)

Steam is launched by handing the OS a `steam://rungameid/<app_id>` URI. Steam itself then starts the game, and the launching call returns **immediately** — modde never sees the game process and cannot wait for it. So modde prints:

```text
Game launched via Steam (fire-and-forget).
Saves will be captured by the launch wrapper on exit, or run:
  modde save auto-capture --game skyrim-se
```

For Steam, saves are captured one of two ways:

1. **The launch wrapper** — during deployment modde generates a launch wrapper script and (where it can) registers it so it runs `modde save auto-capture --game <id>` after the game process exits. This is the hands-off path. On Steam you may need to add the wrapper to the game's launch options yourself (modde prints the snippet); see [Tools](tools.md) and the wrapper notes in [Launcher detection](#launcher-detection).
2. **Manually**, after you finish playing:

   ```bash
   modde save auto-capture --game skyrim-se
   ```

For games where save profiles aren't supported at all, modde simply notes the fire-and-forget launch and skips capture.

## Launcher detection

modde auto-detects installed games by scanning launcher libraries — you don't enter paths by hand. To see everything it found:

```bash
modde detect
```

### What modde scans

| Source | How it's detected |
| ------ | ----------------- |
| **Steam** | Reads every Steam library folder, parses each `appmanifest_<appid>.acf`, and matches the app ID against modde's registry. Falls back to matching known install-dir names under `steamapps/common/`. |
| **Heroic / GOG** | Parses `gog_store/installed.json` and matches GOG app IDs. |
| **Heroic / Epic** | Parses `legendary_store/installed.json` and matches Epic/Legendary app IDs. |
| **Heroic / Sideload** | Parses `sideload_apps/installed.json` and matches by install-directory name. |

A game can be detected from more than one source. The detected source carries the launcher and app ID, and is what `modde play` uses to launch (`Steam (<app_id>)`, `Heroic/GOG (<app_id>)`, etc.).

### Wine / Proton DLL override registration

On Linux, games run under Wine/Proton, and some mods ship **proxy DLLs** (for example a `version.dll` used by script-extender front-ends or `winmm.dll` used by RED4ext). Wine won't load a native DLL over its own built-in stub unless `WINEDLLOVERRIDES` tells it to. modde handles this per launcher during deployment:

- **Heroic** — modde writes the override directly into Heroic's `GamesConfig/<id>.json` under `enviromentOptions`, merging with any existing `WINEDLLOVERRIDES` so it doesn't clobber your settings. It also inserts modde's launch wrapper into Heroic's `wrapperOptions` chain (after fgmod, if present) so DLLs deleted by fgmod get restored before the game starts.
- **Steam** — modde can't edit Steam's per-game launch options programmatically, so it **prints the exact string** to paste into the game's launch options, e.g.:

  ```text
  WINEDLLOVERRIDES="version=n,b" %command%
  ```

- **Unknown launcher** — modde prints the `WINEDLLOVERRIDES` value to export before launching.

Wine DLL overrides are a Linux/Proton concern only; on native Windows there's nothing to override.

## When detection fails or the game won't launch

If `modde play` reports it `could not detect launcher`, work through these:

1. **Confirm detection.** Run `modde detect`. If the game isn't listed, modde didn't find an install for it.
2. **Is it actually installed via a scanned launcher?** modde scans Steam libraries and Heroic's GOG/Epic/Sideload stores. A game installed some other way (a raw extracted copy, a different launcher) won't be detected automatically.
3. **For Steam:** make sure the app appears in a Steam library folder with a valid `appmanifest_*.acf` and that the install directory under `steamapps/common/` actually exists. modde skips manifests whose install path is missing.
4. **For Heroic:** make sure the game shows as *installed* in Heroic — the `installed.json` for its store must list it with an `install_path` that exists on disk. Heroic must be installed as a Flatpak (`com.heroicgameslauncher.hgl`) or be on your `PATH` for modde to launch it; if neither is found you'll see `Heroic Games Launcher not found (checked flatpak and PATH)`.
5. **Override the path.** If detection genuinely can't see your install, you can set an explicit game path in modde's settings; an override path takes precedence over the launcher scan.
6. **Launch manually, capture manually.** As a fallback you can always start the game from Steam/Heroic yourself after a `modde play ... --no-capture` (or just `modde deploy`), then capture saves with `modde save auto-capture --game <id>` or a running `modde save watch`.

If the game launches but mods don't appear or crash on load, that's a deployment/conflict issue rather than a launch issue — see [Deployment](deployment.md), [Conflicts](conflicts.md), and [Troubleshooting](troubleshooting.md).

## See also

- [Save Management](saves.md) — how the profile switch swaps saves and how capture works
- [Deployment](deployment.md) — what the deploy stage builds
- [Tools](tools.md) — the launch wrapper, Wine overlays, and per-game tool config
- [Profiles](profiles.md) — choosing and managing the profile you play
- [Troubleshooting](troubleshooting.md) — launch and detection problems
- [CLI reference](../reference/cli.md) — full command and flag listing
