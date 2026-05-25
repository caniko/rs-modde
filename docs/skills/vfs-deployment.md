# VFS & Deployment System

## Overview

modde uses a **symlink-farm VFS** to deploy mods without modifying the original game directory. This is the Linux-native equivalent of MO2's USVFS (User-Space Virtual Filesystem), but with a critical advantage: symlinks are globally visible to all processes, so external tools see the modded game state without process-level API hooking.

## Architecture

### Typestate Pattern (Compile-Time Safety)

The deployment pipeline enforces a strict `Built -> Materialized -> Deployed` sequence at compile time via Rust's typestate pattern:

```
SymlinkFarm<Built>       -- link map computed in memory
    .materialize()  ->
SymlinkFarm<Materialized> -- symlinks written to staging dir
    .deploy_to()    ->
    (symlinks in game directory)
```

You cannot call `deploy_to()` on a `Built` farm — the compiler prevents it.

### Key Files

| File                                      | Purpose                                         |
| ----------------------------------------- | ----------------------------------------------- |
| `crates/modde-core/src/vfs/mod.rs`        | SymlinkFarm typestate, build/materialize/deploy |
| `crates/modde-cli/src/commands/deploy.rs` | CLI deploy orchestration                        |
| `crates/modde-core/src/fs.rs`             | `symlink_async`, `walk_files_relative`          |

### Build Phase

`SymlinkFarm::build()` takes:

- `profile_name` — determines staging directory location
- `resolved: &ResolvedLoadOrder` — mods in priority order (last wins)
- `mod_files: HashMap<ModId, Vec<(rel_path, source_path)>>` — per-mod file listings
- `overrides: Option<&[(rel_path, source_path)]>` — profile overrides (always win)
- `hidden: Option<&HashSet<(mod_id, rel_path)>>` — files to exclude from deployment

### Conflict Resolution

When multiple mods provide the same file:

1. **Mod priority** (position in load order) determines the default winner — later mods win
2. **Hidden files** are excluded per `(mod_id, rel_path)` — if the winner is hidden, the next-highest-priority provider wins
3. **Profile overrides** always win over all mods

### Atomic Rollback

- Before each deploy, the previous staging dir is preserved as `staging.bak`
- `modde rollback` atomically swaps `staging` and `staging.bak` via rename

### Overwrite Capture (Tool Launcher)

When external tools write new files into the game directory:

1. `modde tool run` snapshots the mod directory before execution
2. Runs the tool with the game install dir as working directory
3. Diffs post-execution to find new files
4. Moves new files to `__overwrite__` mod in the store

### Data Tab / Virtual Filesystem Browser

MO2 has a "Data" tab showing the merged virtual filesystem as the game will see it, with filters for conflicts-only, archives, and hidden files. modde does not yet have this merged-view browser.

## MO2 Feature Comparison

| MO2 Feature                                     | modde    | Notes                                                      |
| ----------------------------------------------- | -------- | ---------------------------------------------------------- |
| Virtual filesystem (clean game folder)          | **Done** | Symlink farm instead of USVFS                              |
| Per-mod isolated directories                    | **Done** | Content store at `~/.local/share/modde/store/`             |
| Per-file conflict visualization                 | **Done** | `ConflictMap` + `resolved_conflicts()`                     |
| Per-file hiding (.mohidden)                     | **Done** | `hidden_files` DB table + VFS exclusion                    |
| Batch unhide all hidden files                   | --       | MO2 has "Restore hidden files" context action              |
| Overwrite folder for tool output                | **Done** | `modde tool run` with diff capture                         |
| Per-tool output mod (not just **overwrite**)    | --       | MO2 lets each executable target a specific mod             |
| Overwrite management (sync to mods, create mod) | --       | MO2 has Sync to Mods/Create Mod/Clear actions              |
| Atomic rollback                                 | **Done** | staging.bak swap                                           |
| Cross-process visibility                        | **Done** | Symlinks visible to all processes (advantage over MO2)     |
| Tool launcher through VFS                       | **Done** | `modde tool run`, `modde tool list`                        |
| Data tab (merged VFS browser)                   | --       | MO2 shows merged view with conflict/archive/hidden filters |
| Automatic archive invalidation                  | --       | MO2 generates BSA invalidation files per-profile           |
| BSA back-dating                                 | --       | MO2 changes BSA timestamps so loose files always win       |
