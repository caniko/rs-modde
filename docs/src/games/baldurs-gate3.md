# Baldur's Gate 3

Baldur's Gate 3 (`baldurs-gate3`) runs on Larian Studios' Divinity engine and is
modelled in modde as the `Larian` engine family. Mods are distributed as `.pak`
archives, the enabled module list lives in `modsettings.lsx`, and the writable
game data lives inside the Steam Proton prefix rather than the install
directory. modde ships built-in support for all of these.

## Overall status

Baldur's Gate 3 is **`Partial`**. Deployment, scanning, conflict
classification, save tracking, and `modsettings.lsx` load-order generation are
wired and reachable, but several BG3-specific workflows are deliberately
conservative and there is no FOMOD-grade installer UI for the game's
`.pak`-with-`info.json` ecosystem. Treat it as solid for `.pak`-based content
and load order, and verify by hand for anything that touches the Script
Extender or non-standard archive layouts.

| Capability | Status | Notes |
| ---------- | ------ | ----- |
| Install detection | `Done` | Steam app id `1086940`, Steam dir `Baldurs Gate 3` |
| Mod directory resolution | `Done` | Proton-prefix `Mods/` with install-relative fallback |
| Scanner | `Partial` | Discovers root `.pak` mods under `Mods/` |
| Conflict detection | `Partial` | `pak`-aware collision classifier |
| Save tracking | `Done` | `.lsv` saves, git-backed vault |
| Load order | `Partial` | Generates `modsettings.lsx` from deployed `.pak` files |

The canonical status baseline for every game lives in
`docs/capability-matrix.toml`; this page describes how the BG3 plugin behaves,
not a promise beyond that matrix.

## Install detection

modde locates the install through its launcher IDs:

| Launcher | Identifier |
| -------- | ---------- |
| Steam app id | `1086940` |
| Steam library dir | `steamapps/common/Baldurs Gate 3` |
| Heroic (GOG/Epic) | not registered |

Only Steam is registered for BG3 today. The GOG and Epic Heroic app ids are not
populated, so if you run a non-Steam copy you must point modde at the install
yourself with `--game-dir` (CLI) or `gameDir` (home-manager).

## Mod directory and path resolution

The writable BG3 data root is **not** in the install directory. Under Proton it
lives inside the prefix that Steam creates for app `1086940`. modde resolves the
data root by walking up from the install path to the `steamapps/common`
directory and then descending into the matching `compatdata` prefix:

```
steamapps/compatdata/1086940/pfx/drive_c/users/steamuser/AppData/Local/Larian Studios/Baldur's Gate 3/
```

From that data root, modde derives:

| Purpose | Path (relative to the data root above) |
| ------- | -------------------------------------- |
| Mod directory | `Mods/` |
| Load order file | `PlayerProfiles/Public/modsettings.lsx` |
| Save directory | `PlayerProfiles/Public/Savegames/` |

If modde cannot find a `steamapps/common` ancestor (for example a manually
specified non-Steam path), it falls back to an install-relative
`Larian Studios/Baldur's Gate 3` data root and the same sub-paths under it.
Directory matching for the bare-layout heuristics is case-insensitive, so
`Mods` / `mods` and `PlayerProfiles` / `playerprofiles` are both recognised.

The deploy strategy is modde's standard symlink farm: `.pak` files are staged
and symlinked into the resolved `Mods/` directory, leaving the original install
untouched and fully reversible. See [Deployment & VFS](../guides/deployment.md)
for the build → materialize → deploy pipeline.

## What scanning finds

The BG3 scanner discovers **root `.pak` files inside `Mods/`**. It prefers the
Proton-prefix data root, but if that prefix has no `Mods/` directory yet it
falls back to scanning the install directory's `Mods/` instead, so a fresh
prefix does not silently produce an empty scan.

- It looks only at the `Mods` directory (single-file `.pak` rule).
- Each discovered mod is reported with a `pak/` mod-id prefix and a confidence
  of `0.95`.
- A mod's on-disk footprint is the lower-cased `mods/<stem>.pak` file, which is
  what conflict detection compares between mods.

Scanning is shallow by design: it enumerates the `.pak` archives that BG3 itself
loads, not the contents inside each archive. There is no MO2-style file-tree or
per-archive INI inspection for BG3, which is part of why the title is `Partial`.
See [Scanning](../guides/scanning.md) for the general model.

## Conflict classification

BG3 uses modde's `pak`-aware collision classifier. Two mods are flagged when
they ship the same relative file path, and the severity is decided by file
extension:

| Severity | Extensions | Meaning |
| -------- | ---------- | ------- |
| Cosmetic | `dds`, `png`, `jpg`, `tga`, `nif` | Visual overlap; last writer wins |
| Config | `ini`, `json`, `xml` | Settings overlap; review which wins |
| Dangerous | `esp`, `esm`, `dll`, `lua`, `ws` | Code/plugin overlap; resolve before playing |

`pak`, `ucas`, and `utoc` are treated as archive extensions for collision
purposes, so two mods packing identically named loose files are compared rather
than two opaque archives being declared incompatible outright. Resolution
follows the load order: the later mod wins each conflicting path. See
[Conflicts](../guides/conflicts.md) for how to inspect and resolve them.

### Save-breaking content

Separately from collision severity, the BG3 plugin classifies individual mods
for save safety. A mod is treated as **save-breaking** when it contains
`.pak`, `.dll`, `.json`, or `.lsx` files, or when it ships a **Script Extender**
directory (`Script Extender` / `ScriptExtender`). Purely cosmetic mods (`.png`,
`.jpg`, `.dds`, `.tga`) are treated as safe. This classification is what feeds
the save fingerprint described below, so adding or removing a Script Extender
mod is correctly surfaced as a save-compatibility change.

## Save tracking

Save tracking is **`Done`**. BG3 stores saves as `.lsv` files under:

```
PlayerProfiles/Public/Savegames/
```

modde's BG3 save tracker:

- Matches `**/*.lsv` recursively under the save directory (each campaign save is
  its own folder containing an `.lsv`, so recursion is required).
- Categorises every detected save as `manual` and labels it by its path
  relative to the save directory.
- Sorts captures newest-first by modification time.

Saves are stored in a per-game git-backed vault at
`~/.local/share/modde/saves/baldurs-gate3/`, with one branch per profile. Each
snapshot embeds a **SHA-256 fingerprint of the save-breaking mods** (the `.pak`
/ `.dll` / `.json` / `.lsx` / Script-Extender content described above), so
restoring a save into a profile whose dangerous mods differ raises a
compatibility warning instead of silently loading an incompatible save. See
[Save Management](../guides/saves.md) for vault mechanics, adoption, and
restore.

## Load order and `modsettings.lsx`

BG3 reads its enabled module list from an LSX (Larian XML) file at
`PlayerProfiles/Public/modsettings.lsx`. modde manages this file for you:

- **After every deploy**, the plugin's post-deploy step reads the deployed
  `Mods/` directory, collects the file stem of each `.pak`, sorts the list, and
  writes a fresh `modsettings.lsx` enabling those modules in order. If no `.pak`
  files are present, the existing file is left untouched.
- The generated XML uses the standard `ModuleSettings` region with one
  `ModuleShortDesc` node per module, keyed on the `Folder` attribute.
- modde can also **read** an existing `modsettings.lsx`, parsing the ordered list
  of enabled module folders back out, which is how it reconciles against what
  the game currently has enabled.

The important limitation: the generated order is the **sorted `.pak` stem
order**, not a dependency-aware load order. For modlists whose `.pak` stems do
not encode the intended order (BG3 mods frequently need a specific sequence and
carry UUID/dependency metadata in their `meta.lsx`), you should review and
adjust `modsettings.lsx` — or use BG3 Mod Manager in the prefix — after
deploying. modde does not parse per-mod `meta.lsx` UUIDs or dependency
declarations.

## Installer specifics

BG3 mod packaging is varied, and modde handles the two layouts that map cleanly
to a symlink farm:

| Archive shape | What modde does |
| ------------- | --------------- |
| One or more `.pak` files at the archive root | Installs as a single file set (the `.pak`s land directly in `Mods/`) |
| A top-level `Mods/` directory in the archive | Strips the `Mods/` content root so its contents map to the game's `Mods/` |

A bare extracted layout is recognised as a BG3 mod when it contains `mods` or
`playerprofiles` directories, or root `.pak` / `.lsx` files (case-insensitive).

There is **no FOMOD installer** path for BG3 here — FOMOD is an XML installer
format used by Bethesda-style mods, and BG3's `.pak`-with-`info.json` packaging
is not FOMOD. There is likewise no REDmod (Cyberpunk) or SMAPI (Stardew)
handling, because those belong to other engines. Archives that bundle a
loose-file overlay alongside `.pak`s, or that expect a manual `info.json`
merge, are not auto-resolved; install those by hand and re-run a deploy.

## Gaming tools and Proton

The BG3 registration ships **no OptiScaler profile**, so there is no curated
DLSS/FSR/XeSS preset for this title the way there is for, say, Stellar Blade. You
can still enable the generic tools per profile — `mangohud`, `vkbasalt`,
`gamemode`, `reshade`, and especially `proton` — from the
[Tools & Overlays](../guides/tools.md) guide and the
[home-manager module](../configuration/hm-module.md).

The `proton` tool is the relevant one for BG3: it manages the per-game Proton
runtime selection, Wine prefix, environment, and DLL overrides. Because BG3's
mod directory and load-order file live **inside** the Proton prefix, the prefix
that `proton` targets must be the same one your `Mods/` and `modsettings.lsx`
resolve into.

## Linux and Proton notes (gotchas)

These notes cover the game's runtime on Linux and macOS, where Baldur's Gate 3
runs through Proton/Wine and its writable data lives inside the compatibility
prefix. On Windows the game runs natively — there is no Wine prefix, and `Mods/`,
`modsettings.lsx`, and saves live at their native Windows locations. modde itself
runs natively on all three platforms.

- **The mod directory is inside the prefix, not the install.** This is the most
  common surprise: editing files under `steamapps/common/Baldurs Gate 3` does
  not affect the modded game. modde resolves the prefix path for you, but if you
  ever inspect things manually, look under
  `compatdata/1086940/pfx/.../AppData/Local/Larian Studios/Baldur's Gate 3/`.
- **The prefix must exist before scanning the prefix data root.** If you have
  never launched BG3 through Proton, the `compatdata/1086940` prefix may not have
  a `Mods/` directory yet; the scanner falls back to the install directory, but
  load-order generation and saves only become meaningful once the prefix is
  populated. Launch the game once through Steam first.
- **Script Extender is save-breaking.** BG3's Script Extender lands as a
  `Script Extender` directory and is classified as dangerous/save-breaking.
  Toggling it changes the save fingerprint, so expect a compatibility warning if
  you restore older saves after enabling or disabling it. Script Extender itself
  is injected via the prefix and is not something modde installs for you.
- **`modsettings.lsx` ordering is heuristic.** As noted above, modde writes a
  sorted-stem order; if your list needs a specific sequence, adjust the file (or
  use a BG3-specific load-order tool inside the prefix) after deploying.
- **Steam-only detection.** Non-Steam copies need an explicit game directory.

## Worked example

### Home-manager profile

```nix
programs.modde = {
  enable = true;

  # Optional: needed for Nexus downloads / update tracking.
  nexus.apiKeyFile = "/run/secrets/nexus-api-key";

  profiles.bg3-mods = {
    game = "baldurs-gate3";
    installMode = "auto";

    # Point at the Steam install. modde derives the in-prefix Mods/ and
    # modsettings.lsx from this path automatically.
    gameDir = "/home/me/.local/share/Steam/steamapps/common/Baldurs Gate 3";

    tools = {
      gamemode.enable = true;
      # Keep modde's prefix targeting aligned with the game's actual prefix.
      proton.enable = true;
    };
  };
};
```

If BG3 is not installed yet, omit `gameDir` or set
`installMode = "await-game"`; activation prints the next step and continues
without failing. Set `gameDir` and rebuild once Steam has the game.

### CLI

```bash
# Scan the resolved in-prefix Mods/ directory for installed .pak mods.
modde scan --game baldurs-gate3

# Install a Nexus-hosted BG3 mod archive into the active profile.
modde install mod "https://www.nexusmods.com/baldursgate3/mods/1234" \
  --profile bg3-mods

# Deploy the profile. This symlinks .pak files into the prefix Mods/ and
# regenerates PlayerProfiles/Public/modsettings.lsx from what was deployed.
modde deploy --game baldurs-gate3

# Inspect conflicts between deployed mods.
modde conflicts --game baldurs-gate3

# Adopt existing .lsv saves into a profile vault.
modde save adopt --game baldurs-gate3 --profile bg3-mods
```

After deploying, review `PlayerProfiles/Public/modsettings.lsx` if your modlist
relies on a specific load order, since modde generates a sorted-stem order
rather than a dependency-resolved one.

## See also

- [Supported Games](supported-games.md) — the status matrix and the full
  per-game guide index
- [Deployment & VFS](../guides/deployment.md) — the symlink-farm pipeline
- [Scanning](../guides/scanning.md) — how installed mods are discovered
- [Conflicts](../guides/conflicts.md) — inspecting and resolving collisions
- [Save Management](../guides/saves.md) — git-backed vaults, fingerprinting,
  restore
- [Tools & Overlays](../guides/tools.md) — Proton, OptiScaler, and overlays
- [Home-Manager Module](../configuration/hm-module.md) — declarative profile
  options
- [Feature parity](../reference/parity.md) — how modde compares to MO2/Vortex
