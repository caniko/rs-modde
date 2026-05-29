use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use smallvec::SmallVec;
use tracing::info;

use crate::error::{CoreError, Result};
use crate::installer::{InstallMethod, InstallPlan, InstallStatus, StagedFile};
use crate::nexus_id::{NexusFileId, NexusIdError, NexusModId};
use crate::profile::{EnabledMod, LoadOrderLock, LockReason, Profile, ProfileSource};
use crate::resolver::{GameId, LoadOrderRule, ModId};

const CURRENT_SCHEMA_VERSION: u32 = 10;

const SCHEMA_V1: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS profiles (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    game_id     TEXT NOT NULL,
    source_type TEXT NOT NULL DEFAULT 'manual',
    source_data TEXT,
    overrides   TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(name, game_id)
);

CREATE TABLE IF NOT EXISTS profile_mods (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    mod_id      TEXT NOT NULL,
    enabled     INTEGER NOT NULL DEFAULT 1,
    version     TEXT,
    fomod_config TEXT,
    sort_index  INTEGER NOT NULL,
    UNIQUE(profile_id, mod_id)
);

CREATE TABLE IF NOT EXISTS load_order_rules (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    rule_type   TEXT NOT NULL,
    mod_a       TEXT NOT NULL,
    mod_b       TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS saves (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    path        TEXT NOT NULL UNIQUE,
    label       TEXT,
    assigned_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS stock_snapshots (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id     TEXT NOT NULL UNIQUE,
    snapshot_path TEXT NOT NULL,
    tree_hash   TEXT NOT NULL,
    file_count  INTEGER NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS active_profiles (
    game_id     TEXT PRIMARY KEY,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    activated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS experiment_stack (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id     TEXT NOT NULL,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    depth       INTEGER NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_profiles_game ON profiles(game_id);
CREATE INDEX IF NOT EXISTS idx_mods_profile ON profile_mods(profile_id);
CREATE INDEX IF NOT EXISTS idx_rules_profile ON load_order_rules(profile_id);
CREATE INDEX IF NOT EXISTS idx_saves_profile ON saves(profile_id);
CREATE INDEX IF NOT EXISTS idx_experiment_game ON experiment_stack(game_id, depth);
";

const SCHEMA_V2: &str = "
-- Per-file hiding (MO2-style .mohidden equivalent)
CREATE TABLE IF NOT EXISTS hidden_files (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    mod_id     TEXT NOT NULL,
    rel_path   TEXT NOT NULL,
    hidden_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(profile_id, mod_id, rel_path)
);

-- Independent plugin ordering (separate from mod install priority)
CREATE TABLE IF NOT EXISTS plugin_order (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    plugin_name TEXT NOT NULL,
    sort_index  INTEGER NOT NULL,
    enabled     INTEGER NOT NULL DEFAULT 1,
    UNIQUE(profile_id, plugin_name)
);

-- Mod categories with collapsible separators
CREATE TABLE IF NOT EXISTS mod_categories (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    color      TEXT,
    sort_index INTEGER NOT NULL,
    UNIQUE(profile_id, name)
);

CREATE INDEX IF NOT EXISTS idx_hidden_profile ON hidden_files(profile_id);
CREATE INDEX IF NOT EXISTS idx_plugin_order_profile ON plugin_order(profile_id);
CREATE INDEX IF NOT EXISTS idx_categories_profile ON mod_categories(profile_id);
";

const SCHEMA_V3: &str = "
-- Per-game tool/overlay configurations (MangoHud, vkBasalt, GameMode, etc.)
CREATE TABLE IF NOT EXISTS game_tools (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id     TEXT NOT NULL,
    tool_id     TEXT NOT NULL,
    enabled     INTEGER NOT NULL DEFAULT 0,
    settings    TEXT NOT NULL DEFAULT '{}',
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(game_id, tool_id)
);

-- Files applied by tools to game directories (for revert tracking)
CREATE TABLE IF NOT EXISTS tool_applied_files (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id     TEXT NOT NULL,
    tool_id     TEXT NOT NULL,
    rel_path    TEXT NOT NULL,
    applied_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(game_id, tool_id, rel_path)
);

CREATE INDEX IF NOT EXISTS idx_game_tools_game ON game_tools(game_id);
CREATE INDEX IF NOT EXISTS idx_tool_files_game ON tool_applied_files(game_id, tool_id);
";

// Schema V8 adds the installer pipeline's state:
//
// * Three new columns on `profile_mods` capturing how a mod was installed:
//   `install_method` (TOML-serialized `InstallMethod`), `source_archive_hash`
//   (xxh64 of the downloaded archive), and `install_status` (one of
//   `installed | unknown | pending_user_input | failed`).
//
// * A new `installed_mod_files` table that records the concrete file
//   manifest for every installed mod so uninstall can remove exactly the
//   files it staged — no orphaned files, no collateral damage. The
//   `merge_group` column is reserved for the future script-merge feature;
//   it is written but not read by current code.
const SCHEMA_V8: &str = "
CREATE TABLE IF NOT EXISTS installed_mod_files (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id          INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    mod_id              TEXT NOT NULL,
    rel_path            TEXT NOT NULL,
    origin_rel_path     TEXT NOT NULL,
    size                INTEGER NOT NULL,
    merge_group         TEXT,
    UNIQUE(profile_id, mod_id, rel_path)
);

CREATE INDEX IF NOT EXISTS idx_imf_profile_mod ON installed_mod_files(profile_id, mod_id);
CREATE INDEX IF NOT EXISTS idx_imf_merge_group ON installed_mod_files(merge_group)
    WHERE merge_group IS NOT NULL;
";

// Schema V9 adds MO2-style executable definitions. These are distinct
// from `game_tools`: tools model optional overlays/config patchers, while
// executables model named launch targets such as xEdit, BodySlide, Nemesis,
// or a game-specific helper with persistent args/environment/output routing.
const SCHEMA_V9: &str = "
CREATE TABLE IF NOT EXISTS executable_configs (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id             TEXT NOT NULL,
    name                TEXT NOT NULL,
    executable_path     TEXT NOT NULL,
    arguments           TEXT NOT NULL DEFAULT '[]',
    working_dir         TEXT,
    environment         TEXT NOT NULL DEFAULT '{}',
    wine_dll_overrides  TEXT,
    output_mod          TEXT NOT NULL DEFAULT '__overwrite__',
    enabled             INTEGER NOT NULL DEFAULT 1,
    updated_at          TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(game_id, name)
);

CREATE INDEX IF NOT EXISTS idx_executable_configs_game ON executable_configs(game_id);
";

// Schema V10 adds a generalized DAG of game tool settings. `game_tools`
// remains the current-state table; each mutation also appends a node and an
// edge from the previous current node, allowing restore operations to branch.
const SCHEMA_V10: &str = "
CREATE TABLE IF NOT EXISTS tool_setting_nodes (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id     TEXT NOT NULL UNIQUE,
    game_id     TEXT NOT NULL,
    tool_id     TEXT NOT NULL,
    enabled     INTEGER NOT NULL,
    settings    TEXT NOT NULL,
    reason      TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS tool_setting_edges (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_node_id  TEXT NOT NULL,
    child_node_id   TEXT NOT NULL,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(parent_node_id, child_node_id)
);

CREATE INDEX IF NOT EXISTS idx_tool_setting_nodes_tool
    ON tool_setting_nodes(game_id, tool_id, created_at);
CREATE INDEX IF NOT EXISTS idx_tool_setting_edges_child
    ON tool_setting_edges(child_node_id);
";

/// Summary view of a profile (without loading all mods).
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileSummary {
    pub id: i64,
    pub name: String,
    pub game_id: GameId,
    pub mod_count: usize,
    pub source_type: String,
}

/// A save file/directory assigned to a profile.
#[derive(Debug, Clone)]
pub struct SaveEntry {
    pub path: PathBuf,
    pub label: Option<String>,
    pub assigned_at: String,
}

/// Metadata for a stock game snapshot stored in the database.
#[derive(Debug, Clone)]
pub struct SnapshotMeta {
    pub game_id: GameId,
    pub snapshot_path: PathBuf,
    pub tree_hash: String,
    pub file_count: usize,
    pub created_at: String,
}

/// A hidden file entry — prevents a specific file from a mod from being deployed.
#[derive(Debug, Clone)]
pub struct HiddenFile {
    pub mod_id: String,
    pub rel_path: String,
}

/// A plugin entry in the plugin load order (independent of mod install priority).
#[derive(Debug, Clone)]
pub struct PluginEntry {
    pub plugin_name: String,
    pub sort_index: i64,
    pub enabled: bool,
}

/// A mod category for organizing the mod list.
#[derive(Debug, Clone)]
pub struct ModCategory {
    pub id: Option<i64>,
    pub name: String,
    pub color: Option<String>,
    pub sort_index: i64,
}

/// SQLite-backed persistent storage for modde.
pub struct ModdeDb {
    conn: Connection,
}

impl ModdeDb {
    /// Open the database at the default XDG path, creating it if needed.
    pub fn open() -> Result<Self> {
        let path = crate::paths::db_path();
        Self::open_at(&path)
    }

    /// Open the database at a specific path (useful for testing).
    pub fn open_at(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Open an in-memory database (for testing).
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let version: u32 = self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?;

        if version < 1 {
            self.conn.execute_batch(SCHEMA_V1)?;
            info!(from = version, to = 1, "database schema migrated to V1");
        }

        if version < 2 {
            self.conn.execute_batch(SCHEMA_V2)?;
            self.add_column_if_missing("profile_mods", "nexus_mod_id", "INTEGER")?;
            self.add_column_if_missing("profile_mods", "nexus_file_id", "INTEGER")?;
            self.add_column_if_missing("profile_mods", "nexus_game_domain", "TEXT")?;
            self.add_column_if_missing("profile_mods", "installed_timestamp", "INTEGER")?;
            self.add_column_if_missing(
                "profile_mods",
                "category_id",
                "INTEGER REFERENCES mod_categories(id)",
            )?;
            self.add_column_if_missing("profile_mods", "notes", "TEXT")?;
            self.add_column_if_missing("profile_mods", "tags", "TEXT")?;
            info!(
                from = version.max(1),
                to = 2,
                "database schema migrated to V2"
            );
        }

        if version < 3 {
            self.conn.execute_batch(SCHEMA_V3)?;
            info!(
                from = version.max(2),
                to = 3,
                "database schema migrated to V3"
            );
        }

        if version < 6 {
            // Add display_name column if it doesn't already exist.
            let has_display_name = self
                .conn
                .prepare("SELECT display_name FROM profile_mods LIMIT 0")
                .is_ok();
            if !has_display_name {
                self.conn
                    .execute_batch("ALTER TABLE profile_mods ADD COLUMN display_name TEXT;")?;
            }
            info!(
                from = version.max(5),
                to = 6,
                "database schema migrated to V6"
            );
        }

        if version < 7 {
            // Load order lock (V7): profile-level + per-mod locks. See
            // `crates/modde-core/src/profile/mod.rs` for `LoadOrderLock` /
            // `LockReason`. Columns are TOML-encoded to match the existing
            // `source_data` convention.
            let has_load_order_lock = self
                .conn
                .prepare("SELECT load_order_lock FROM profiles LIMIT 0")
                .is_ok();
            if !has_load_order_lock {
                self.conn
                    .execute_batch("ALTER TABLE profiles ADD COLUMN load_order_lock TEXT;")?;
            }
            let has_lock_reason = self
                .conn
                .prepare("SELECT lock_reason FROM profile_mods LIMIT 0")
                .is_ok();
            if !has_lock_reason {
                self.conn
                    .execute_batch("ALTER TABLE profile_mods ADD COLUMN lock_reason TEXT;")?;
            }
            info!(
                from = version.max(6),
                to = 7,
                "database schema migrated to V7"
            );
        }

        if version < 8 {
            // Installer pipeline (V8): per-mod install method + file
            // manifest. See `crates/modde-core/src/installer/` for the
            // pipeline that produces these values.
            let has_install_method = self
                .conn
                .prepare("SELECT install_method FROM profile_mods LIMIT 0")
                .is_ok();
            if !has_install_method {
                self.conn
                    .execute_batch("ALTER TABLE profile_mods ADD COLUMN install_method TEXT;")?;
            }
            let has_source_archive_hash = self
                .conn
                .prepare("SELECT source_archive_hash FROM profile_mods LIMIT 0")
                .is_ok();
            if !has_source_archive_hash {
                self.conn.execute_batch(
                    "ALTER TABLE profile_mods ADD COLUMN source_archive_hash TEXT;",
                )?;
            }
            let has_install_status = self
                .conn
                .prepare("SELECT install_status FROM profile_mods LIMIT 0")
                .is_ok();
            if !has_install_status {
                self.conn
                    .execute_batch("ALTER TABLE profile_mods ADD COLUMN install_status TEXT;")?;
            }
            self.conn.execute_batch(SCHEMA_V8)?;
            info!(
                from = version.max(7),
                to = 8,
                "database schema migrated to V8"
            );
        }

        if version < 9 {
            self.conn.execute_batch(SCHEMA_V9)?;
            info!(
                from = version.max(8),
                to = 9,
                "database schema migrated to V9"
            );
        }

        if version < 10 {
            self.conn.execute_batch(SCHEMA_V10)?;
            self.add_column_if_missing("game_tools", "current_node_id", "TEXT")?;
            info!(
                from = version.max(9),
                to = 10,
                "database schema migrated to V10"
            );
        }

        if version < CURRENT_SCHEMA_VERSION {
            self.conn
                .pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)?;
        }

        // Ensure WAL and FK are always on (they reset per-connection).
        self.conn
            .execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;

        Ok(())
    }

    fn add_column_if_missing(&self, table: &str, column: &str, definition: &str) -> Result<()> {
        if self.column_exists(table, column)? {
            return Ok(());
        }

        self.conn.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition};"
        ))?;
        Ok(())
    }

    fn column_exists(&self, table: &str, column: &str) -> Result<bool> {
        let mut stmt = self.conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        for name in rows {
            if name? == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // ── Profile CRUD ──────────────────────────────────────────────

    /// Create a new profile, returning its database ID.
    pub fn create_profile(&self, profile: &Profile) -> Result<i64> {
        let (source_type, source_data) = encode_source(&profile.source);
        let load_order_lock = encode_lock(profile.load_order_lock.as_ref());

        self.conn.execute(
            "INSERT INTO profiles (name, game_id, source_type, source_data, overrides, load_order_lock)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                profile.name,
                profile.game_id,
                source_type,
                source_data,
                profile.overrides.to_string_lossy().as_ref(),
                load_order_lock,
            ],
        )?;

        let profile_id = self.conn.last_insert_rowid();

        self.insert_mods(profile_id, &profile.mods)?;
        self.insert_rules(profile_id, &profile.load_order_rules)?;

        Ok(profile_id)
    }

    /// Load a profile by name and `game_id`.
    pub fn load_profile(&self, name: &str, game_id: &GameId) -> Result<Profile> {
        let (id, source_type, source_data, overrides, load_order_lock) = self
            .conn
            .query_row(
                "SELECT id, source_type, source_data, overrides, load_order_lock FROM profiles
                 WHERE name = ?1 AND game_id = ?2",
                params![name, game_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    CoreError::ProfileNotFound(format!("{name} (game: {game_id})"))
                }
                other => CoreError::Database(other),
            })?;

        self.assemble_profile(
            id,
            name,
            game_id,
            &source_type,
            source_data.as_deref(),
            &overrides,
            load_order_lock.as_deref(),
        )
    }

    /// Load a profile by its database ID.
    pub fn load_profile_by_id(&self, id: i64) -> Result<Profile> {
        let (name, game_id, source_type, source_data, overrides, load_order_lock) = self
            .conn
            .query_row(
                "SELECT name, game_id, source_type, source_data, overrides, load_order_lock
                 FROM profiles WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    CoreError::ProfileNotFound(format!("id={id}"))
                }
                other => CoreError::Database(other),
            })?;

        self.assemble_profile(
            id,
            &name,
            &GameId::from(game_id),
            &source_type,
            source_data.as_deref(),
            &overrides,
            load_order_lock.as_deref(),
        )
    }

    /// Load a profile by name only. Errors with `AmbiguousProfile` if multiple games match.
    pub fn load_profile_by_name(&self, name: &str) -> Result<Profile> {
        let mut stmt = self.conn.prepare(
            "SELECT id, game_id, source_type, source_data, overrides, load_order_lock
             FROM profiles WHERE name = ?1",
        )?;

        let rows: Vec<(i64, String, String, Option<String>, String, Option<String>)> = stmt
            .query_map(params![name], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        match rows.len() {
            0 => Err(CoreError::ProfileNotFound(name.to_string())),
            1 => {
                let (id, game_id, source_type, source_data, overrides, load_order_lock) = &rows[0];
                self.assemble_profile(
                    *id,
                    name,
                    &GameId::from(game_id.clone()),
                    source_type,
                    source_data.as_deref(),
                    overrides,
                    load_order_lock.as_deref(),
                )
            }
            _ => {
                let games: SmallVec<[GameId; 4]> = rows
                    .iter()
                    .map(|(_, g, _, _, _, _)| GameId::from(g.clone()))
                    .collect();
                Err(CoreError::AmbiguousProfile {
                    name: name.to_string(),
                    games,
                })
            }
        }
    }

    /// Update an existing profile (identified by name + `game_id`).
    pub fn update_profile(&self, profile: &Profile) -> Result<()> {
        let (source_type, source_data) = encode_source(&profile.source);
        let load_order_lock = encode_lock(profile.load_order_lock.as_ref());

        let profile_id: i64 = self
            .conn
            .query_row(
                "SELECT id FROM profiles WHERE name = ?1 AND game_id = ?2",
                params![profile.name, profile.game_id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => CoreError::ProfileNotFound(format!(
                    "{} (game: {})",
                    profile.name, profile.game_id
                )),
                other => CoreError::Database(other),
            })?;

        self.conn.execute(
            "UPDATE profiles SET source_type = ?1, source_data = ?2, overrides = ?3,
                    load_order_lock = ?4, updated_at = datetime('now')
             WHERE id = ?5",
            params![
                source_type,
                source_data,
                profile.overrides.to_string_lossy().as_ref(),
                load_order_lock,
                profile_id,
            ],
        )?;

        // Replace mods and rules
        self.conn.execute(
            "DELETE FROM profile_mods WHERE profile_id = ?1",
            params![profile_id],
        )?;
        self.conn.execute(
            "DELETE FROM load_order_rules WHERE profile_id = ?1",
            params![profile_id],
        )?;

        self.insert_mods(profile_id, &profile.mods)?;
        self.insert_rules(profile_id, &profile.load_order_rules)?;

        Ok(())
    }

    /// Delete a profile by name and `game_id`.
    pub fn delete_profile(&self, name: &str, game_id: &GameId) -> Result<()> {
        let changes = self.conn.execute(
            "DELETE FROM profiles WHERE name = ?1 AND game_id = ?2",
            params![name, game_id],
        )?;
        if changes == 0 {
            return Err(CoreError::ProfileNotFound(format!(
                "{name} (game: {game_id})"
            )));
        }
        Ok(())
    }

    /// List profile summaries, optionally filtered by game.
    pub fn list_profiles(&self, game_id: Option<&GameId>) -> Result<Vec<ProfileSummary>> {
        let (sql, bind) = match game_id {
            Some(gid) => (
                "SELECT p.id, p.name, p.game_id, p.source_type,
                        (SELECT COUNT(*) FROM profile_mods WHERE profile_id = p.id) as mod_count
                 FROM profiles p WHERE p.game_id = ?1 ORDER BY p.name",
                Some(gid.to_string()),
            ),
            None => (
                "SELECT p.id, p.name, p.game_id, p.source_type,
                        (SELECT COUNT(*) FROM profile_mods WHERE profile_id = p.id) as mod_count
                 FROM profiles p ORDER BY p.game_id, p.name",
                None,
            ),
        };

        let mut stmt = self.conn.prepare(sql)?;

        let row_mapper = |row: &rusqlite::Row<'_>| {
            Ok(ProfileSummary {
                id: row.get(0)?,
                name: row.get(1)?,
                game_id: GameId::from(row.get::<_, String>(2)?),
                source_type: row.get(3)?,
                mod_count: row.get::<_, i64>(4)? as usize,
            })
        };

        let summaries = match &bind {
            Some(gid) => stmt.query_map(params![gid], row_mapper)?,
            None => stmt.query_map([], row_mapper)?,
        }
        .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(summaries)
    }

    // ── Save CRUD ─────────────────────────────────────────────────

    /// Assign a save to a profile.
    pub fn assign_save(&self, profile_id: i64, path: &Path, label: Option<&str>) -> Result<()> {
        let path_str = path.to_string_lossy();

        // Check if already assigned to a different profile
        let existing: Option<(i64, String)> = self
            .conn
            .query_row(
                "SELECT s.profile_id, p.name FROM saves s
                 JOIN profiles p ON p.id = s.profile_id
                 WHERE s.path = ?1",
                params![path_str.as_ref()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        if let Some((existing_id, existing_name)) = existing {
            if existing_id != profile_id {
                return Err(CoreError::SaveAlreadyAssigned {
                    path: path_str.to_string(),
                    profile: existing_name,
                });
            }
            // Already assigned to this profile — update label
            self.conn.execute(
                "UPDATE saves SET label = ?1 WHERE path = ?2",
                params![label, path_str.as_ref()],
            )?;
            return Ok(());
        }

        self.conn.execute(
            "INSERT INTO saves (profile_id, path, label) VALUES (?1, ?2, ?3)",
            params![profile_id, path_str.as_ref(), label],
        )?;

        Ok(())
    }

    /// Remove a save assignment.
    pub fn unassign_save(&self, path: &Path) -> Result<()> {
        let path_str = path.to_string_lossy();
        self.conn.execute(
            "DELETE FROM saves WHERE path = ?1",
            params![path_str.as_ref()],
        )?;
        Ok(())
    }

    /// List all saves assigned to a profile.
    pub fn list_saves(&self, profile_id: i64) -> Result<Vec<SaveEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, label, assigned_at FROM saves WHERE profile_id = ?1 ORDER BY assigned_at",
        )?;

        let saves = stmt
            .query_map(params![profile_id], |row| {
                Ok(SaveEntry {
                    path: PathBuf::from(row.get::<_, String>(0)?),
                    label: row.get(1)?,
                    assigned_at: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(saves)
    }

    /// Check if a save path is assigned to any profile.
    pub fn is_save_assigned(&self, path: &Path) -> Result<bool> {
        let path_str = path.to_string_lossy();
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM saves WHERE path = ?1",
            params![path_str.as_ref()],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    // ── Active Profile Tracking ────────────────────────────────────

    /// Set the active profile for a game, replacing any previous one.
    pub fn set_active_profile(&self, game_id: &GameId, profile_id: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO active_profiles (game_id, profile_id)
             VALUES (?1, ?2)
             ON CONFLICT(game_id) DO UPDATE SET
                profile_id = excluded.profile_id,
                activated_at = datetime('now')",
            params![game_id, profile_id],
        )?;
        Ok(())
    }

    /// Get the active profile for a game, returning (`profile_id`, `profile_name`).
    pub fn get_active_profile(&self, game_id: &GameId) -> Result<Option<(i64, String)>> {
        let result = self.conn.query_row(
            "SELECT a.profile_id, p.name FROM active_profiles a
             JOIN profiles p ON p.id = a.profile_id
             WHERE a.game_id = ?1",
            params![game_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        match result {
            Ok(pair) => Ok(Some(pair)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Clear the active profile for a game.
    pub fn clear_active_profile(&self, game_id: &GameId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM active_profiles WHERE game_id = ?1",
            params![game_id],
        )?;
        Ok(())
    }

    // ── Experiment Stack ──────────────────────────────────────────

    /// Push a profile onto the experiment stack for a game.
    pub fn push_experiment(&self, game_id: &GameId, profile_id: i64) -> Result<()> {
        let depth = self.experiment_depth(game_id)?;
        self.conn.execute(
            "INSERT INTO experiment_stack (game_id, profile_id, depth)
             VALUES (?1, ?2, ?3)",
            params![game_id, profile_id, depth as i64],
        )?;
        Ok(())
    }

    /// Pop the top entry from the experiment stack, returning the `profile_id`.
    pub fn pop_experiment(&self, game_id: &GameId) -> Result<Option<i64>> {
        let result = self.conn.query_row(
            "SELECT id, profile_id FROM experiment_stack
             WHERE game_id = ?1 ORDER BY depth DESC LIMIT 1",
            params![game_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        );

        match result {
            Ok((id, profile_id)) => {
                self.conn
                    .execute("DELETE FROM experiment_stack WHERE id = ?1", params![id])?;
                Ok(Some(profile_id))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get the experiment stack depth for a game.
    pub fn experiment_depth(&self, game_id: &GameId) -> Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM experiment_stack WHERE game_id = ?1",
            params![game_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Clear the entire experiment stack for a game.
    pub fn clear_experiment_stack(&self, game_id: &GameId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM experiment_stack WHERE game_id = ?1",
            params![game_id],
        )?;
        Ok(())
    }

    // ── Stock Snapshots ───────────────────────────────────────────

    /// Insert or update a stock snapshot record.
    pub fn upsert_snapshot(
        &self,
        game_id: &GameId,
        snapshot_path: &Path,
        tree_hash: &str,
        file_count: usize,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO stock_snapshots (game_id, snapshot_path, tree_hash, file_count)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(game_id) DO UPDATE SET
                snapshot_path = excluded.snapshot_path,
                tree_hash = excluded.tree_hash,
                file_count = excluded.file_count,
                created_at = datetime('now')",
            params![
                game_id,
                snapshot_path.to_string_lossy().as_ref(),
                tree_hash,
                file_count as i64,
            ],
        )?;
        Ok(())
    }

    /// Get snapshot metadata for a game.
    pub fn get_snapshot(&self, game_id: &GameId) -> Result<Option<SnapshotMeta>> {
        let result = self.conn.query_row(
            "SELECT game_id, snapshot_path, tree_hash, file_count, created_at
             FROM stock_snapshots WHERE game_id = ?1",
            params![game_id],
            |row| {
                Ok(SnapshotMeta {
                    game_id: GameId::from(row.get::<_, String>(0)?),
                    snapshot_path: PathBuf::from(row.get::<_, String>(1)?),
                    tree_hash: row.get(2)?,
                    file_count: row.get::<_, i64>(3)? as usize,
                    created_at: row.get(4)?,
                })
            },
        );

        match result {
            Ok(meta) => Ok(Some(meta)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // ── Hidden Files ─────────────────────────────────────────────

    /// Hide a file from a mod in a profile (prevents deployment).
    pub fn hide_file(&self, profile_id: i64, mod_id: &ModId, rel_path: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO hidden_files (profile_id, mod_id, rel_path)
             VALUES (?1, ?2, ?3)",
            params![profile_id, mod_id, rel_path],
        )?;
        Ok(())
    }

    /// Unhide a previously hidden file.
    pub fn unhide_file(&self, profile_id: i64, mod_id: &ModId, rel_path: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM hidden_files WHERE profile_id = ?1 AND mod_id = ?2 AND rel_path = ?3",
            params![profile_id, mod_id, rel_path],
        )?;
        Ok(())
    }

    /// List all hidden files for a profile.
    pub fn list_hidden_files(&self, profile_id: i64) -> Result<Vec<HiddenFile>> {
        let mut stmt = self
            .conn
            .prepare("SELECT mod_id, rel_path FROM hidden_files WHERE profile_id = ?1")?;
        let files = stmt
            .query_map(params![profile_id], |row| {
                Ok(HiddenFile {
                    mod_id: row.get(0)?,
                    rel_path: row.get(1)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(files)
    }

    /// List hidden files for a specific mod in a profile.
    pub fn list_hidden_files_for_mod(
        &self,
        profile_id: i64,
        mod_id: &ModId,
    ) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT rel_path FROM hidden_files WHERE profile_id = ?1 AND mod_id = ?2")?;
        let paths = stmt
            .query_map(params![profile_id, mod_id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(paths)
    }

    // ── Plugin Order ─────────────────────────────────────────────

    /// Set the plugin order for a profile (replaces any existing order).
    pub fn set_plugin_order(&self, profile_id: i64, plugins: &[PluginEntry]) -> Result<()> {
        self.conn.execute(
            "DELETE FROM plugin_order WHERE profile_id = ?1",
            params![profile_id],
        )?;
        let mut stmt = self.conn.prepare(
            "INSERT INTO plugin_order (profile_id, plugin_name, sort_index, enabled)
             VALUES (?1, ?2, ?3, ?4)",
        )?;
        for plugin in plugins {
            stmt.execute(params![
                profile_id,
                plugin.plugin_name,
                plugin.sort_index,
                plugin.enabled,
            ])?;
        }
        Ok(())
    }

    /// Get the plugin order for a profile.
    pub fn get_plugin_order(&self, profile_id: i64) -> Result<Vec<PluginEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT plugin_name, sort_index, enabled FROM plugin_order
             WHERE profile_id = ?1 ORDER BY sort_index",
        )?;
        let plugins = stmt
            .query_map(params![profile_id], |row| {
                Ok(PluginEntry {
                    plugin_name: row.get(0)?,
                    sort_index: row.get(1)?,
                    enabled: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(plugins)
    }

    /// Toggle a plugin's enabled state.
    pub fn toggle_plugin(&self, profile_id: i64, plugin_name: &str, enabled: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE plugin_order SET enabled = ?1 WHERE profile_id = ?2 AND plugin_name = ?3",
            params![enabled, profile_id, plugin_name],
        )?;
        Ok(())
    }

    // ── Mod Categories ───────────────────────────────────────────

    /// Create a mod category, returning its ID.
    pub fn create_category(&self, profile_id: i64, category: &ModCategory) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO mod_categories (profile_id, name, color, sort_index)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                profile_id,
                category.name,
                category.color,
                category.sort_index
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Update a category.
    pub fn update_category(
        &self,
        category_id: i64,
        name: &str,
        color: Option<&str>,
        sort_index: i64,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE mod_categories SET name = ?1, color = ?2, sort_index = ?3 WHERE id = ?4",
            params![name, color, sort_index, category_id],
        )?;
        Ok(())
    }

    /// Delete a category (nullifies `category_id` on affected mods).
    pub fn delete_category(&self, category_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE profile_mods SET category_id = NULL WHERE category_id = ?1",
            params![category_id],
        )?;
        self.conn.execute(
            "DELETE FROM mod_categories WHERE id = ?1",
            params![category_id],
        )?;
        Ok(())
    }

    /// List categories for a profile.
    pub fn list_categories(&self, profile_id: i64) -> Result<Vec<ModCategory>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, color, sort_index FROM mod_categories
             WHERE profile_id = ?1 ORDER BY sort_index",
        )?;
        let cats = stmt
            .query_map(params![profile_id], |row| {
                Ok(ModCategory {
                    id: Some(row.get(0)?),
                    name: row.get(1)?,
                    color: row.get(2)?,
                    sort_index: row.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(cats)
    }

    /// Assign a mod to a category.
    pub fn set_mod_category(
        &self,
        profile_id: i64,
        mod_id: &ModId,
        category_id: Option<i64>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE profile_mods SET category_id = ?1 WHERE profile_id = ?2 AND mod_id = ?3",
            params![category_id, profile_id, mod_id],
        )?;
        Ok(())
    }

    /// Set notes for a mod.
    pub fn set_mod_notes(
        &self,
        profile_id: i64,
        mod_id: &ModId,
        notes: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE profile_mods SET notes = ?1 WHERE profile_id = ?2 AND mod_id = ?3",
            params![notes, profile_id, mod_id],
        )?;
        Ok(())
    }

    /// Set tags for a mod (stored as a JSON array in the TEXT column).
    pub fn set_mod_tags(&self, profile_id: i64, mod_id: &ModId, tags: &[String]) -> Result<()> {
        let encoded_tags = encode_tags(tags)?;
        self.conn.execute(
            "UPDATE profile_mods SET tags = ?1 WHERE profile_id = ?2 AND mod_id = ?3",
            params![encoded_tags, profile_id, mod_id],
        )?;
        Ok(())
    }

    /// Set Nexus metadata for a mod.
    pub fn set_mod_nexus_meta(
        &self,
        profile_id: i64,
        mod_id: &ModId,
        nexus_mod_id: NexusModId,
        nexus_file_id: NexusFileId,
        nexus_game_domain: &str,
        installed_timestamp: i64,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE profile_mods SET nexus_mod_id = ?1, nexus_file_id = ?2,
                    nexus_game_domain = ?3, installed_timestamp = ?4
             WHERE profile_id = ?5 AND mod_id = ?6",
            params![
                nexus_mod_id.to_i64()?,
                nexus_file_id.to_i64()?,
                nexus_game_domain,
                installed_timestamp,
                profile_id,
                mod_id
            ],
        )?;
        Ok(())
    }

    // ── Installer tracking (V8) ───────────────────────────────────

    /// Persist an installer's decision and file manifest for a single
    /// mod row, atomically.
    ///
    /// Steps in one transaction:
    /// 1. Write the encoded `install_method`, `source_archive_hash`, and
    ///    `install_status` back to `profile_mods`.
    /// 2. Wipe any previous `installed_mod_files` rows for this mod
    ///    (so retries don't leave orphans in the manifest).
    /// 3. Insert one row per `plan.staged_files`.
    ///
    /// Callers are expected to have already written the `EnabledMod` into
    /// the profile (via `update_profile` / `create_profile`); this method
    /// just enriches the existing row with install metadata and files.
    pub fn record_install(
        &mut self,
        profile_id: i64,
        mod_id: &ModId,
        plan: &InstallPlan,
        status: InstallStatus,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;

        let method_toml = encode_install_method(&plan.method)?;
        tx.execute(
            "UPDATE profile_mods
                SET install_method = ?1,
                    source_archive_hash = ?2,
                    install_status = ?3
              WHERE profile_id = ?4 AND mod_id = ?5",
            params![
                method_toml,
                plan.source_archive_hash,
                status.as_str(),
                profile_id,
                mod_id,
            ],
        )?;

        tx.execute(
            "DELETE FROM installed_mod_files WHERE profile_id = ?1 AND mod_id = ?2",
            params![profile_id, mod_id],
        )?;

        {
            let mut stmt = tx.prepare(
                "INSERT INTO installed_mod_files
                    (profile_id, mod_id, rel_path, origin_rel_path, size, merge_group)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for file in &plan.staged_files {
                stmt.execute(params![
                    profile_id,
                    mod_id,
                    file.rel_path,
                    file.origin_rel_path,
                    file.size as i64,
                    file.merge_group,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// Return every file staged by `mod_id` in `profile_id`, sorted by
    /// relative path for deterministic uninstall order.
    pub fn installed_files_for_mod(
        &self,
        profile_id: i64,
        mod_id: &ModId,
    ) -> Result<Vec<StagedFile>> {
        let mut stmt = self.conn.prepare(
            "SELECT rel_path, origin_rel_path, size, merge_group
               FROM installed_mod_files
              WHERE profile_id = ?1 AND mod_id = ?2
           ORDER BY rel_path",
        )?;
        let files = stmt
            .query_map(params![profile_id, mod_id], |row| {
                let size_i: i64 = row.get(2)?;
                Ok(StagedFile {
                    rel_path: row.get(0)?,
                    origin_rel_path: row.get(1)?,
                    size: size_i.max(0) as u64,
                    merge_group: row.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(files)
    }

    /// Remove a mod from `profile_mods` and return its staged files so
    /// the caller can unlink them from the store / deploy dir. Runs in
    /// one transaction — either the manifest and row both disappear or
    /// neither does.
    pub fn remove_installed_mod(
        &mut self,
        profile_id: i64,
        mod_id: &ModId,
    ) -> Result<Vec<StagedFile>> {
        let files = self.installed_files_for_mod(profile_id, mod_id)?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM installed_mod_files WHERE profile_id = ?1 AND mod_id = ?2",
            params![profile_id, mod_id],
        )?;
        tx.execute(
            "DELETE FROM profile_mods WHERE profile_id = ?1 AND mod_id = ?2",
            params![profile_id, mod_id],
        )?;
        tx.commit()?;
        Ok(files)
    }

    /// Return every file tagged with `merge_group`, across all mods in
    /// `profile_id`. Reserved for the future script-merge feature —
    /// unused by current code, but exposed now so the V8 schema can
    /// carry a stable query shape.
    pub fn files_in_merge_group(
        &self,
        profile_id: i64,
        merge_group: &str,
    ) -> Result<Vec<(String, StagedFile)>> {
        let mut stmt = self.conn.prepare(
            "SELECT mod_id, rel_path, origin_rel_path, size, merge_group
               FROM installed_mod_files
              WHERE profile_id = ?1 AND merge_group = ?2
           ORDER BY mod_id, rel_path",
        )?;
        let rows = stmt
            .query_map(params![profile_id, merge_group], |row| {
                let size_i: i64 = row.get(3)?;
                Ok((
                    row.get::<_, String>(0)?,
                    StagedFile {
                        rel_path: row.get(1)?,
                        origin_rel_path: row.get(2)?,
                        size: size_i.max(0) as u64,
                        merge_group: row.get(4)?,
                    },
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ── TOML Import ───────────────────────────────────────────────

    /// Import existing TOML profile files into the database.
    /// Returns the number of profiles imported.
    pub fn import_toml_profiles(&self, profiles_dir: &Path) -> Result<usize> {
        if !profiles_dir.exists() {
            return Ok(0);
        }

        let mut count = 0usize;

        for entry in std::fs::read_dir(profiles_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }

            let toml_path = entry.path().join("profile.toml");
            if !toml_path.exists() {
                continue;
            }

            let content = match std::fs::read_to_string(&toml_path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(path = %toml_path.display(), error = %e, "skipping unreadable profile");
                    continue;
                }
            };

            #[allow(deprecated)]
            let mut profile: Profile = match toml::from_str(&content) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(path = %toml_path.display(), error = %e, "skipping unparseable profile");
                    continue;
                }
            };

            // Preserve-before-overwrite: if the TOML file already carried
            // a lock (e.g. a Wabbajack-installed profile that was previously
            // exported), honor that provenance. Only stamp a fresh
            // `TomlImport` lock when the parsed profile has no lock — so
            // round-tripping an already-locked profile through TOML doesn't
            // destroy its origin.
            if profile.load_order_lock.is_none() {
                profile.load_order_lock = Some(LoadOrderLock::now(LockReason::TomlImport {
                    source_path: toml_path.display().to_string(),
                }));
            }

            // Skip if already in DB
            let exists: bool = self.conn.query_row(
                "SELECT COUNT(*) > 0 FROM profiles WHERE name = ?1 AND game_id = ?2",
                params![profile.name, profile.game_id],
                |row| row.get(0),
            )?;

            if exists {
                tracing::debug!(name = %profile.name, game = %profile.game_id, "profile already in DB, skipping");
                continue;
            }

            self.create_profile(&profile)?;
            info!(name = %profile.name, game = %profile.game_id, "imported TOML profile");
            count += 1;
        }

        Ok(count)
    }

    // ── Internal helpers ──────────────────────────────────────────

    fn nexus_mod_id_from_row(
        row: &rusqlite::Row<'_>,
        idx: usize,
    ) -> rusqlite::Result<Option<NexusModId>> {
        let raw: Option<i64> = row.get(idx)?;
        raw.map(NexusModId::try_from)
            .transpose()
            .map_err(Self::nexus_id_row_error)
    }

    fn nexus_file_id_from_row(
        row: &rusqlite::Row<'_>,
        idx: usize,
    ) -> rusqlite::Result<Option<NexusFileId>> {
        let raw: Option<i64> = row.get(idx)?;
        raw.map(NexusFileId::try_from)
            .transpose()
            .map_err(Self::nexus_id_row_error)
    }

    fn nexus_id_row_error(err: NexusIdError) -> rusqlite::Error {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Integer, Box::new(err))
    }

    fn core_row_error(err: CoreError) -> rusqlite::Error {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
    }

    fn insert_mods(&self, profile_id: i64, mods: &[EnabledMod]) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO profile_mods (profile_id, mod_id, display_name, enabled, version, fomod_config, sort_index,
                    nexus_mod_id, nexus_file_id, nexus_game_domain, installed_timestamp,
                    category_id, notes, tags, lock_reason,
                    install_method, source_archive_hash, install_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        )?;

        for (idx, m) in mods.iter().enumerate() {
            let lock_reason = encode_lock_reason(m.lock.as_ref());
            let nexus_mod_id = m.nexus_mod_id.map(NexusModId::to_i64).transpose()?;
            let nexus_file_id = m.nexus_file_id.map(NexusFileId::to_i64).transpose()?;
            let tags = encode_tags(&m.tags)?;
            let install_method = m
                .install_method
                .as_ref()
                .map(encode_install_method)
                .transpose()?;
            let install_status = m.install_status.map(InstallStatus::as_str);
            stmt.execute(params![
                profile_id,
                m.mod_id,
                m.display_name,
                m.enabled,
                m.version,
                m.fomod_config,
                idx as i64,
                nexus_mod_id,
                nexus_file_id,
                m.nexus_game_domain,
                m.installed_timestamp,
                m.category_id,
                m.notes,
                tags,
                lock_reason,
                install_method,
                m.source_archive_hash,
                install_status,
            ])?;
        }

        Ok(())
    }

    fn insert_rules(&self, profile_id: i64, rules: &[LoadOrderRule]) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO load_order_rules (profile_id, rule_type, mod_a, mod_b)
             VALUES (?1, ?2, ?3, ?4)",
        )?;

        for rule in rules {
            let (rule_type, mod_a, mod_b) = match rule {
                LoadOrderRule::LoadAfter { mod_id, after } => {
                    ("load_after", mod_id.as_str(), after.as_str())
                }
                LoadOrderRule::LoadBefore { mod_id, before } => {
                    ("load_before", mod_id.as_str(), before.as_str())
                }
                LoadOrderRule::Incompatible { mod_a, mod_b } => {
                    ("incompatible", mod_a.as_str(), mod_b.as_str())
                }
            };
            stmt.execute(params![profile_id, rule_type, mod_a, mod_b])?;
        }

        Ok(())
    }

    fn load_mods(&self, profile_id: i64) -> Result<Vec<EnabledMod>> {
        let mut stmt = self.conn.prepare(
            "SELECT mod_id, display_name, enabled, version, fomod_config,
                    nexus_mod_id, nexus_file_id, nexus_game_domain, installed_timestamp,
                    category_id, notes, tags, lock_reason,
                    install_method, source_archive_hash, install_status
             FROM profile_mods WHERE profile_id = ?1 ORDER BY sort_index",
        )?;

        let mods = stmt
            .query_map(params![profile_id], |row| {
                let lock_reason_raw: Option<String> = row.get(12)?;
                let nexus_mod_id = Self::nexus_mod_id_from_row(row, 5)?;
                let nexus_file_id = Self::nexus_file_id_from_row(row, 6)?;
                let tags_raw: Option<String> = row.get(11)?;
                let install_method_raw: Option<String> = row.get(13)?;
                let install_status_raw: Option<String> = row.get(15)?;
                Ok(EnabledMod {
                    mod_id: row.get(0)?,
                    display_name: row.get(1)?,
                    enabled: row.get(2)?,
                    version: row.get(3)?,
                    fomod_config: row.get(4)?,
                    nexus_mod_id,
                    nexus_file_id,
                    nexus_game_domain: row.get(7)?,
                    installed_timestamp: row.get(8)?,
                    category_id: row.get(9)?,
                    notes: row.get(10)?,
                    tags: decode_tags(tags_raw.as_deref()).map_err(Self::core_row_error)?,
                    lock: decode_lock_reason(lock_reason_raw.as_deref())
                        .map_err(Self::core_row_error)?,
                    install_method: decode_install_method(install_method_raw.as_deref())
                        .map_err(Self::core_row_error)?,
                    source_archive_hash: row.get(14)?,
                    install_status: decode_install_status(install_status_raw.as_deref())
                        .map_err(Self::core_row_error)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(mods)
    }

    fn load_rules(&self, profile_id: i64) -> Result<SmallVec<[LoadOrderRule; 4]>> {
        let mut stmt = self.conn.prepare(
            "SELECT rule_type, mod_a, mod_b FROM load_order_rules WHERE profile_id = ?1",
        )?;

        let rules = stmt
            .query_map(params![profile_id], |row| {
                let rule_type: String = row.get(0)?;
                let mod_a: String = row.get(1)?;
                let mod_b: String = row.get(2)?;
                Ok((rule_type, mod_a, mod_b))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut result = SmallVec::with_capacity(rules.len());
        for (rule_type, mod_a, mod_b) in rules {
            let rule = match rule_type.as_str() {
                "load_after" => LoadOrderRule::LoadAfter {
                    mod_id: ModId::from(mod_a),
                    after: ModId::from(mod_b),
                },
                "load_before" => LoadOrderRule::LoadBefore {
                    mod_id: ModId::from(mod_a),
                    before: ModId::from(mod_b),
                },
                "incompatible" => LoadOrderRule::Incompatible {
                    mod_a: ModId::from(mod_a),
                    mod_b: ModId::from(mod_b),
                },
                other => {
                    tracing::warn!(rule_type = other, "unknown load order rule type, skipping");
                    continue;
                }
            };
            result.push(rule);
        }

        Ok(result)
    }

    fn assemble_profile(
        &self,
        id: i64,
        name: &str,
        game_id: &GameId,
        source_type: &str,
        source_data: Option<&str>,
        overrides: &str,
        load_order_lock_raw: Option<&str>,
    ) -> Result<Profile> {
        let source = decode_source(source_type, source_data)?;
        let mods = self.load_mods(id)?;
        let load_order_rules = self.load_rules(id)?;
        let load_order_lock = decode_lock(load_order_lock_raw)?;

        Ok(Profile {
            id: Some(id),
            name: name.to_string(),
            game_id: game_id.clone(),
            source,
            mods,
            overrides: PathBuf::from(overrides),
            load_order_rules,
            load_order_lock,
        })
    }
}

// ── Source encoding ──────────────────────────────────────────

fn encode_source(source: &ProfileSource) -> (&'static str, Option<String>) {
    match source {
        ProfileSource::Manual => ("manual", None),
        ProfileSource::NexusCollection { slug, version } => {
            let data = format!("slug = {slug:?}\nversion = {version:?}");
            ("nexus_collection", Some(data))
        }
        ProfileSource::Wabbajack { manifest_hash } => {
            let data = format!("manifest_hash = {manifest_hash:?}");
            ("wabbajack", Some(data))
        }
    }
}

fn decode_source(source_type: &str, source_data: Option<&str>) -> Result<ProfileSource> {
    match source_type {
        "manual" => Ok(ProfileSource::Manual),
        "nexus_collection" => {
            let data = source_data.unwrap_or_default();
            let table: toml::Table = toml::from_str(data).map_err(|e| {
                CoreError::Other(
                    format!("failed to parse nexus_collection source data: {e}").into(),
                )
            })?;
            let slug = table
                .get("slug")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let version = table
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            Ok(ProfileSource::NexusCollection { slug, version })
        }
        "wabbajack" => {
            let data = source_data.unwrap_or_default();
            let table: toml::Table = toml::from_str(data).map_err(|e| {
                CoreError::Other(format!("failed to parse wabbajack source data: {e}").into())
            })?;
            let manifest_hash = table
                .get("manifest_hash")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            Ok(ProfileSource::Wabbajack { manifest_hash })
        }
        other => Err(CoreError::Other(
            format!("unknown profile source type: {other}").into(),
        )),
    }
}

// ── Load order lock encoding ─────────────────────────────────
//
// Lock state lives in TEXT columns (TOML-encoded) to match the existing
// `source_data` convention above. A NULL column means "no lock".

fn encode_lock(lock: Option<&LoadOrderLock>) -> Option<String> {
    lock.map(|l| toml::to_string(l).expect("LoadOrderLock should always serialize"))
}

fn decode_lock(raw: Option<&str>) -> Result<Option<LoadOrderLock>> {
    match raw {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => toml::from_str::<LoadOrderLock>(s)
            .map(Some)
            .map_err(|e| CoreError::Other(format!("failed to parse load_order_lock: {e}").into())),
    }
}

fn encode_lock_reason(reason: Option<&LockReason>) -> Option<String> {
    reason.map(|r| toml::to_string(r).expect("LockReason should always serialize"))
}

fn decode_lock_reason(raw: Option<&str>) -> Result<Option<LockReason>> {
    match raw {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => toml::from_str::<LockReason>(s)
            .map(Some)
            .map_err(|e| CoreError::Other(format!("failed to parse lock_reason: {e}").into())),
    }
}

// ── Installer method encoding (V8) ───────────────────────────
//
// `InstallMethod` is a serde-friendly enum, so we TOML-encode it like
// the other metadata blobs stored on `profile_mods`. Decoding is
// surfaced via `crate::installer::InstallMethod`'s own deserialize
// — callers decode directly when they need a typed value.

fn encode_install_method(method: &InstallMethod) -> Result<String> {
    toml::to_string(method)
        .map_err(|e| CoreError::Other(format!("failed to encode install_method: {e}").into()))
}

/// Parse the TOML-encoded `install_method` column back into a typed
/// [`InstallMethod`]. Returns `None` for NULL / empty strings.
pub fn decode_install_method(raw: Option<&str>) -> Result<Option<InstallMethod>> {
    match raw {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => toml::from_str::<InstallMethod>(s)
            .map(Some)
            .map_err(|e| CoreError::Other(format!("failed to parse install_method: {e}").into())),
    }
}

fn encode_tags(tags: &[String]) -> Result<Option<String>> {
    if tags.is_empty() {
        Ok(None)
    } else {
        serde_json::to_string(tags)
            .map(Some)
            .map_err(CoreError::Json)
    }
}

fn decode_tags(raw: Option<&str>) -> Result<Vec<String>> {
    match raw {
        None => Ok(Vec::new()),
        Some(s) if s.is_empty() => Ok(Vec::new()),
        Some(s) => serde_json::from_str::<Vec<String>>(s)
            .map_err(|e| CoreError::Other(format!("failed to parse tags JSON: {e}").into())),
    }
}

fn decode_install_status(raw: Option<&str>) -> Result<Option<InstallStatus>> {
    match raw {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => InstallStatus::parse(s)
            .map(Some)
            .ok_or_else(|| CoreError::Other(format!("unknown install_status: {s}").into())),
    }
}

fn new_tool_setting_node_id(game_id: &GameId, tool_id: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let game = sanitize_node_id_part(game_id.as_str());
    let tool = sanitize_node_id_part(tool_id);
    format!("tool-{game}-{tool}-{nanos}-{}", std::process::id())
}

fn sanitize_node_id_part(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

// ── Tool config types (re-exported from modde_games::tools) ──────────

/// Per-game tool configuration stored in the database.
///
/// This is a DB-layer representation; the full `ToolConfig` with
/// `serde_json::Value` settings lives in `modde_games::tools`.
#[derive(Debug, Clone)]
pub struct ToolConfigRow {
    pub tool_id: String,
    pub enabled: bool,
    pub settings_json: String,
}

/// One versioned tool settings node in the per-tool DAG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSettingHistoryNode {
    pub node_id: String,
    pub game_id: String,
    pub tool_id: String,
    pub enabled: bool,
    pub settings_json: String,
    pub reason: String,
    pub created_at: String,
    pub is_current: bool,
}

/// One parent-child edge in the per-tool settings DAG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSettingHistoryEdge {
    pub parent_node_id: String,
    pub child_node_id: String,
}

/// A file applied by a tool to a game directory.
#[derive(Debug, Clone)]
pub struct ToolAppliedFileRow {
    pub tool_id: String,
    pub rel_path: String,
}

/// A named executable launch target for a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableConfigRow {
    pub game_id: String,
    pub name: String,
    pub executable_path: PathBuf,
    pub arguments_json: String,
    pub working_dir: Option<PathBuf>,
    pub environment_json: String,
    pub wine_dll_overrides: Option<String>,
    pub output_mod: String,
    pub enabled: bool,
}

impl ModdeDb {
    // ── Game Tool CRUD ────────────────────────────────────────────

    /// Save (insert or update) a tool configuration for a game.
    pub fn save_tool_config(
        &self,
        game_id: &GameId,
        tool_id: &str,
        enabled: bool,
        settings_json: &str,
    ) -> Result<()> {
        self.save_tool_config_with_reason(game_id, tool_id, enabled, settings_json, "update")
    }

    /// Save a tool configuration and append a history node.
    pub fn save_tool_config_with_reason(
        &self,
        game_id: &GameId,
        tool_id: &str,
        enabled: bool,
        settings_json: &str,
        reason: &str,
    ) -> Result<()> {
        let parent_node_id = self.current_tool_setting_node_id(game_id, tool_id)?;
        let node_id = new_tool_setting_node_id(game_id, tool_id);
        let reason = if reason.trim().is_empty() {
            "update"
        } else {
            reason.trim()
        };

        self.conn.execute(
            "INSERT INTO tool_setting_nodes (node_id, game_id, tool_id, enabled, settings, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                node_id,
                game_id,
                tool_id,
                i32::from(enabled),
                settings_json,
                reason,
            ],
        )?;
        if let Some(parent_node_id) = parent_node_id {
            self.conn.execute(
                "INSERT OR IGNORE INTO tool_setting_edges (parent_node_id, child_node_id)
                 VALUES (?1, ?2)",
                params![parent_node_id, node_id],
            )?;
        }
        self.conn.execute(
            "INSERT INTO game_tools (game_id, tool_id, enabled, settings, updated_at, current_node_id)
             VALUES (?1, ?2, ?3, ?4, datetime('now'), ?5)
             ON CONFLICT(game_id, tool_id) DO UPDATE SET
                 enabled = excluded.enabled,
                 settings = excluded.settings,
                 updated_at = excluded.updated_at,
                 current_node_id = excluded.current_node_id",
            params![game_id, tool_id, i32::from(enabled), settings_json, node_id],
        )?;
        Ok(())
    }

    /// Load recent settings history nodes for a tool.
    pub fn list_tool_setting_history(
        &self,
        game_id: &GameId,
        tool_id: &str,
        limit: usize,
    ) -> Result<Vec<ToolSettingHistoryNode>> {
        let current_node_id = self.current_tool_setting_node_id(game_id, tool_id)?;
        let mut stmt = self.conn.prepare(
            "SELECT node_id, game_id, tool_id, enabled, settings, reason, created_at
             FROM tool_setting_nodes
             WHERE game_id = ?1 AND tool_id = ?2
             ORDER BY id DESC
             LIMIT ?3",
        )?;

        let rows = stmt
            .query_map(params![game_id, tool_id, limit as i64], |row| {
                let node_id: String = row.get(0)?;
                Ok(ToolSettingHistoryNode {
                    is_current: current_node_id.as_deref() == Some(node_id.as_str()),
                    node_id,
                    game_id: row.get(1)?,
                    tool_id: row.get(2)?,
                    enabled: row.get::<_, i32>(3)? != 0,
                    settings_json: row.get(4)?,
                    reason: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    /// Load DAG edges for a tool's recorded settings history.
    pub fn list_tool_setting_edges(
        &self,
        game_id: &GameId,
        tool_id: &str,
    ) -> Result<Vec<ToolSettingHistoryEdge>> {
        let mut stmt = self.conn.prepare(
            "SELECT e.parent_node_id, e.child_node_id
             FROM tool_setting_edges e
             JOIN tool_setting_nodes child ON child.node_id = e.child_node_id
             WHERE child.game_id = ?1 AND child.tool_id = ?2
             ORDER BY e.id",
        )?;
        let rows = stmt
            .query_map(params![game_id, tool_id], |row| {
                Ok(ToolSettingHistoryEdge {
                    parent_node_id: row.get(0)?,
                    child_node_id: row.get(1)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    /// Restore a settings node by appending a new child node with copied state.
    pub fn restore_tool_setting_node(
        &self,
        game_id: &GameId,
        tool_id: &str,
        node_id: &str,
    ) -> Result<()> {
        let (enabled, settings_json): (bool, String) = self.conn.query_row(
            "SELECT enabled, settings FROM tool_setting_nodes
             WHERE game_id = ?1 AND tool_id = ?2 AND node_id = ?3",
            params![game_id, tool_id, node_id],
            |row| Ok((row.get::<_, i32>(0)? != 0, row.get(1)?)),
        )?;
        let reason = format!("restore:{node_id}");
        self.save_tool_config_with_reason(game_id, tool_id, enabled, &settings_json, &reason)
    }

    fn current_tool_setting_node_id(
        &self,
        game_id: &GameId,
        tool_id: &str,
    ) -> Result<Option<String>> {
        let result = self.conn.query_row(
            "SELECT current_node_id FROM game_tools WHERE game_id = ?1 AND tool_id = ?2",
            params![game_id, tool_id],
            |row| row.get(0),
        );

        match result {
            Ok(node_id) => Ok(node_id),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Load all tool configurations for a game.
    pub fn load_tool_configs(&self, game_id: &GameId) -> Result<Vec<ToolConfigRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT tool_id, enabled, settings FROM game_tools WHERE game_id = ?1")?;

        let rows = stmt
            .query_map(params![game_id], |row| {
                Ok(ToolConfigRow {
                    tool_id: row.get(0)?,
                    enabled: row.get::<_, i32>(1)? != 0,
                    settings_json: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    /// Load a single tool configuration for a game.
    pub fn load_tool_config(
        &self,
        game_id: &GameId,
        tool_id: &str,
    ) -> Result<Option<ToolConfigRow>> {
        let result = self.conn.query_row(
            "SELECT tool_id, enabled, settings FROM game_tools
             WHERE game_id = ?1 AND tool_id = ?2",
            params![game_id, tool_id],
            |row| {
                Ok(ToolConfigRow {
                    tool_id: row.get(0)?,
                    enabled: row.get::<_, i32>(1)? != 0,
                    settings_json: row.get(2)?,
                })
            },
        );

        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Record files applied by a tool to a game directory.
    pub fn save_applied_files(
        &self,
        game_id: &GameId,
        tool_id: &str,
        rel_paths: &[String],
    ) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "INSERT OR IGNORE INTO tool_applied_files (game_id, tool_id, rel_path)
             VALUES (?1, ?2, ?3)",
        )?;

        for path in rel_paths {
            stmt.execute(params![game_id, tool_id, path])?;
        }

        Ok(())
    }

    /// Load files previously applied by a tool.
    pub fn load_applied_files(&self, game_id: &GameId, tool_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT rel_path FROM tool_applied_files
             WHERE game_id = ?1 AND tool_id = ?2",
        )?;

        let rows = stmt
            .query_map(params![game_id, tool_id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;

        Ok(rows)
    }

    /// Clear all applied file records for a tool on a game.
    pub fn clear_applied_files(&self, game_id: &GameId, tool_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM tool_applied_files WHERE game_id = ?1 AND tool_id = ?2",
            params![game_id, tool_id],
        )?;
        Ok(())
    }

    // ── Executable Config CRUD ───────────────────────────────────

    /// Save or update a named executable for a game.
    pub fn save_executable_config(&self, executable: &ExecutableConfigRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO executable_configs (
                game_id, name, executable_path, arguments, working_dir,
                environment, wine_dll_overrides, output_mod, enabled, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))
             ON CONFLICT(game_id, name) DO UPDATE SET
                executable_path = excluded.executable_path,
                arguments = excluded.arguments,
                working_dir = excluded.working_dir,
                environment = excluded.environment,
                wine_dll_overrides = excluded.wine_dll_overrides,
                output_mod = excluded.output_mod,
                enabled = excluded.enabled,
                updated_at = excluded.updated_at",
            params![
                executable.game_id,
                executable.name,
                executable.executable_path.to_string_lossy().as_ref(),
                executable.arguments_json,
                executable
                    .working_dir
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string()),
                executable.environment_json,
                executable.wine_dll_overrides,
                executable.output_mod,
                i32::from(executable.enabled),
            ],
        )?;
        Ok(())
    }

    /// Load every executable configured for a game, ordered by display name.
    pub fn load_executable_configs(&self, game_id: &GameId) -> Result<Vec<ExecutableConfigRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT game_id, name, executable_path, arguments, working_dir,
                    environment, wine_dll_overrides, output_mod, enabled
             FROM executable_configs
             WHERE game_id = ?1
             ORDER BY name COLLATE NOCASE",
        )?;

        let rows = stmt
            .query_map(params![game_id], executable_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    /// Load a single named executable for a game.
    pub fn load_executable_config(
        &self,
        game_id: &GameId,
        name: &str,
    ) -> Result<Option<ExecutableConfigRow>> {
        let result = self.conn.query_row(
            "SELECT game_id, name, executable_path, arguments, working_dir,
                    environment, wine_dll_overrides, output_mod, enabled
             FROM executable_configs
             WHERE game_id = ?1 AND name = ?2",
            params![game_id, name],
            executable_from_row,
        );

        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Delete a named executable for a game. Returns whether a row was removed.
    pub fn delete_executable_config(&self, game_id: &GameId, name: &str) -> Result<bool> {
        let affected = self.conn.execute(
            "DELETE FROM executable_configs WHERE game_id = ?1 AND name = ?2",
            params![game_id, name],
        )?;
        Ok(affected > 0)
    }
}

fn executable_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExecutableConfigRow> {
    let executable_path: String = row.get(2)?;
    let working_dir: Option<String> = row.get(4)?;
    Ok(ExecutableConfigRow {
        game_id: row.get(0)?,
        name: row.get(1)?,
        executable_path: PathBuf::from(executable_path),
        arguments_json: row.get(3)?,
        working_dir: working_dir.map(PathBuf::from),
        environment_json: row.get(5)?,
        wine_dll_overrides: row.get(6)?,
        output_mod: row.get(7)?,
        enabled: row.get::<_, i32>(8)? != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> ModdeDb {
        ModdeDb::open_memory().unwrap()
    }

    fn sample_profile(name: &str, game_id: &str) -> Profile {
        Profile {
            id: None,
            name: name.to_string(),
            game_id: GameId::from(game_id),
            source: ProfileSource::Manual,
            mods: vec![
                EnabledMod {
                    mod_id: "mod_a".to_string(),
                    enabled: true,
                    version: Some("1.0".to_string()),
                    fomod_config: None,
                    ..Default::default()
                },
                EnabledMod {
                    mod_id: "mod_b".to_string(),
                    enabled: false,
                    version: None,
                    fomod_config: None,
                    ..Default::default()
                },
            ],
            overrides: PathBuf::from("/tmp/overrides"),
            load_order_rules: smallvec::smallvec![LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_b"),
                after: ModId::from("mod_a"),
            }],
            load_order_lock: None,
        }
    }

    #[test]
    fn create_and_load_profile() {
        let db = test_db();
        let profile = sample_profile("test", "skyrim-se");

        let id = db.create_profile(&profile).unwrap();
        assert!(id > 0);

        let loaded = db.load_profile("test", &GameId::from("skyrim-se")).unwrap();
        assert_eq!(loaded.name, "test");
        assert_eq!(loaded.game_id, "skyrim-se");
        assert_eq!(loaded.mods.len(), 2);
        assert_eq!(loaded.mods[0].mod_id, "mod_a");
        assert!(loaded.mods[0].enabled);
        assert_eq!(loaded.mods[1].mod_id, "mod_b");
        assert!(!loaded.mods[1].enabled);
        assert_eq!(loaded.load_order_rules.len(), 1);
    }

    #[test]
    fn nexus_ids_roundtrip_with_unchanged_sqlite_values() {
        let db = test_db();
        let mut profile = sample_profile("test", "skyrim-se");
        profile.mods[0].nexus_mod_id = Some(NexusModId::from(42));
        profile.mods[0].nexus_file_id = Some(NexusFileId::from(99));

        db.create_profile(&profile).unwrap();

        let stored: (i64, i64) = db
            .conn
            .query_row(
                "SELECT nexus_mod_id, nexus_file_id FROM profile_mods WHERE mod_id = 'mod_a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored, (42, 99));

        let loaded = db.load_profile("test", &GameId::from("skyrim-se")).unwrap();
        assert_eq!(loaded.mods[0].nexus_mod_id, Some(NexusModId::from(42)));
        assert_eq!(loaded.mods[0].nexus_file_id, Some(NexusFileId::from(99)));
    }

    #[test]
    fn negative_nexus_ids_fail_closed_on_load() {
        let db = test_db();
        let profile = sample_profile("test", "skyrim-se");
        db.create_profile(&profile).unwrap();
        db.conn
            .execute(
                "UPDATE profile_mods SET nexus_mod_id = -1 WHERE mod_id = 'mod_a'",
                [],
            )
            .unwrap();

        let err = db
            .load_profile("test", &GameId::from("skyrim-se"))
            .unwrap_err();
        assert!(matches!(err, CoreError::Database(_)));
    }

    #[test]
    fn legacy_installer_metadata_loads_typed_and_roundtrips_storage() {
        let db = test_db();
        let profile = sample_profile("test", "skyrim-se");
        db.create_profile(&profile).unwrap();

        let method_raw = encode_install_method(&InstallMethod::BareExtract).unwrap();
        let tags_raw = r#"["quest","ui"]"#;
        db.conn
            .execute(
                "UPDATE profile_mods
                    SET install_status = ?1, install_method = ?2, tags = ?3
                  WHERE mod_id = 'mod_a'",
                params!["pending_user_input", method_raw, tags_raw],
            )
            .unwrap();

        let loaded = db.load_profile("test", &GameId::from("skyrim-se")).unwrap();
        assert_eq!(
            loaded.mods[0].install_status,
            Some(InstallStatus::PendingUserInput)
        );
        assert_eq!(
            loaded.mods[0].install_method,
            Some(InstallMethod::BareExtract)
        );
        assert_eq!(
            loaded.mods[0].tags,
            vec!["quest".to_string(), "ui".to_string()]
        );
        assert_eq!(loaded.mods[1].install_status, None);

        db.update_profile(&loaded).unwrap();
        let stored: (String, String, String) = db
            .conn
            .query_row(
                "SELECT install_status, install_method, tags
                   FROM profile_mods
                  WHERE mod_id = 'mod_a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            stored,
            (
                "pending_user_input".to_string(),
                method_raw,
                tags_raw.to_string()
            )
        );
    }

    #[test]
    fn load_by_name_unique() {
        let db = test_db();
        let profile = sample_profile("default", "skyrim-se");
        db.create_profile(&profile).unwrap();

        let loaded = db.load_profile_by_name("default").unwrap();
        assert_eq!(loaded.game_id, "skyrim-se");
    }

    #[test]
    fn load_by_name_ambiguous() {
        let db = test_db();
        db.create_profile(&sample_profile("default", "skyrim-se"))
            .unwrap();
        db.create_profile(&sample_profile("default", "fallout4"))
            .unwrap();

        let err = db.load_profile_by_name("default").unwrap_err();
        match err {
            CoreError::AmbiguousProfile { name, games } => {
                assert_eq!(name, "default");
                assert!(games.contains(&GameId::from("skyrim-se")));
                assert!(games.contains(&GameId::from("fallout4")));
            }
            other => panic!("expected AmbiguousProfile, got: {other}"),
        }
    }

    #[test]
    fn multi_profile_per_game() {
        let db = test_db();
        db.create_profile(&sample_profile("vanilla", "skyrim-se"))
            .unwrap();
        db.create_profile(&sample_profile("modded", "skyrim-se"))
            .unwrap();
        db.create_profile(&sample_profile("hardcore", "skyrim-se"))
            .unwrap();

        let profiles = db.list_profiles(Some(&GameId::from("skyrim-se"))).unwrap();
        assert_eq!(profiles.len(), 3);
    }

    #[test]
    fn update_profile() {
        let db = test_db();
        let mut profile = sample_profile("test", "skyrim-se");
        db.create_profile(&profile).unwrap();

        profile.mods.push(EnabledMod {
            mod_id: "mod_c".to_string(),
            enabled: true,
            version: None,
            fomod_config: None,
            ..Default::default()
        });

        db.update_profile(&profile).unwrap();

        let loaded = db.load_profile("test", &GameId::from("skyrim-se")).unwrap();
        assert_eq!(loaded.mods.len(), 3);
    }

    #[test]
    fn delete_profile() {
        let db = test_db();
        db.create_profile(&sample_profile("test", "skyrim-se"))
            .unwrap();
        db.delete_profile("test", &GameId::from("skyrim-se"))
            .unwrap();

        let err = db
            .load_profile("test", &GameId::from("skyrim-se"))
            .unwrap_err();
        assert!(matches!(err, CoreError::ProfileNotFound(_)));
    }

    #[test]
    fn delete_cascades_to_mods_and_saves() {
        let db = test_db();
        let id = db
            .create_profile(&sample_profile("test", "skyrim-se"))
            .unwrap();
        db.assign_save(id, Path::new("/saves/save1.ess"), Some("my save"))
            .unwrap();

        let saves = db.list_saves(id).unwrap();
        assert_eq!(saves.len(), 1);

        db.delete_profile("test", &GameId::from("skyrim-se"))
            .unwrap();

        // Saves and mods should be cascade-deleted
        let saves = db.list_saves(id).unwrap();
        assert_eq!(saves.len(), 0);
    }

    #[test]
    fn save_assignment() {
        let db = test_db();
        let id = db
            .create_profile(&sample_profile("test", "skyrim-se"))
            .unwrap();

        db.assign_save(id, Path::new("/saves/save1.ess"), Some("Level 50"))
            .unwrap();
        db.assign_save(id, Path::new("/saves/save2.ess"), None)
            .unwrap();

        let saves = db.list_saves(id).unwrap();
        assert_eq!(saves.len(), 2);
        assert_eq!(saves[0].label.as_deref(), Some("Level 50"));
        assert!(saves[1].label.is_none());

        db.unassign_save(Path::new("/saves/save1.ess")).unwrap();
        let saves = db.list_saves(id).unwrap();
        assert_eq!(saves.len(), 1);
    }

    #[test]
    fn save_already_assigned_to_different_profile() {
        let db = test_db();
        let id1 = db
            .create_profile(&sample_profile("profile1", "skyrim-se"))
            .unwrap();
        let id2 = db
            .create_profile(&sample_profile("profile2", "skyrim-se"))
            .unwrap();

        db.assign_save(id1, Path::new("/saves/save1.ess"), None)
            .unwrap();

        let err = db
            .assign_save(id2, Path::new("/saves/save1.ess"), None)
            .unwrap_err();
        assert!(matches!(err, CoreError::SaveAlreadyAssigned { .. }));
    }

    #[test]
    fn snapshot_upsert_and_get() {
        let db = test_db();

        db.upsert_snapshot(
            &GameId::from("skyrim-se"),
            Path::new("/stock/skyrim-se"),
            "abc123",
            5000,
        )
        .unwrap();
        let meta = db
            .get_snapshot(&GameId::from("skyrim-se"))
            .unwrap()
            .unwrap();
        assert_eq!(meta.tree_hash, "abc123");
        assert_eq!(meta.file_count, 5000);

        // Upsert updates
        db.upsert_snapshot(
            &GameId::from("skyrim-se"),
            Path::new("/stock/skyrim-se"),
            "def456",
            5001,
        )
        .unwrap();
        let meta = db
            .get_snapshot(&GameId::from("skyrim-se"))
            .unwrap()
            .unwrap();
        assert_eq!(meta.tree_hash, "def456");
        assert_eq!(meta.file_count, 5001);
    }

    #[test]
    fn snapshot_not_found() {
        let db = test_db();
        assert!(
            db.get_snapshot(&GameId::from("nonexistent"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn list_profiles_all_and_by_game() {
        let db = test_db();
        db.create_profile(&sample_profile("vanilla", "skyrim-se"))
            .unwrap();
        db.create_profile(&sample_profile("modded", "skyrim-se"))
            .unwrap();
        db.create_profile(&sample_profile("default", "fallout4"))
            .unwrap();

        let all = db.list_profiles(None).unwrap();
        assert_eq!(all.len(), 3);

        let skyrim = db.list_profiles(Some(&GameId::from("skyrim-se"))).unwrap();
        assert_eq!(skyrim.len(), 2);

        let fallout = db.list_profiles(Some(&GameId::from("fallout4"))).unwrap();
        assert_eq!(fallout.len(), 1);
    }

    #[test]
    fn source_roundtrip_nexus_collection() {
        let db = test_db();
        let mut profile = sample_profile("test", "skyrim-se");
        profile.source = ProfileSource::NexusCollection {
            slug: "my-collection".to_string(),
            version: "1.2.3".to_string(),
        };

        db.create_profile(&profile).unwrap();
        let loaded = db.load_profile("test", &GameId::from("skyrim-se")).unwrap();

        match loaded.source {
            ProfileSource::NexusCollection { slug, version } => {
                assert_eq!(slug, "my-collection");
                assert_eq!(version, "1.2.3");
            }
            other => panic!("expected NexusCollection, got: {other:?}"),
        }
    }

    #[test]
    fn source_roundtrip_wabbajack() {
        let db = test_db();
        let mut profile = sample_profile("test", "skyrim-se");
        profile.source = ProfileSource::Wabbajack {
            manifest_hash: "deadbeef".to_string(),
        };

        db.create_profile(&profile).unwrap();
        let loaded = db.load_profile("test", &GameId::from("skyrim-se")).unwrap();

        match loaded.source {
            ProfileSource::Wabbajack { manifest_hash } => {
                assert_eq!(manifest_hash, "deadbeef");
            }
            other => panic!("expected Wabbajack, got: {other:?}"),
        }
    }

    #[test]
    fn profile_not_found() {
        let db = test_db();
        let err = db
            .load_profile("nonexistent", &GameId::from("skyrim-se"))
            .unwrap_err();
        assert!(matches!(err, CoreError::ProfileNotFound(_)));
    }

    #[test]
    fn duplicate_profile_errors() {
        let db = test_db();
        db.create_profile(&sample_profile("test", "skyrim-se"))
            .unwrap();

        let err = db
            .create_profile(&sample_profile("test", "skyrim-se"))
            .unwrap_err();
        assert!(matches!(err, CoreError::Database(_)));
    }

    #[test]
    fn executable_config_roundtrip() {
        let db = test_db();
        let row = ExecutableConfigRow {
            game_id: "skyrim-se".to_string(),
            name: "xEdit".to_string(),
            executable_path: PathBuf::from("/tools/SSEEdit.exe"),
            arguments_json: serde_json::json!(["-IKnowWhatImDoing"]).to_string(),
            working_dir: Some(PathBuf::from("/games/Skyrim Special Edition")),
            environment_json: serde_json::json!({"WINESYNC": "1"}).to_string(),
            wine_dll_overrides: Some("dinput8=n,b".to_string()),
            output_mod: "xedit-output".to_string(),
            enabled: true,
        };

        db.save_executable_config(&row).unwrap();
        let loaded = db
            .load_executable_config(&GameId::from("skyrim-se"), "xEdit")
            .unwrap()
            .unwrap();
        assert_eq!(loaded, row);

        let all = db
            .load_executable_configs(&GameId::from("skyrim-se"))
            .unwrap();
        assert_eq!(all.len(), 1);
        assert!(
            db.delete_executable_config(&GameId::from("skyrim-se"), "xEdit")
                .unwrap()
        );
        assert!(
            db.load_executable_config(&GameId::from("skyrim-se"), "xEdit")
                .unwrap()
                .is_none()
        );
    }
}
