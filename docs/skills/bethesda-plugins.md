# Bethesda Plugin Management

## Overview

modde has deep Bethesda-specific support: plugin load order management, LOOT masterlist integration for automatic sorting, binary plugin header parsing for validation, BSA/BA2 archive awareness, and per-profile INI management. Compared to MO2, the core plugin pipeline is strong, but archive conflict detection, the "Archives" tab, plugin lock, and load order backup/restore are missing.

## Architecture

### Plugin Load Order

**Key file:** `crates/modde-games/src/bethesda/plugins_txt.rs`

Reads/writes the standard Bethesda `plugins.txt` format:
- `*PluginName.esp` = enabled
- `PluginName.esp` = disabled
- Lines starting with `#` = comments

Proton path resolution:
```
~/.local/share/Steam/steamapps/compatdata/<APP_ID>/pfx/drive_c/users/steamuser/AppData/Local/<GAME_FOLDER>/plugins.txt
```

### Dual-Pane Plugin Order (Independent of Mod Priority)

**Key file:** `crates/modde-core/src/db.rs` — `plugin_order` table

```sql
plugin_order(profile_id, plugin_name, sort_index, enabled)
```

DB methods: `set_plugin_order()`, `get_plugin_order()`, `toggle_plugin()`

This separates "which mod's files win" (mod install priority) from "which .esp loads first" (plugin load order) — MO2's core dual-pane insight.

### LOOT Masterlist Integration

**Key file:** `crates/modde-games/src/bethesda/loot.rs`

Pure Rust LOOT masterlist parser (no FFI to libloot needed):

1. Parses LOOT's public YAML masterlist format
2. Handles both simple string (`"Skyrim.esm"`) and complex object (`{name: "Skyrim.esm"}`) file references
3. Extracts `after`, `requires`, `incompatible` rules per plugin
4. `rules_for_plugins(active_plugins)` generates modde `LoadOrderRule` variants, only for plugins in the active set
5. Case-insensitive matching throughout

Masterlist URLs:
| Game | Repository |
|------|-----------|
| Skyrim SE/AE | `loot/skyrimse` |
| Fallout 4 | `loot/fallout4` |
| Fallout 76 | `loot/fallout76` |

CLI: `modde loot sort --game skyrim-se`

### Plugin Header Validation

**Key file:** `crates/modde-games/src/bethesda/plugin_header.rs`

Binary parser that reads only the first ~1KB of each plugin:

**TES4 Record Header (24 bytes):**
```
[4] Signature ("TES4")
[4] Data size
[4] Record flags (ESM=0x01, ESL=0x200)
[4] Form ID
[4] Revision
[2] Version
[2] Unknown
```

**Extracted Data:**
- `version` — Form 43 (0.94, Oldrim) vs Form 44 (1.70, SSE)
- `record_flags` — ESM/ESL flags
- `masters` — MAST sub-records listing required master files

**Validation Warnings:**
- `Form43` — Oldrim-format plugin in SSE (causes CTDs)
- `MissingMaster` — Required master not in active load order (crash on load)

CLI: `modde loot validate --game skyrim-se`

### BSA/BA2 Archive Handling

**Key file:** `crates/modde-games/src/bethesda/archives.rs`

Archives are deployed as-is (not extracted) since Bethesda engines load them natively:
- `is_archive(path)` — checks `.bsa`/`.ba2` extension
- `staging_path()` — places archives directly into Data directory

### Mod Safety Classification

Extension-based classification for Bethesda games:

| Category | Extensions |
|----------|-----------|
| Save-breaking | `.esp`, `.esm`, `.esl`, `.pex`, `.dll`, `.psc` |
| Cosmetic | `.nif`, `.bsa`, `.ba2`, `.dds`, `.png`, `.tga`, `.jpg`, `.hkx`, `.fuz`, `.wav`, `.xwm`, `.swf`, `.ini`, `.json` |

### Game Instances

| Game ID | Steam App ID | My Games Dir |
|---------|-------------|-------------|
| `skyrim-se` | 489830 | Skyrim Special Edition |
| `skyrim-ae` | 489830 | Skyrim Special Edition |
| `fallout4` | 377160 | Fallout4 |
| `fallout76` | 1151340 | Fallout 76 |

## MO2 Feature Comparison

| MO2 Feature | modde | Notes |
|-------------|-------|-------|
| Plugin load order management | **Done** | plugins.txt read/write + DB |
| Dual-pane (mod priority vs plugin order) | **Done** | Separate `plugin_order` table |
| LOOT integration (auto-sort) | **Done** | Pure Rust masterlist parser |
| LOOT sorting reports | Partial | Rules printed to terminal, no rich report |
| Form 43 detection | **Done** | Binary header parsing |
| Missing master detection | **Done** | Header master list vs active plugins |
| ESM/ESL flag detection | **Done** | Record flags parsing |
| BSA/BA2 archive deploy | **Done** | Archives placed as-is into Data/ |
| Plugin list columns (priority, mod index, form version, author) | -- | MO2 has 8 columns in plugin list |
| BSA/BA2 conflict detection (files inside archives) | -- | Only loose file conflicts tracked |
| BSA/BA2 content preview/browser | -- | MO2 can list files inside archives |
| BSA/BA2 packing tool | -- | MO2 has archive packer |
| Archives tab (BSA load order management) | -- | MO2 has a dedicated Archives right-pane tab |
| Archive invalidation (auto-generate) | -- | MO2 generates invalidation files per-profile |
| BSA back-dating (force loose wins) | -- | MO2 changes archive timestamps |
| "Lock load order" for plugins | -- | MO2 can pin plugins to resist LOOT re-sorting |
| Plugin load order backup/restore | -- | MO2 can snapshot and restore plugin order |
| Optional ESPs (move to optional/ subfolder) | -- | MO2's mod info dialog has Optional ESPs tab |
| Per-mod INI tweaks (INI Files tab in mod info) | -- | MO2 edits .ini files bundled inside mods |
| Problems/diagnostics system | -- | MO2 has plugin-based diagnostic with guided auto-fixes |
