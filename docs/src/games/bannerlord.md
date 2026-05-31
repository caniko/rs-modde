# Mount & Blade II: Bannerlord

Mount & Blade II: Bannerlord (`bannerlord`) is TaleWorlds' medieval sandbox built on its in-house engine. modde models it as its own engine family (`EngineFamily::Bannerlord`) because its mod layout — a flat `Modules/` directory where each module carries a `SubModule.xml` manifest — does not match any of the Bethesda, Unreal, REDengine, or SMAPI families.

## Overall status

Bannerlord ships as a **`Partial`** title in [the supported-games matrix](supported-games.md). What that means in practice:

- **Scanner** — Yes. modde discovers installed modules under `Modules/` and parses each `SubModule.xml` for identity and dependencies.
- **Conflict detection** — Yes, via the generic policy classifier plus Bannerlord-specific save-breaking and cosmetic classification.
- **Save tracking** — `Done`. The git-backed save vault, fingerprinting, and capture/restore all work against the Bannerlord save directory.

The `Partial` label reflects what is *not* yet built rather than broken behavior: there is no Bannerlord-aware load-order manager (the game's own Launcher / Mod Options screen still owns load order), and the mod information dialog only surfaces a Nexus-metadata side panel, not a file-tree / dependency-graph inspector. Deployment, scanning, conflict classification, save management, and launcher integration are all functional.

| Capability | Status | Notes |
| ---------- | ------ | ----- |
| Install detection (Steam) | Yes | Steam app id `261550`, install dir `Mount & Blade II Bannerlord` |
| Mod directory | Yes | `Modules/` under the install root |
| Filesystem scanner | Yes | Walks `Modules/`, parses `SubModule.xml` |
| Dependency checking | Yes | Reports `<DependedModule>` ids missing from the module set |
| Conflict classification | Yes | Generic classifier + Bannerlord save-breaking / cosmetic policy |
| Save tracking | `Done` | `Documents/Mount and Blade II Bannerlord/Game Saves/Native`, `*.sav` |
| Load-order management | Not shipped | Use the in-game Launcher's Mod Options screen |
| OptiScaler profiles | Not shipped | No bundled upscaling profile (see [tools](#gaming-tools-proton-and-overlays)) |

## How modde detects the install

Bannerlord is registered with a Steam app id of **`261550`** and a default Steam library directory name of **`Mount & Blade II Bannerlord`**. When modde walks your Steam libraries, it matches by app id first and falls back to the directory name. No Heroic (GOG/Epic) ids are registered, because Bannerlord is a Steam title; if you run it from a non-Steam source you can still point modde at the install explicitly with `--game-dir` (see the [worked example](#worked-example)).

On Linux, Steam runs Bannerlord through Proton. The mod directory and save directory modde uses are *host-side* paths — modde reads and writes `Modules/` directly in the real game install and the save directory under your real `$HOME/Documents`, so you do not generally need to reach inside the Proton prefix to manage mods. The game's Windows-side view of those paths is provided by Proton's drive mapping. The Windows executable lives under `bin/Win64_Shipping_Client/` relative to the install root; modde records that as the game's executable directory so tools and launch integrations can find it.

## Mod directory and deploy strategy

The mod directory is **`Modules/`** under the game install root. Each mod is a self-contained subdirectory (for example `Modules/MyAwesomeMod/`) that contains, at minimum, a `SubModule.xml` and usually a `bin/`, `ModuleData/`, `GUI/`, or `AssetPackages/` tree.

modde deploys mods with its standard **symlink-farm VFS**: mods are staged in `~/.local/share/modde/staging/<profile>/` and then linked into `Modules/`, leaving the original game files untouched and the deployment fully reversible. Conflict resolution (later mods in the load order win) happens during the build phase. See [Deployment & VFS](../guides/deployment.md) for the full pipeline.

### Archive analysis (what modde does when you install a mod)

When you install a Bannerlord mod archive, modde inspects the extracted contents and picks a layout strategy:

- **Module-root archives** — if the archive extracts to a directory that already contains a `SubModule.xml` at its top level, modde treats the whole archive as a single module. It uses the module's own `Id.value` attribute (from `SubModule.xml`) as the mod id when one is present.
- **`Modules/`-prefixed archives** — if the archive instead contains a top-level `Modules/` directory, modde strips that prefix and deploys its contents into the game's `Modules/` directory, so a mod packaged as `Modules/CoolMod/...` lands at `Modules/CoolMod/...`.

The same two shapes are recognized as a "bare layout" (a loose, already-extracted folder you point modde at): a directory holding a `SubModule.xml`, or a directory whose root contains a `modules/` folder and `.xml` files (the directory match is case-insensitive). This is what lets you adopt manually downloaded mods without a wrapping archive.

## What scanning finds

`modde scan --game bannerlord` walks the `Modules/` directory and reports one discovered mod per immediate subdirectory that contains a `SubModule.xml`. For each such module it:

1. Parses `SubModule.xml` for the module **id** (`<Id value="...">`) and human-readable **name** (`<Name value="...">`). If `<Id>` is missing it falls back to the directory name; if `<Name>` is missing it falls back to the id.
2. Records every file under the module directory, relative to the install root, as the module's footprint.
3. Emits a discovered mod with id `module/<id>`, the parsed display name, and a high confidence score (0.95). Modules with no files are skipped.

A directory without a `SubModule.xml` is not reported — modde only counts a subfolder as a Bannerlord module when that manifest is present. See [Mod Scanning](../guides/scanning.md) for importing scan results into a profile and for Wabbajack-manifest matching.

### Dependency checking

Beyond identity, `SubModule.xml` parsing collects every `<DependedModule Id="...">` entry. modde can then cross-reference the declared dependencies of all discovered modules against the set of module ids that are actually present and report **missing dependencies** as `(module_id, missing_dependency_id)` pairs. This catches the classic Bannerlord failure mode where a mod silently refuses to load because a required module (for example `Bannerlord.Harmony` or `Bannerlord.UIExtenderEx`) is not installed. modde does not reorder modules to satisfy dependencies — load order remains the in-game Launcher's job — it only surfaces what is missing.

## Conflict classification

Bannerlord uses the generic policy collision classifier for file-level overwrite severity, layered on top of a Bannerlord content policy that maps file types to save-breaking, cosmetic, and content categories:

| Extension | Content category | Save-breaking? |
| --------- | ---------------- | -------------- |
| `dll` | Binary | Yes |
| `xml` | Config | Yes |
| `xslt` | Config | Yes |
| `xsl` | Config | Yes |
| `pak` | Archive | Yes |
| `dds`, `png`, `tga`, `jpg` | Texture | No (cosmetic) |

The save-breaking extension set is `dll`, `xml`, `xslt`, `xsl`, `pak`; the cosmetic set is `png`, `jpg`, `dds`, `tga`. In addition, anything under a module's `bin/` directory or a `submodule.xml` file is treated as save-breaking, because those carry the compiled assemblies and the module manifest that actually change how a campaign behaves. Texture overwrites are classified as cosmetic — visible, but they will not invalidate a save.

Run `modde collisions --profile <name>` to see conflicting module pairs with severity and file counts, or add `--all` to include cosmetic overlaps. The conflict model (load-order priority, shadowed mods, redundant files, per-file hiding) is the same one described in [Conflicts & Load Order](../guides/conflicts.md); the LOOT plugin-sorting parts of that guide are Bethesda-only and do not apply to Bannerlord.

## Save tracking

Bannerlord saves are tracked from:

```
~/Documents/Mount and Blade II Bannerlord/Game Saves/Native/
```

The tracker matches **`*.sav`** files (non-recursively) in that directory and files them all under the `campaign` category. Each file's relative name is used as its label, and the capture summary is reported by category.

These saves flow into modde's standard **git-backed save vault**: a per-game repository under `~/.local/share/modde/saves/bannerlord/`, with one branch per profile. Switching profiles captures the outgoing saves and restores the incoming profile's. Every snapshot embeds a **SHA-256 fingerprint** of the profile's enabled save-breaking mods (the `dll`/`xml`/`xslt`/`xsl`/`pak` + `bin/` + `submodule.xml` set above), so that on restore modde can warn you when a snapshot was taken with a different set of save-breaking modules than your current profile:

```
Mod-Fingerprint: a1b2c3d4e5f6
Save-Breaking-Mods: module/Bannerlord.Harmony, module/CoolMod
```

Adopt pre-existing saves with `modde save adopt --game bannerlord --profile <name>`, then capture/restore/history as in [Save Management](../guides/saves.md). Because Bannerlord saves carry the active module list inside the `.sav` itself, the fingerprint warning is a strong signal: loading a campaign save against a mismatched module set is the most common cause of corrupted Bannerlord saves.

## Plugin / load-order handling

Bannerlord has no separate plugin format (no `.esp`/`.esl` equivalent); a module either loads or it does not, and order is resolved by the game's own **Launcher → Mods** screen and the `LauncherData.xml` it writes. modde does **not** ship a Bannerlord load-order manager and does not write `LauncherData.xml`. What modde provides instead is:

- **Dependency visibility** — missing-`<DependedModule>` reporting (above), so you know *which* modules must be present and roughly in what dependency relationship.
- **Deployment ordering** — modde's load-order priority decides which mod wins a file-level conflict during deployment, which is independent from the runtime module activation order TaleWorlds' Launcher controls.

After deploying with modde, enable and order your modules in the in-game Launcher (or a third-party launcher such as the Bannerlord Mod Launcher / BUTR tools) as you normally would.

## Installer specifics

Bannerlord does not use FOMOD, REDmod, `.pak` mod managers, or SMAPI. Its packaging is simpler and modde handles it directly:

- **No FOMOD wizard** — most Bannerlord mods are plain module folders. modde's two archive shapes (a top-level `SubModule.xml`, or a top-level `Modules/` prefix to strip) cover the vast majority of Nexus downloads. The FOMOD installer described in [FOMOD installer](../guides/fomod.md) is used by Bethesda-style mods and generally does not apply here.
- **`.pak` files** are Bannerlord asset packages, classified as save-breaking archives — they are content, not an installer format, and are deployed like any other module file.
- **Harmony / UIExtenderEx / ButterLib / MCM** are themselves ordinary modules: install them like any other mod and let dependency checking confirm they are present.

Install a single mod from Nexus with `modde install mod <url> --profile <name>`; the Nexus domain for Bannerlord is `mountandblade2bannerlord`, which modde uses to resolve Nexus URLs to this game.

## Gaming tools, Proton, and overlays

Bannerlord registers **no OptiScaler profile**, so the bundled DLSS/FSR upscaling workflow that ships for titles like Stellar Blade is not pre-configured here. The general tool integrations still apply: you can enable MangoHud, vkBasalt, GameMode, ReShade, and per-game Proton settings through `modde tool ...`, exactly as documented in [Tools & Overlays](../guides/tools.md).

The most relevant tool for a Linux Bannerlord install is the **`proton`** integration, which stores per-game launch settings (Proton version mode, prefix override, extra environment variables, and DLL overrides) without installing Steam or the game itself:

```bash
# Keep Steam's default Proton runner for Bannerlord
modde tool configure proton --game bannerlord version_mode=launcher_default

# Force a DLL override if a native-loader mod needs it
modde tool configure proton --game bannerlord dll_override_mode=forced forced_dll_overrides=dxgi
```

modde's **executable management** is fully shipped and is useful for Bannerlord's out-of-game tools: register a named executable (with arguments, working directory, environment, Wine DLL overrides, and an output-capture mod) via `modde tool add-executable ...` and run it with `modde exec ...`, capturing any files the tool writes into an overwrite mod. This is handy for running modding utilities against the deployed `Modules/` tree.

## Linux / Proton notes and known gotchas

These notes cover the game's runtime on Linux and macOS, where Bannerlord runs through Proton/Wine. On Windows the game runs natively — there is no Wine prefix, and `Modules/` and the save directory live at their native Windows locations. modde itself runs natively on all three platforms.

- **Run the game once before modding.** Let Steam/Proton create the prefix and let Bannerlord generate `~/Documents/Mount and Blade II Bannerlord/` (Proton maps this from the prefix's `drive_c/users/steamuser/Documents`, but on a typical setup it surfaces at your real `$HOME/Documents`). modde's save tracker reads the `Game Saves/Native/` directory there.
- **Modules live host-side.** Because modde links into the real `Modules/` directory under the install root, you usually do not touch the Proton prefix to add or remove mods — but the game still needs to be launched through the same Proton runner you configured.
- **Enable modules in the Launcher after deploying.** Deployment puts the files in place; it does not toggle modules on. A freshly deployed module that is not enabled in the in-game Launcher will appear "missing" even though its files are present.
- **`.NET` module DLLs are save-breaking.** Adding or removing a `dll`-bearing module changes the save fingerprint. Expect (and heed) modde's mismatch warning when restoring an older campaign save.
- **Case sensitivity.** modde's bare-layout recognition treats the `Modules`/`modules` directory case-insensitively, which matters on Linux's case-sensitive filesystem when a mod ships an oddly-cased folder; the on-disk `Modules/` directory itself is whatever the game created.

## Worked example

### Declarative (home-manager)

Define a Bannerlord profile and wait for the game to be installed before modde deploys into it:

```nix
programs.modde.profiles.my-bannerlord = {
  game = "bannerlord";
  # Point at the real Steam install once it exists; until then modde
  # prints an awaiting message instead of failing activation.
  installMode = "await-game";
  # gameDir = "/home/you/.steam/steam/steamapps/common/Mount & Blade II Bannerlord";

  tools.proton.settings = {
    version_mode = "launcher_default";
  };
};
```

After Steam has installed the game, set `gameDir` to the install path and switch `installMode = "auto"`, then rebuild your home-manager configuration; modde deploys the profile during activation.

### Imperative (CLI)

```bash
# 1. Create a Bannerlord profile
modde profile create my-bannerlord --game bannerlord

# 2. Install a mod from Nexus into it
modde install mod \
  https://www.nexusmods.com/mountandblade2bannerlord/mods/2018 \
  --profile my-bannerlord

# 3. Import any modules already sitting in Modules/
modde scan --game bannerlord --import-to my-bannerlord

# 4. Inspect conflicts (add --all for cosmetic overlaps)
modde collisions --profile my-bannerlord

# 5. Deploy the symlink farm into the game's Modules/ directory
modde deploy --profile my-bannerlord --game bannerlord \
  --game-dir "$HOME/.steam/steam/steamapps/common/Mount & Blade II Bannerlord"

# 6. Adopt existing campaign saves into the git-backed vault
modde save adopt --game bannerlord --profile my-bannerlord
```

Then launch through Steam (Proton), enable your modules in the in-game Launcher, and order them there.

## See also

- [Supported games](supported-games.md)
- [Deployment & VFS](../guides/deployment.md)
- [Mod Scanning](../guides/scanning.md)
- [Conflicts & Load Order](../guides/conflicts.md)
- [Save Management](../guides/saves.md)
- [Tools & Overlays](../guides/tools.md)
- [Feature parity reference](../reference/parity.md)
