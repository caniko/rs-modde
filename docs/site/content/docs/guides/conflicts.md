+++
title = "Conflicts & Load Order"
description = "Understand and resolve mod conflicts and manage Bethesda plugin load order"
weight = 45
+++

## Mod conflicts

When multiple mods provide the same file, only one version can win. modde resolves this by **load order priority**: the mod later in the list overwrites earlier ones.

### Analysing conflicts

```bash
# Show critical and major conflicts
modde collisions --profile my-skyrim

# Include cosmetic conflicts too
modde collisions --profile my-skyrim --all
```

The report shows:

- **Collision pairs**: Which mods conflict, with severity (`CRITICAL`, `MAJOR`, `COSMETIC`) and file count
- **Per-file details**: File path, loser vs winner, and whether the file is loose or from an archive
- **Shadowed mods**: Mods where every file is overridden by another mod
- **Redundant files**: Files that never win (always overridden)

### Severity levels

| Level    | Meaning                  | Examples                             |
| -------- | ------------------------ | ------------------------------------ |
| Critical | Will cause game issues   | BSA/BA2 overwrites, plugin conflicts |
| Major    | Likely visible problems  | Texture and mesh conflicts           |
| Cosmetic | Minor visual differences | Optional textures, minor overlaps    |

### Suggesting hides

To get actionable commands for hiding redundant files:

```bash
modde collisions --profile my-skyrim --suggest-hides
```

This outputs hide commands you can run to suppress files that are always overridden.

## Per-file hiding

Hide specific files from a mod without removing the entire mod:

```bash
modde profile hide "mod_id" "path/to/file.nif"
```

Hidden files are excluded during the VFS build phase. This is the equivalent of MO2's `.mohidden` system.

## Bethesda plugin load order

For Bethesda games, plugin load order (`.esp`, `.esm`, `.esl`) is managed separately from mod install priority.

### LOOT sorting

Sort plugins using the LOOT masterlist:

```bash
modde loot sort --game skyrim-se
```

This applies community-maintained sorting rules to produce a stable plugin order.

### Validating plugins

Check for common plugin issues:

```bash
modde loot validate --game skyrim-se
```

Detects:

- **Form 43 plugins**: Old Skyrim (LE) format plugins in SSE — causes crashes
- **Missing masters**: Plugins that require other plugins not in your load order

### Plugin order backup

Before making load order changes:

```bash
# Save current plugin order
modde backup plugins --profile my-skyrim --game skyrim-se

# Restore if something goes wrong
modde backup restore-plugins --profile my-skyrim --game skyrim-se
```

## Diagnostics

Run a comprehensive diagnostic check for common modding issues:

```bash
modde diagnostics --game skyrim-se --profile my-skyrim
```

Issues are reported with severity (`ERROR`, `WARN`, `INFO`), a description, the affected mod, and a fix suggestion when available.
