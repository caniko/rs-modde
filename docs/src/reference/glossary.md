# Glossary

This page defines the vocabulary the rest of the documentation assumes — both
modde's own concepts and the wider modding terms (FOMOD, LOOT, Wabbajack, REDmod,
…) you meet when you actually install mods. Each entry is one to three sentences
with a link to the guide that covers it in depth. Terms are grouped by theme;
within a group they read roughly in the order you encounter them.

## modde core concepts

### Profile

A profile is modde's central organizing unit: a per-game bundle of an ordered mod
set, a load order (and plugin order), a branch in the save vault, and the
deployment state that records exactly what was staged. Profiles are scoped by
`(name, game_id)`, so a `default` profile for Skyrim and a `default` for Fallout 4
coexist without collision. See [Profile management](../guides/profiles.md).

### Instance

An instance is a named, self-contained data directory. Instances let you run
entirely separate modde states — a daily driver and a Wabbajack testbed, say —
without sharing a database, store, or save vaults; the active instance is recorded
in `~/.config/modde/instances.toml`. See
[Data, instances & backups](../guides/data-management.md).

### Store

The store (`<data>/store/`) is modde's content-addressed mod file repository: the
actual staged bytes of every installed mod, one directory per mod, that the VFS
symlinks point into. The database's per-mod file manifest maps each profile's mods
to the precise files they staged, which is what makes uninstall surgical. See
[Deployment & VFS](../guides/deployment.md).

### Staging

Staging is the per-profile scratch tree (`profiles/<profile>/staging/`) that
`materialize()` writes the symlink farm into before it is projected onto the game
directory. For Wabbajack profiles, "staging" instead means the pre-resolved
`mods/<mod>/…` layout the modlist install produces. See
[Deployment & VFS](../guides/deployment.md).

### Deploy

To deploy is to project the active profile's materialized staging tree into the
game's resolved mod directory — creating the symlinks (or, for Wabbajack, hardlinks
or copies) the game actually sees. `modde deploy` (and `modde play`, which deploys
before launching) run this step. See [Deployment & VFS](../guides/deployment.md).

### Rollback

`modde rollback` swaps a profile's `staging` and `staging.bak` directories with
atomic directory renames, restoring a deliberately-preserved earlier deploy state;
it fails if no `staging.bak` exists, and you must re-run `modde deploy` afterward to
re-project the restored tree. See
[Deployment & VFS](../guides/deployment.md#rollback).

## Ordering and conflicts

### Load order vs. plugin order

These are two independent ordering systems, and conflating them is the most common
source of confusion. **Load order** (mod install priority) decides which *file*
wins when two mods ship the same path — later mod in the list wins, resolved by the
VFS. **Plugin order** decides the sequence Bethesda `.esp`/`.esm`/`.esl` plugins
load in the engine — a separate axis managed by LOOT and `plugins.txt`. See
[Conflicts & load order](../guides/conflicts.md).

### VFS / symlink farm

The VFS (virtual filesystem) is the merged, modded view of the game's data
directory that modde assembles as a **symlink farm**: symlinks pointing back at the
content-addressed store, so the real install on disk stays pristine. Unlike MO2's
USVFS — which hooks file-system calls per process — modde's symlinks live in the
real filesystem and are globally visible to every process without API hooking. See
[Deployment & VFS](../guides/deployment.md).

### Conflict / collision

A conflict (or collision) is when two or more enabled mods provide the same relative
path, so only one version can be deployed. modde resolves it by load order priority
— the highest-priority provider wins — and `modde collisions` reports the pairs,
per-file winners, and severities. See
[Conflicts & load order](../guides/conflicts.md).

### Collision severity (cosmetic / config / dangerous)

Each colliding file is graded by extension into a severity level: **Cosmetic**
(visual/audio only — `dds`, `nif`, `wav`; low risk), **Config** (may change
behaviour — `ini`, `json`, `toml`), **Dangerous** (scripts/plugins/DLLs that can
crash or corrupt saves — `esp`, `esm`, `dll`, `pex`), with **Unknown** for
unrecognised extensions. A mod pair's reported severity is the *worst* among its
colliding files, and the classifier is per-game. See
[Collision severity](../guides/conflicts.md#collision-severity).

### Shadowed mod

A shadowed mod is one whose *every* file is overridden by higher-priority mods, so
it contributes nothing to the deployed VFS. The collision report lists it as
`"<mod>" (N files, all overridden by <mods>)` and a diagnostic suggests disabling
it to shrink the deploy. See
[Shadowed mods and redundant files](../guides/conflicts.md#shadowed-mods-and-redundant-files).

### Redundant file

A redundant file is a single path from a mod that always loses the conflict — it is
never the deployed winner. `modde collisions --suggest-hides` turns each into an
actionable `modde profile hide` command. See
[Shadowed mods and redundant files](../guides/conflicts.md#shadowed-mods-and-redundant-files).

### Hidden file

A hidden file is a single file you exclude from a mod without disabling the whole
mod (modde's equivalent of MO2's `.mohidden`). Hidden files are recorded per
profile and skipped during the VFS build, so the next-highest provider of that path
wins instead. See [Per-file hiding](../guides/conflicts.md#per-file-hiding).

### Overwrite mod

The overwrite (output) mod is the destination that captures files a launched
executable writes back into the deployed tree — for example xEdit output or
generated patches — so they are not lost between deploys. It is configurable per
executable and is part of the shipped executable-management feature. See
[Executables & external tools](../guides/executables.md).

## Saves

### Save vault

A save vault is the git-backed history of a game's saves: one git repository per
game under `<data>/saves/<game_id>/`, with one branch per profile. Every capture is
an ordinary git commit, so you get full history, branching, and the ability to roll
a profile's saves back to any point. See [Save management](../guides/saves.md).

### Save snapshot

A save snapshot is a single capture in the vault — a git commit on the profile's
branch containing the live save files at that moment, plus the mod fingerprint
embedded as commit trailers. `modde save history` lists them and `modde save
restore <commit>` restores one. See [Save management](../guides/saves.md).

### Save fingerprint

A save fingerprint is a SHA-256 hash computed over only the **enabled,
save-breaking** mods of a profile (sorted and de-duplicated), stored as a commit
trailer on each snapshot. On restore, modde compares the snapshot's fingerprint
against your current profile's and warns when the save-breaking mod set differs.
See [Save-breaking mods and the fingerprint](../guides/saves.md#save-breaking-mods-and-the-fingerprint).

### Save-breaking mod

A save-breaking mod is one whose presence or absence can make a save game
incompatible — typically anything that changes the game's serialized data: script
extenders and their plugins, mods that bake scripts or records into saves,
framework mods. Cosmetic mods (textures, reshades, UI tweaks) are *not*
save-breaking, and each game plugin classifies its own mods. See
[Save management](../guides/saves.md#save-breaking-mods-and-the-fingerprint).

## Experiments, profiles, and instances

### Experiment stack

The experiment stack lets you try profile changes non-destructively, like git
branches for your mod setup: `modde profile try` pushes the current profile and
activates another, `rollback` pops back one level, and `commit` accepts wherever
you landed and clears the history. Saves swap along with each step. See
[Experiment stack](../guides/profiles.md#experiment-stack).

### Stock snapshot

A stock (vanilla) snapshot is a preserved, hardlinked copy of a clean game install,
captured with `modde stock snapshot <game>` before you mod anything. modde computes
a deterministic tree hash over it so `modde stock verify` can later tell whether the
install has drifted from vanilla. See
[Stock snapshots](../guides/data-management.md#stock-vanilla-game-snapshots).

## Install formats and methods

### FOMOD

FOMOD is the most common *scripted installer* format on Nexus: instead of a flat
archive, the mod ships a `fomod/ModuleConfig.xml` script that asks the user
questions (texture resolution, body type, optional patches) and copies a different
file set depending on the answers. modde supports it interactively (a wizard) and
declaratively (a generated config you can pin and replay). See
[FOMOD installer](../guides/fomod.md).

### FOMOD declarative config

A FOMOD declarative config is a file (TOML, JSON, or Nix) that records your FOMOD
answers so an install is reproducible and non-interactive — the path used by Home
Manager and any automation. Generate one with `modde fomod generate`, then pass it
as `--fomod-config`. See
[Declarative installation](../guides/fomod.md#declarative-installation).

### BAIN

BAIN (BAIN Archive INstaller) is the Wrye Bash multi-option layout: an archive of
numbered subdirectories like `00 Core`, `01 Option` that you pick from. modde
auto-*detects* BAIN by the numbered-subdir convention, but the interactive
sub-package selection flow is still `Partial` — until a selection is made,
execution returns `RequiresUserInput`. See
[How mod installation works](../guides/mod-installation.md) and the
[parity audit](parity.md).

### Install method

An install method is modde's classification of an archive's layout
(`InstallMethod`): `BareExtract`, `SingleFileSet`, `DirectoryMod`, `Fomod`,
`REDmod`, `Bain`, `DllOverlay`, and more. Detection is ordered so game-specific
layouts win before generic guesses; most methods stage automatically, while FOMOD
and BAIN may pause for input. See
[How mod installation works](../guides/mod-installation.md).

## Cyberpunk 2077 / REDengine

### REDmod

REDmod is CD Projekt Red's official mod packaging format for Cyberpunk 2077 — a
package keyed off an `info.json` manifest (plus `archives/`) that installs under
`<install>/mods/<name>/`. modde detects it, symlinks the package, and runs a
post-deploy `redmod deploy` pass to register it as a loadable archive. See
[Cyberpunk 2077](../games/cyberpunk2077.md).

### REDscript / CET / TweakXL

These are the major loose-file Cyberpunk modding frameworks modde deploys:
**REDscript** (compiled gameplay scripts under `r6/scripts/`), **CET** (Cyber Engine
Tweaks, a Lua console/UI loaded via a `version.dll` proxy under
`bin/x64/plugins/cyber_engine_tweaks/`), and **TweakXL** (data tweaks under
`r6/tweaks/`). modde deploys their files into the correct roots; each framework
sequences its own loading. See [Cyberpunk 2077](../games/cyberpunk2077.md).

## Bethesda plugins and archives

### BSA / BA2

BSA (Skyrim) and BA2 (Fallout 4) are Bethesda's packed archive container formats
for game assets. modde reads and indexes their contents so a loose
`textures/sky.dds` correctly collides with a `sky.dds` packed inside another mod's
`.bsa`; the container extension itself classifies as cosmetic, with danger judged by
what is inside. Wabbajack lists can also have modde repack loose files back into a
real `.bsa`/`.ba2`. See
[Archive-aware conflicts](../guides/conflicts.md#archive-aware-conflicts-bsa-ba2-pak).

### pak / ucas / utoc

`pak`, `ucas`, and `utoc` are the Unreal Engine 4/5 archive formats — the archive
extensions modde's UE classifier recognises for titles like Stellar Blade and
Oblivion Remastered. UE games route mod paks into a `~mods/` directory at deploy
time. See [Per-game classifiers](../guides/conflicts.md#per-game-classifiers) and
[Supported games](../games/supported-games.md).

### Master / plugin

A plugin is a Bethesda content file (`.esp`, `.esm`, `.esl`); a master is a plugin
that another plugin depends on (declared in its header). modde reads the first ~1 KB
of each active plugin's TES4 header and flags **missing masters** — a plugin
requiring a master not in the active load order, which crashes the game on load. See
[Validating plugins](../guides/conflicts.md#validating-plugins).

### Form 43

Form 43 (record header version `0.94`) is the old Skyrim LE ("Oldrim") plugin
format. Using a Form 43 plugin in Skyrim SE/AE (which expects Form 44, `1.70`) can
crash the game; `modde loot validate` detects it and the fix is to resave the plugin
in the Creation Kit. See [Validating plugins](../guides/conflicts.md#validating-plugins).

### LOOT

LOOT (the Load Order Optimisation Tool) is the community standard for sorting
Bethesda plugin load order. `modde loot sort` parses the LOOT masterlist and derives
load-after / incompatible rules for the plugins you actually have active. See
[LOOT sorting](../guides/conflicts.md#loot-sorting).

### Masterlist

The masterlist is LOOT's community-maintained YAML file of per-plugin `after`,
`requires`, and `incompatible` rules for a game. modde caches it locally and, if it
is missing, prints the exact `curl` command to fetch the right one; masterlists ship
for Skyrim SE/AE, Fallout 4, Fallout 76, and Starfield. See
[LOOT sorting](../guides/conflicts.md#loot-sorting).

## Wabbajack

### Wabbajack modlist

A Wabbajack modlist (a `.wabbajack` file) is a *recipe*, not a bundle of mods: it
records exactly which archives to download and exactly how to reconstruct a curated
mod setup from them. modde replays that recipe natively on Linux — no Windows VM,
no Wine-hosted client — verifying every download against its hash. See
[Wabbajack modlists](../guides/wabbajack.md).

### Directive

A directive is one step in a Wabbajack manifest's ordered install plan. modde
supports the common types: `FromArchive` (extract one file from a source archive),
`PatchedFromArchive` (extract then apply an OctoDiff binary delta),
`InlineFile`/`RemappedInlineFile` (write bytes stored inside the `.wabbajack`), and
`CreateBSA`/BA2 repack. See
[Install directives](../guides/wabbajack.md#install-directives).

## Nexus

### Nexus Collection

A Nexus Collection is a curated, version-pinned set of mods published on Nexus.
modde resolves it by slug (discovering the game domain and latest revision, then
fetching the pinned manifest) and installing it locks the profile to preserve the
curator's intended load order. See
[Nexus Collections](../guides/nexus.md#nexus-collections).

### `nxm://`

`nxm://` is the URI scheme Nexus's "Download with [manager]" buttons emit, carrying
the game domain, mod ID, file ID, and a short-lived `key`/`expires` pair. Run
`modde nxm install` to register modde as the system handler so clicking a Nexus
download link hands the URI to modde. See
[The nxm:// handler](../guides/nexus.md#nxm-protocol-handler).

## Games and engines

### GamePlugin

A `GamePlugin` is the per-game integration trait inside modde: each of the 15
built-in titles is a `GamePlugin` that knows that game's archive layouts, mod
directory, save location, conflict classifier, and Wine/Proton DLL-override quirks.
It also supplies the analyze hook that lets a game claim a layout (REDmod, an ENB
pack) before any generic install probe runs. See
[Games](../games/supported-games.md) and [Architecture](architecture.md).

### Generic game

A generic game is a user-defined title registered at runtime with `modde game add`
or an imported GameSpec TOML, rather than a hand-coded `GamePlugin`. Generic game
support is `Partial`: deployment, conflict classification, launcher integration, and
executable management work, but there is no bespoke filesystem scanner and no save
tracker. See [Generic & user-defined games](../games/generic-games.md).

## Tools and runtime

### OptiScaler

OptiScaler is an upscaler/frame-generation replacement (DLSS/FSR/XeSS) that hooks a
game through a proxy DLL, which modde manages as a release-backed tool — including
release downloading, community profiles, FSR4 payload variants, and OptiPatcher. It
also subsumes modde's old fgmod DLL-restore logic. See
[OptiScaler](../guides/tools.md#optiscaler).

### Proton / Wine DLL override

A Wine DLL override (`WINEDLLOVERRIDES`) tells Wine/Proton to load a mod's *native*
proxy DLL instead of its own built-in stub — required for frameworks that ship as a
proxy DLL (CET's `version.dll`, ReShade's `dxgi.dll`, OptiScaler). modde detects the
proxy DLLs a deploy places and contributes the overrides automatically, surfaced
through the per-game **Proton** tool integration. See
[Tools & overlays](../guides/tools.md) and
[Cyberpunk Wine DLL overrides](../games/cyberpunk2077.md#wine-dll-overrides).

## See also

- [Conflicts & load order](../guides/conflicts.md) — severity, shadowed mods, LOOT, Form 43
- [Deployment & VFS](../guides/deployment.md) — the symlink farm, staging, deploy, rollback
- [Save management](../guides/saves.md) — vaults, snapshots, fingerprints, save-breaking mods
- [Profile management](../guides/profiles.md) — profiles, locking, the experiment stack
- [How mod installation works](../guides/mod-installation.md) — install methods, FOMOD, BAIN
- [Wabbajack modlists](../guides/wabbajack.md) — modlists, directives, archive sources
- [Nexus Mods](../guides/nexus.md) — Collections, `nxm://`, browse/search
- [Tools & overlays](../guides/tools.md) — OptiScaler, Proton, Wine DLL overrides
- [Generic & user-defined games](../games/generic-games.md) — GamePlugin vs generic games
- [MO2 parity & capability audit](parity.md) — the `Done` / `Partial` status of every feature
- [CLI reference](cli.md) — every command and flag
