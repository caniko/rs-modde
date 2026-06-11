# Conflicts & Load Order

## Two different kinds of ordering

Modding has two independent ordering systems, and conflating them is the single
most common source of confusion:

| Concept                 | What it orders                          | Who resolves it                              |
| ----------------------- | --------------------------------------- | -------------------------------------------- |
| **Mod install priority** | Which file wins when two mods ship the same path | The VFS — *later mod in the list overrides earlier* |
| **Plugin load order**    | The order Bethesda `.esp`/`.esm`/`.esl` plugins load | LOOT + `plugins.txt` — a separate axis        |

Install priority decides what lands on disk (textures, meshes, scripts, configs).
Plugin load order decides how the game engine merges plugin *records* at runtime.
A mod can win the file fight and still need its plugin sorted correctly — these
are orthogonal. The first half of this page is install-priority conflicts; the
[second half](#bethesda-plugin-load-order) is Bethesda plugin order.

## Mod conflicts (install priority)

When multiple enabled mods provide the same relative path, only one version can be
deployed. modde resolves this by **load order priority**: walking the resolved
order, the last (highest-priority) provider of a path wins. Hidden files and
profile overrides shift the winner, exactly as in
[deployment](deployment.md#how-files-map-to-winners).

### Analysing conflicts

```bash
# Show config and dangerous collisions
modde collisions --profile my-skyrim

# Include cosmetic collisions too
modde collisions --profile my-skyrim --all
```

By default, pairs whose worst severity is purely **cosmetic** are hidden; `--all`
shows them. The report contains:

- **Collision pairs** — `[SEVERITY] loser vs winner (N files)`, each followed by
  per-file lines
- **Per-file detail** — `path -> winner: <mod>` with origin/hidden tags
- **Shadowed mods** — mods whose every file is overridden by higher-priority mods
- **Loose vs archive conflicts** — a loose file overriding archive contents (or
  vice versa)
- **Redundant files** — files that never win (always overridden)

### Collision severity

Severity is classified per file from its extension by the game's classifier. The
four levels, lowest to highest risk:

| Severity      | Display      | Meaning                                              | Typical extensions                        |
| ------------- | ------------ | ---------------------------------------------------- | ----------------------------------------- |
| **Cosmetic**  | `COSMETIC`   | Visual/audio only; low risk                          | `dds`, `nif`, `png`, `wav`, `hkx`, `bsa`  |
| **Config**    | `CONFIG`     | May change behaviour                                 | `ini`, `cfg`, `json`, `toml`, `xml`       |
| **Dangerous** | `DANGEROUS`  | Scripts/plugins/DLLs; crashes or save corruption     | `esp`, `esm`, `esl`, `dll`, `pex`, `psc`  |
| **Unknown**   | `UNKNOWN`    | Extension not in the game's table                    | anything unrecognised                     |

A mod pair's reported severity is the **worst** severity among its colliding
files. Pairs are sorted most-severe first, then by file count.

### Per-game classifiers

Each game supplies its own classifier — both the severity table *and* which
extensions count as indexable archives:

| Game family            | Archive extensions      | Notable Dangerous extensions             |
| ---------------------- | ----------------------- | ---------------------------------------- |
| Bethesda (Skyrim, Fallout, Starfield) | `bsa`, `ba2` (indexed)  | `esp`, `esm`, `esl`, `dll`, `pex`, `psc` |
| Cyberpunk 2077         | `archive` (not indexed) | `reds`, `lua`, `tweak`, `xl`, `dll`, `yaml` |
| UE4/UE5 titles         | `pak`, `ucas`, `utoc`   | `pak`, `ucas`, `utoc`, `dll`, `lua`      |

Note that for Bethesda games the archive container extensions (`bsa`, `ba2`)
classify as *cosmetic* — the danger is judged by what is *inside* the archive,
not the wrapper.

### Archive-aware conflicts (BSA / BA2 / pak)

modde does not stop at loose files. For each mod it walks loose files **and**, for
any file whose extension is an archive extension, asks the classifier to index the
archive's contents. Every entry inside the archive is registered into the conflict
map under its normalised internal path, so a loose `textures/sky.dds` in one mod
correctly collides with a `textures/sky.dds` packed inside another mod's `.bsa`.

When a conflict spans a loose file and an archived one, the per-file line is
tagged so you can see which form won:

- ` [loose > archive]` — a loose file beat an archived one
- ` [archive > loose]` — an archived file beat a loose one

These are also collected separately under **Loose vs archive conflicts** in the
report.

Indexing support is **classifier-specific and not uniform**:

- **Bethesda** `.bsa`/`.ba2` are read and their contents indexed.
- **Cyberpunk** `.archive` is listed as an archive extension but its proprietary
  format is **not yet indexed** — Cyberpunk conflicts are detected at the loose
  file level only.
- If an archive fails to index, modde warns and treats it as contributing no
  internal entries (it still counts as a loose-file collision on the archive
  itself).

### Shadowed mods and redundant files

Two pre-deploy optimisation signals fall out of the analysis:

- A **shadowed mod** is one where *every* file it provides is overridden by a
  higher-priority mod. It contributes nothing to the deployed VFS. The report
  lists it as `"<mod>" (N files, all overridden by <mods>)`; a diagnostic also
  suggests disabling it to shrink the deployment.
- A **redundant file** is a single path from a mod that always loses. The count
  is reported, and `--suggest-hides` turns each into an actionable hide command.

### Suggesting hides

```bash
modde collisions --profile my-skyrim --suggest-hides
```

For every redundant file this prints a ready-to-run command, e.g.:

```text
Suggested hide commands:
  modde profile hide "SomeTextureMod" "textures/landscape/dirt.dds"
```

Without `--suggest-hides`, the report just notes how many redundant files exist
and reminds you of the flag.

### Reading the output

A trimmed `modde collisions --profile my-skyrim` run might look like:

```text
Collision Report for profile "my-skyrim" (skyrim-se)
============================================================
7 file collisions across 3 mod pairs

[DANGEROUS] CombatOverhaulB vs CombatOverhaulA (1 files)
  scripts/combat.pex  ->  winner: CombatOverhaulA

[CONFIG] BaseINI vs TweakedINI (1 files)
  SkyrimPrefs.ini  ->  winner: TweakedINI

[COSMETIC] HiResTextures vs CustomSky (2 files)
  textures/sky/clouds.dds  ->  winner: CustomSky [archive > loose]
  textures/sky/stars.dds   ->  winner: CustomSky (hidden)

Shadowed mods (all files overridden):
  - "OldTexturePack" (2 files, all overridden by HiResTextures, CustomSky)

Loose vs archive conflicts: 1 files

Redundant files (never win): 3
  Run with --suggest-hides to get hide commands
```

How to read it:

- The header line reports total file-level collisions and how many mod pairs they
  span.
- `[DANGEROUS] CombatOverhaulB vs CombatOverhaulA` — the **second** name is the
  winner. Here a `.pex` script collides, which is high-risk, so verify this is
  intentional.
- `[archive > loose]` on `clouds.dds` means `CustomSky` won via a packed archive
  entry over a loose file in `HiResTextures`.
- `(hidden)` on `stars.dds` means the loser was explicitly hidden by you.
- `OldTexturePack` is fully shadowed — disabling it changes nothing on disk.

## Per-file hiding

Hide a single file from a mod without disabling the whole mod:

```bash
modde profile hide "mod_id" "path/to/file.nif"
```

Hidden files are recorded per profile and excluded during the VFS
[build phase](deployment.md#how-files-map-to-winners): the next-highest provider
of that path wins instead. This is modde's equivalent of MO2's `.mohidden`
system. In the collision report, a hidden loser is tagged `(hidden)` so you can
confirm the override took effect.

## Bethesda plugin load order

For Bethesda games, plugin load order (`.esp`, `.esm`, `.esl`) is managed
**separately** from mod install priority. These commands read the game's real
`plugins.txt` for the active plugin set.

### LOOT sorting

```bash
modde loot sort --game skyrim-se
```

modde parses the community **LOOT masterlist** (a YAML file of per-plugin
`after` / `requires` / `incompatible` rules) and derives load-order rules for the
plugins you actually have active:

- `after` and `requires` both become **load-after** rules (a `requires` whose
  target is not in the active set is logged as a dependency gap).
- `incompatible` becomes an **incompatible** rule.
- Rules referencing plugins that are not active are dropped.

The masterlist must be cached locally first; if it is missing, `sort` prints the
exact `curl` command (and target path) to fetch the right masterlist for your
game. Masterlists are available for `skyrim-se`/`skyrim-ae`, `fallout4`,
`fallout76`, and `starfield`. `sort` prints the generated rules
(`X loads after Y`, `INCOMPATIBLE: A <-> B`) so you can review what the
masterlist implies for your setup.

### Validating plugins

```bash
modde loot validate --game skyrim-se
```

modde reads only the first ~1 KB of each active plugin's TES4 header — it never
loads multi-GB plugins in full — and reports:

- **Form 43 plugins** — `[FORM43] <plugin> (vX.XX) — Oldrim format, may cause
  CTDs in SSE`. Form 43 (version `0.94`) is the old Skyrim LE record format;
  using it in SSE/AE (which expects Form 44, `1.70`) can crash the game. The fix
  is to resave the plugin in the Creation Kit. This check only runs for
  `skyrim-se` / `skyrim-ae`.
- **Missing masters** — `[MISSING] <plugin> requires '<master>' which is not
  loaded`. A plugin depends on a master that is not in the active load order; the
  game will crash on load. Matching is case-insensitive.

A clean run prints `All N plugins are valid.`

### Plugin order backup and restore

Take a restore point before reordering, and roll back if a sort goes wrong:

```bash
# Save the current plugin order
modde backup plugins --profile my-skyrim --game skyrim-se

# Restore the saved order
modde backup restore-plugins --profile my-skyrim --game skyrim-se
```

## Diagnostics

`modde doctor profile` runs a rule engine over the fully analysed profile — resolved
load order, the archive-aware conflict map, and the collision report together:

```bash
modde doctor profile --game skyrim-se --profile my-skyrim
```

Each finding carries a severity (`ERROR`, `WARN`, `INFO`), a title and detail, the
affected mod (when applicable), and a suggested fix. Findings are sorted with
errors first. Built-in rules include:

- **Empty / missing store mod** (`WARN`) — an enabled mod with no files in the
  store; suggests re-installing or disabling it.
- **Completely shadowed mod** (`WARN`) — every file is overridden; suggests
  disabling to shrink the deploy.
- **Dangerous collision** (`WARN`) — a mod pair whose worst collision is a
  script/plugin/DLL file; suggests reviewing that the winner is intentional.
- **Bethesda plugin rules** — Form 43 and missing-master findings surfaced as
  diagnostics for Bethesda games.

Diagnostics are advisory: they never change your load order or files on their own.

## See also

- [Deployment & VFS](deployment.md) — how the winning file is projected on disk
- [Profiles](profiles.md) — enabling/disabling mods, overrides, hidden files
- [Wabbajack](wabbajack.md) — modlists ship a pre-resolved layout
- [Troubleshooting](troubleshooting.md) — CTDs, missing masters, broken links
- [Parity reference](../reference/parity.md) — conflict tooling coverage vs MO2
- [Supported games](../games/supported-games.md) — which games ship a classifier
