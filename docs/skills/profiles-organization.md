# Profile & Organization System

## Overview

modde's profile system is SQLite-backed with organization features: categories, tags, notes, and an experiment/try mode for non-destructive profile testing. Compared to MO2, modde has stronger save integration but lacks some of MO2's UI polish (filter modes, group-by, CSV export, per-row color coding).

## Architecture

### Profile Structure

```rust
Profile {
    name: String,
    game_id: GameId,
    source: ProfileSource,        // Manual | NexusCollection | Wabbajack
    mods: Vec<EnabledMod>,        // Ordered by install priority
    overrides: PathBuf,           // Profile-specific file overrides
    load_order_rules: SmallVec<[LoadOrderRule; 4]>,
}
```

### EnabledMod Fields

| Field | Type | Purpose |
|-------|------|---------|
| `mod_id` | String | Unique identifier |
| `enabled` | bool | Toggle without removing |
| `version` | Option<String> | Installed version |
| `fomod_config` | Option<String> | Stored FOMOD selections |
| `nexus_mod_id` | Option<i64> | Nexus tracking for updates |
| `nexus_file_id` | Option<i64> | Nexus file tracking |
| `nexus_game_domain` | Option<String> | Game domain for API |
| `installed_timestamp` | Option<i64> | For update comparison |
| `category_id` | Option<i64> | Category assignment |
| `notes` | Option<String> | User notes |
| `tags` | Option<String> | JSON array of tags |

### Key Files

| File | Purpose |
|------|---------|
| `crates/modde-core/src/profile/mod.rs` | Profile, EnabledMod, ProfileManager |
| `crates/modde-core/src/db.rs` | SQLite persistence, CRUD, categories |
| `crates/modde-core/src/resolver/mod.rs` | Load order DAG, ConflictMap |
| `crates/modde-cli/src/commands/profile.rs` | CLI profile commands |

### Experiment Mode (Try/Rollback/Commit)

Unique to modde — a stackable experiment system:

```
modde profile try "experimental" --game skyrim-se    # push current onto stack
modde profile try "even-more-mods" --game skyrim-se  # stack another
modde profile rollback --game skyrim-se              # pop back to "experimental"
modde profile rollback --game skyrim-se              # pop back to original
modde profile commit --game skyrim-se                # clear stack, keep current
```

Each try/rollback automatically swaps saves.

### Categories & Organization

**Schema (V2):**
- `mod_categories(id, profile_id, name, color, sort_index)` — collapsible groups
- `profile_mods.category_id` — assign mods to categories
- `profile_mods.notes` — per-mod notes
- `profile_mods.tags` — JSON array of tag strings

**DB Methods:**
- `create_category()`, `update_category()`, `delete_category()`, `list_categories()`
- `set_mod_category()`, `set_mod_notes()`, `set_mod_tags()`

### Dual-Pane: Mod Priority vs Plugin Order

**Mod priority** (install order — which files win conflicts) is determined by position in `profile_mods.sort_index`.

**Plugin order** (which .esp/.esm/.esl loads in the engine) is independently stored in `plugin_order(profile_id, plugin_name, sort_index, enabled)`.

This separation mirrors MO2's left-pane (mod priority) vs right-pane (plugin order) design.

### Per-Profile INI Management

Each profile stores its own copy of game INI files:

```
<profiles_dir>/<name>/ini/
    Skyrim.ini
    SkyrimPrefs.ini
    SkyrimCustom.ini
```

On profile activation, INIs are swapped automatically alongside saves.

**Key file:** `crates/modde-games/src/bethesda/ini_profiles.rs`

## MO2 Feature Comparison

| MO2 Feature | modde | Notes |
|-------------|-------|-------|
| Profile system with mod lists | **Done** | SQLite-backed, richer metadata |
| Profile switching | **Done** | Save-aware with fingerprint tracking |
| Independent mod priority ordering | **Done** | `sort_index` in DB |
| Per-profile INI files | **Done** | `ini_profiles.rs`, auto-swap |
| Per-profile save games | **Done** | Git-backed vault with auto-swap |
| Experiment/try mode | **Done** | Stackable — unique to modde |
| Profile forking | **Done** | Clone mods + rules + saves |
| Mod categories | **Done** | DB-backed with colors |
| Collapsible separators with aggregated child info | -- | MO2 separators collapse/expand and show combined flags |
| Separator scrollbar markers | -- | MO2 draws colored marks on scrollbar at separator positions |
| Per-mod notes | **Done** | `notes` field on EnabledMod |
| Per-mod tags | **Done** | JSON array in `tags` field |
| Per-row notes color | -- | MO2 lets you color-code individual mod rows via Notes column |
| "Send to..." priority system | -- | MO2: send to separator, first/last conflict, specific priority |
| Three-state filters (show/hide/only) | -- | DB supports it, UI not wired |
| AND/OR filter logic | -- | MO2 supports AND/OR mode across filter criteria |
| Inverted filters | -- | MO2 can invert each filter criterion independently |
| Group-by modes (category, Nexus ID) | -- | MO2 mod list can be grouped by category or shared Nexus ID |
| Content type icons per mod | -- | MO2 shows icons for textures, meshes, scripts, etc. in each mod |
| Flags column (invalid, backup, foreign, etc.) | -- | MO2 shows ~12 status flag icons per mod |
| Mod author / uploader columns | -- | MO2 tracks author and uploader separately |
| Install time column | -- | MO2 records when each mod was installed |
| CSV export | -- | MO2 exports configurable columns/rows |
| "Mark as converted/working" | -- | MO2 dismisses alternate-game warnings for ported mods |
| "Ignore missing data" | -- | MO2 dismisses invalid-data warnings |
| Mod backup/restore | -- | MO2 backs up individual mod directories, restores on demand |
| Reinstall mod from original archive | -- | MO2 re-runs the installer from the stored archive |
| Compact/detailed list views | -- | MO2 has toggle for denser list display |
| Keyboard shortcuts (A-Z jump, F2 rename, etc.) | -- | MO2 has extensive mod list keyboard shortcuts |
