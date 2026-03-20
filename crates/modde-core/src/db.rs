use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use smallvec::SmallVec;
use tracing::info;

use crate::error::{CoreError, Result};
use crate::profile::{EnabledMod, Profile, ProfileSource};
use crate::resolver::{GameId, LoadOrderRule, ModId};

const CURRENT_SCHEMA_VERSION: u32 = 1;

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

        if version < CURRENT_SCHEMA_VERSION {
            self.conn.execute_batch(SCHEMA_V1)?;
            self.conn
                .pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)?;
            info!(from = version, to = CURRENT_SCHEMA_VERSION, "database schema migrated");
        }

        // Ensure WAL and FK are always on (they reset per-connection).
        self.conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;

        Ok(())
    }

    // ── Profile CRUD ──────────────────────────────────────────────

    /// Create a new profile, returning its database ID.
    pub fn create_profile(&self, profile: &Profile) -> Result<i64> {
        let (source_type, source_data) = encode_source(&profile.source);

        self.conn.execute(
            "INSERT INTO profiles (name, game_id, source_type, source_data, overrides)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                profile.name,
                profile.game_id,
                source_type,
                source_data,
                profile.overrides.to_string_lossy().as_ref(),
            ],
        )?;

        let profile_id = self.conn.last_insert_rowid();

        self.insert_mods(profile_id, &profile.mods)?;
        self.insert_rules(profile_id, &profile.load_order_rules)?;

        Ok(profile_id)
    }

    /// Load a profile by name and game_id.
    pub fn load_profile(&self, name: &str, game_id: &str) -> Result<Profile> {
        let (id, source_type, source_data, overrides) = self
            .conn
            .query_row(
                "SELECT id, source_type, source_data, overrides FROM profiles
                 WHERE name = ?1 AND game_id = ?2",
                params![name, game_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    CoreError::ProfileNotFound(format!("{name} (game: {game_id})"))
                }
                other => CoreError::Database(other),
            })?;

        self.assemble_profile(id, name, game_id, &source_type, source_data.as_deref(), &overrides)
    }

    /// Load a profile by its database ID.
    pub fn load_profile_by_id(&self, id: i64) -> Result<Profile> {
        let (name, game_id, source_type, source_data, overrides) = self
            .conn
            .query_row(
                "SELECT name, game_id, source_type, source_data, overrides FROM profiles
                 WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    CoreError::ProfileNotFound(format!("id={id}"))
                }
                other => CoreError::Database(other),
            })?;

        self.assemble_profile(id, &name, &game_id, &source_type, source_data.as_deref(), &overrides)
    }

    /// Load a profile by name only. Errors with `AmbiguousProfile` if multiple games match.
    pub fn load_profile_by_name(&self, name: &str) -> Result<Profile> {
        let mut stmt = self.conn.prepare(
            "SELECT id, game_id, source_type, source_data, overrides FROM profiles WHERE name = ?1",
        )?;

        let rows: Vec<(i64, String, String, Option<String>, String)> = stmt
            .query_map(params![name], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        match rows.len() {
            0 => Err(CoreError::ProfileNotFound(name.to_string())),
            1 => {
                let (id, game_id, source_type, source_data, overrides) = &rows[0];
                self.assemble_profile(*id, name, game_id, source_type, source_data.as_deref(), overrides)
            }
            _ => {
                let games: SmallVec<[GameId; 4]> = rows.iter().map(|(_, g, _, _, _)| GameId::from(g.clone())).collect();
                Err(CoreError::AmbiguousProfile {
                    name: name.to_string(),
                    games,
                })
            }
        }
    }

    /// Update an existing profile (identified by name + game_id).
    pub fn update_profile(&self, profile: &Profile) -> Result<()> {
        let (source_type, source_data) = encode_source(&profile.source);

        let profile_id: i64 = self
            .conn
            .query_row(
                "SELECT id FROM profiles WHERE name = ?1 AND game_id = ?2",
                params![profile.name, profile.game_id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    CoreError::ProfileNotFound(format!("{} (game: {})", profile.name, profile.game_id))
                }
                other => CoreError::Database(other),
            })?;

        self.conn.execute(
            "UPDATE profiles SET source_type = ?1, source_data = ?2, overrides = ?3,
                    updated_at = datetime('now')
             WHERE id = ?4",
            params![
                source_type,
                source_data,
                profile.overrides.to_string_lossy().as_ref(),
                profile_id,
            ],
        )?;

        // Replace mods and rules
        self.conn
            .execute("DELETE FROM profile_mods WHERE profile_id = ?1", params![profile_id])?;
        self.conn
            .execute("DELETE FROM load_order_rules WHERE profile_id = ?1", params![profile_id])?;

        self.insert_mods(profile_id, &profile.mods)?;
        self.insert_rules(profile_id, &profile.load_order_rules)?;

        Ok(())
    }

    /// Delete a profile by name and game_id.
    pub fn delete_profile(&self, name: &str, game_id: &str) -> Result<()> {
        let changes = self.conn.execute(
            "DELETE FROM profiles WHERE name = ?1 AND game_id = ?2",
            params![name, game_id],
        )?;
        if changes == 0 {
            return Err(CoreError::ProfileNotFound(format!("{name} (game: {game_id})")));
        }
        Ok(())
    }

    /// List profile summaries, optionally filtered by game.
    pub fn list_profiles(&self, game_id: Option<&str>) -> Result<Vec<ProfileSummary>> {
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
                mod_count: row.get(4)?,
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
        self.conn
            .execute("DELETE FROM saves WHERE path = ?1", params![path_str.as_ref()])?;
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
    pub fn set_active_profile(&self, game_id: &str, profile_id: i64) -> Result<()> {
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

    /// Get the active profile for a game, returning (profile_id, profile_name).
    pub fn get_active_profile(&self, game_id: &str) -> Result<Option<(i64, String)>> {
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
    pub fn clear_active_profile(&self, game_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM active_profiles WHERE game_id = ?1",
            params![game_id],
        )?;
        Ok(())
    }

    // ── Experiment Stack ──────────────────────────────────────────

    /// Push a profile onto the experiment stack for a game.
    pub fn push_experiment(&self, game_id: &str, profile_id: i64) -> Result<()> {
        let depth = self.experiment_depth(game_id)?;
        self.conn.execute(
            "INSERT INTO experiment_stack (game_id, profile_id, depth)
             VALUES (?1, ?2, ?3)",
            params![game_id, profile_id, depth as i64],
        )?;
        Ok(())
    }

    /// Pop the top entry from the experiment stack, returning the profile_id.
    pub fn pop_experiment(&self, game_id: &str) -> Result<Option<i64>> {
        let result = self.conn.query_row(
            "SELECT id, profile_id FROM experiment_stack
             WHERE game_id = ?1 ORDER BY depth DESC LIMIT 1",
            params![game_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        );

        match result {
            Ok((id, profile_id)) => {
                self.conn.execute(
                    "DELETE FROM experiment_stack WHERE id = ?1",
                    params![id],
                )?;
                Ok(Some(profile_id))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get the experiment stack depth for a game.
    pub fn experiment_depth(&self, game_id: &str) -> Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM experiment_stack WHERE game_id = ?1",
            params![game_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Clear the entire experiment stack for a game.
    pub fn clear_experiment_stack(&self, game_id: &str) -> Result<()> {
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
        game_id: &str,
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
    pub fn get_snapshot(&self, game_id: &str) -> Result<Option<SnapshotMeta>> {
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
            let profile: Profile = match toml::from_str(&content) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(path = %toml_path.display(), error = %e, "skipping unparseable profile");
                    continue;
                }
            };

            // Skip if already in DB
            let exists: bool = self
                .conn
                .query_row(
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

    fn insert_mods(&self, profile_id: i64, mods: &[EnabledMod]) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "INSERT INTO profile_mods (profile_id, mod_id, enabled, version, fomod_config, sort_index)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;

        for (idx, m) in mods.iter().enumerate() {
            stmt.execute(params![
                profile_id,
                m.mod_id,
                m.enabled,
                m.version,
                m.fomod_config,
                idx as i64,
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
                LoadOrderRule::LoadAfter { mod_id, after } => ("load_after", mod_id.as_str(), after.as_str()),
                LoadOrderRule::LoadBefore { mod_id, before } => ("load_before", mod_id.as_str(), before.as_str()),
                LoadOrderRule::Incompatible { mod_a, mod_b } => ("incompatible", mod_a.as_str(), mod_b.as_str()),
            };
            stmt.execute(params![profile_id, rule_type, mod_a, mod_b])?;
        }

        Ok(())
    }

    fn load_mods(&self, profile_id: i64) -> Result<Vec<EnabledMod>> {
        let mut stmt = self.conn.prepare(
            "SELECT mod_id, enabled, version, fomod_config
             FROM profile_mods WHERE profile_id = ?1 ORDER BY sort_index",
        )?;

        let mods = stmt
            .query_map(params![profile_id], |row| {
                Ok(EnabledMod {
                    mod_id: row.get(0)?,
                    enabled: row.get(1)?,
                    version: row.get(2)?,
                    fomod_config: row.get(3)?,
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
        game_id: &str,
        source_type: &str,
        source_data: Option<&str>,
        overrides: &str,
    ) -> Result<Profile> {
        let source = decode_source(source_type, source_data)?;
        let mods = self.load_mods(id)?;
        let load_order_rules = self.load_rules(id)?;

        Ok(Profile {
            id: Some(id),
            name: name.to_string(),
            game_id: GameId::from(game_id),
            source,
            mods,
            overrides: PathBuf::from(overrides),
            load_order_rules,
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
                CoreError::Other(format!("failed to parse nexus_collection source data: {e}").into())
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
        other => Err(CoreError::Other(format!(
            "unknown profile source type: {other}"
        ).into())),
    }
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
                },
                EnabledMod {
                    mod_id: "mod_b".to_string(),
                    enabled: false,
                    version: None,
                    fomod_config: None,
                },
            ],
            overrides: PathBuf::from("/tmp/overrides"),
            load_order_rules: smallvec::smallvec![LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_b"),
                after: ModId::from("mod_a"),
            }],
        }
    }

    #[test]
    fn create_and_load_profile() {
        let db = test_db();
        let profile = sample_profile("test", "skyrim-se");

        let id = db.create_profile(&profile).unwrap();
        assert!(id > 0);

        let loaded = db.load_profile("test", "skyrim-se").unwrap();
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
        db.create_profile(&sample_profile("default", "skyrim-se")).unwrap();
        db.create_profile(&sample_profile("default", "fallout4")).unwrap();

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
        db.create_profile(&sample_profile("vanilla", "skyrim-se")).unwrap();
        db.create_profile(&sample_profile("modded", "skyrim-se")).unwrap();
        db.create_profile(&sample_profile("hardcore", "skyrim-se")).unwrap();

        let profiles = db.list_profiles(Some("skyrim-se")).unwrap();
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
        });

        db.update_profile(&profile).unwrap();

        let loaded = db.load_profile("test", "skyrim-se").unwrap();
        assert_eq!(loaded.mods.len(), 3);
    }

    #[test]
    fn delete_profile() {
        let db = test_db();
        db.create_profile(&sample_profile("test", "skyrim-se")).unwrap();
        db.delete_profile("test", "skyrim-se").unwrap();

        let err = db.load_profile("test", "skyrim-se").unwrap_err();
        assert!(matches!(err, CoreError::ProfileNotFound(_)));
    }

    #[test]
    fn delete_cascades_to_mods_and_saves() {
        let db = test_db();
        let id = db.create_profile(&sample_profile("test", "skyrim-se")).unwrap();
        db.assign_save(id, Path::new("/saves/save1.ess"), Some("my save")).unwrap();

        let saves = db.list_saves(id).unwrap();
        assert_eq!(saves.len(), 1);

        db.delete_profile("test", "skyrim-se").unwrap();

        // Saves and mods should be cascade-deleted
        let saves = db.list_saves(id).unwrap();
        assert_eq!(saves.len(), 0);
    }

    #[test]
    fn save_assignment() {
        let db = test_db();
        let id = db.create_profile(&sample_profile("test", "skyrim-se")).unwrap();

        db.assign_save(id, Path::new("/saves/save1.ess"), Some("Level 50")).unwrap();
        db.assign_save(id, Path::new("/saves/save2.ess"), None).unwrap();

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
        let id1 = db.create_profile(&sample_profile("profile1", "skyrim-se")).unwrap();
        let id2 = db.create_profile(&sample_profile("profile2", "skyrim-se")).unwrap();

        db.assign_save(id1, Path::new("/saves/save1.ess"), None).unwrap();

        let err = db.assign_save(id2, Path::new("/saves/save1.ess"), None).unwrap_err();
        assert!(matches!(err, CoreError::SaveAlreadyAssigned { .. }));
    }

    #[test]
    fn snapshot_upsert_and_get() {
        let db = test_db();

        db.upsert_snapshot("skyrim-se", Path::new("/stock/skyrim-se"), "abc123", 5000).unwrap();
        let meta = db.get_snapshot("skyrim-se").unwrap().unwrap();
        assert_eq!(meta.tree_hash, "abc123");
        assert_eq!(meta.file_count, 5000);

        // Upsert updates
        db.upsert_snapshot("skyrim-se", Path::new("/stock/skyrim-se"), "def456", 5001).unwrap();
        let meta = db.get_snapshot("skyrim-se").unwrap().unwrap();
        assert_eq!(meta.tree_hash, "def456");
        assert_eq!(meta.file_count, 5001);
    }

    #[test]
    fn snapshot_not_found() {
        let db = test_db();
        assert!(db.get_snapshot("nonexistent").unwrap().is_none());
    }

    #[test]
    fn list_profiles_all_and_by_game() {
        let db = test_db();
        db.create_profile(&sample_profile("vanilla", "skyrim-se")).unwrap();
        db.create_profile(&sample_profile("modded", "skyrim-se")).unwrap();
        db.create_profile(&sample_profile("default", "fallout4")).unwrap();

        let all = db.list_profiles(None).unwrap();
        assert_eq!(all.len(), 3);

        let skyrim = db.list_profiles(Some("skyrim-se")).unwrap();
        assert_eq!(skyrim.len(), 2);

        let fallout = db.list_profiles(Some("fallout4")).unwrap();
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
        let loaded = db.load_profile("test", "skyrim-se").unwrap();

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
        let loaded = db.load_profile("test", "skyrim-se").unwrap();

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
        let err = db.load_profile("nonexistent", "skyrim-se").unwrap_err();
        assert!(matches!(err, CoreError::ProfileNotFound(_)));
    }

    #[test]
    fn duplicate_profile_errors() {
        let db = test_db();
        db.create_profile(&sample_profile("test", "skyrim-se")).unwrap();

        let err = db.create_profile(&sample_profile("test", "skyrim-se")).unwrap_err();
        assert!(matches!(err, CoreError::Database(_)));
    }
}
