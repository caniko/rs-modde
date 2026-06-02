//! Persistent storage for modde, backed by `SQLite` (default) or `PostgreSQL`.
//!
//! The public [`ModdeDb`] API is identical across both backends and async
//! throughout. Each method is written once against the [`backend::Db`] executor
//! using portable SQL (`?` placeholders, `RETURNING id`, `ON CONFLICT … DO
//! UPDATE … EXCLUDED`, `lower(name)`, `bool` columns, and a `{NOW}` token);
//! the executor rewrites placeholders/`now()` per dialect. The only genuinely
//! per-backend code is schema creation/migration in [`migrate`].

mod backend;
mod migrate;

use std::path::{Path, PathBuf};
use std::str::FromStr;

use smallvec::SmallVec;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

use crate::error::{CoreError, Result};
use crate::installer::{InstallMethod, InstallPlan, InstallStatus, StagedFile};
use crate::nexus_id::{NexusFileId, NexusModId};
use crate::profile::{EnabledMod, LoadOrderLock, LockReason, Profile, ProfileSource};
use crate::resolver::{GameId, LoadOrderRule, ModId};
use crate::settings::{AppSettings, DatabaseSettings, DbBackend};

pub use backend::Val;
use backend::{Db, DbRow, vals};

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

/// Per-game tool configuration stored in the database.
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

/// SQLite/PostgreSQL-backed persistent storage for modde.
#[derive(Debug, Clone)]
pub struct ModdeDb {
    db: Db,
}

impl ModdeDb {
    /// Open the database selected by configuration (settings + environment),
    /// creating/migrating it as needed. Defaults to `SQLite` at the XDG path.
    pub async fn open() -> Result<Self> {
        let settings = AppSettings::load();
        Self::open_with_settings(&settings).await
    }

    /// Open the database described by `settings`, honoring the
    /// `MODDE_DATABASE_*` environment overrides used by the Home Manager module.
    pub async fn open_with_settings(settings: &AppSettings) -> Result<Self> {
        let backend = std::env::var("MODDE_DATABASE_BACKEND")
            .ok()
            .and_then(|v| DbBackend::parse(&v))
            .unwrap_or(settings.database.backend);
        match backend {
            DbBackend::Sqlite => Self::open_at(&crate::paths::db_path()).await,
            DbBackend::Postgres => Self::open_postgres(&settings.database).await,
        }
    }

    /// Open a `SQLite` database at a specific path, creating it if needed.
    pub async fn open_at(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new().connect_with(opts).await?;
        migrate::migrate_sqlite(&pool).await?;
        Ok(Self {
            db: Db::Sqlite(pool),
        })
    }

    /// Open an in-memory `SQLite` database (for testing).
    pub async fn open_memory() -> Result<Self> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        migrate::migrate_sqlite(&pool).await?;
        Ok(Self {
            db: Db::Sqlite(pool),
        })
    }

    #[cfg(feature = "postgres")]
    async fn open_postgres(settings: &DatabaseSettings) -> Result<Self> {
        use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

        let url = std::env::var("MODDE_DATABASE_URL")
            .ok()
            .or_else(|| settings.url.clone());

        let mut opts = if let Some(url) = url {
            PgConnectOptions::from_str(&url)?
        } else {
            let mut o = PgConnectOptions::new();
            if let Some(host) = &settings.host {
                o = o.host(host);
            }
            if let Some(port) = settings.port {
                o = o.port(port);
            }
            let dbname = settings.dbname.as_deref().ok_or_else(|| {
                CoreError::Other("postgres backend selected but no database name configured".into())
            })?;
            o = o.database(dbname);
            if let Some(user) = &settings.user {
                o = o.username(user);
            }
            o
        };

        let pw_path = std::env::var("MODDE_DB_PASSWORD_FILE")
            .ok()
            .map(PathBuf::from)
            .or_else(|| settings.password_file.clone());
        if let Some(path) = pw_path {
            let pw = std::fs::read_to_string(&path)?;
            opts = opts.password(pw.trim());
        }

        let pool = PgPoolOptions::new().connect_with(opts).await?;
        migrate::migrate_postgres(&pool).await?;
        Ok(Self {
            db: Db::Postgres(pool),
        })
    }

    #[cfg(not(feature = "postgres"))]
    async fn open_postgres(_settings: &DatabaseSettings) -> Result<Self> {
        Err(CoreError::Other(
            "PostgreSQL backend requested but modde was built without the `postgres` feature"
                .into(),
        ))
    }

    // ── Profile CRUD ──────────────────────────────────────────────

    /// Create a new profile, returning its database ID.
    pub async fn create_profile(&self, profile: &Profile) -> Result<i64> {
        let (source_type, source_data) = encode_source(&profile.source);
        let load_order_lock = encode_lock(profile.load_order_lock.as_ref());

        let id = self
            .db
            .fetch_one(
                "INSERT INTO profiles (name, game_id, source_type, source_data, overrides, load_order_lock)
                 VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
                &vals![
                    profile.name.clone(),
                    &profile.game_id,
                    source_type,
                    source_data,
                    profile.overrides.to_string_lossy().to_string(),
                    load_order_lock,
                ],
                |r| r.i64(0),
            )
            .await?;

        self.insert_mods(id, &profile.mods).await?;
        self.insert_rules(id, &profile.load_order_rules).await?;

        Ok(id)
    }

    /// Load a profile by name and `game_id`.
    pub async fn load_profile(&self, name: &str, game_id: &GameId) -> Result<Profile> {
        let row = self
            .db
            .fetch_optional(
                "SELECT id, source_type, source_data, overrides, load_order_lock FROM profiles
                 WHERE name = ? AND game_id = ?",
                &vals![name, game_id],
                |r| {
                    Ok((
                        r.i64(0)?,
                        r.string(1)?,
                        r.opt_string(2)?,
                        r.string(3)?,
                        r.opt_string(4)?,
                    ))
                },
            )
            .await?;

        let (id, source_type, source_data, overrides, load_order_lock) =
            row.ok_or_else(|| CoreError::ProfileNotFound(format!("{name} (game: {game_id})")))?;

        self.assemble_profile(
            id,
            name,
            game_id,
            &source_type,
            source_data.as_deref(),
            &overrides,
            load_order_lock.as_deref(),
        )
        .await
    }

    /// Load a profile by its database ID.
    pub async fn load_profile_by_id(&self, id: i64) -> Result<Profile> {
        let row = self
            .db
            .fetch_optional(
                "SELECT name, game_id, source_type, source_data, overrides, load_order_lock
                 FROM profiles WHERE id = ?",
                &vals![id],
                |r| {
                    Ok((
                        r.string(0)?,
                        r.string(1)?,
                        r.string(2)?,
                        r.opt_string(3)?,
                        r.string(4)?,
                        r.opt_string(5)?,
                    ))
                },
            )
            .await?;

        let (name, game_id, source_type, source_data, overrides, load_order_lock) =
            row.ok_or_else(|| CoreError::ProfileNotFound(format!("id={id}")))?;

        self.assemble_profile(
            id,
            &name,
            &GameId::from(game_id),
            &source_type,
            source_data.as_deref(),
            &overrides,
            load_order_lock.as_deref(),
        )
        .await
    }

    /// Load a profile by name only. Errors with `AmbiguousProfile` if multiple games match.
    pub async fn load_profile_by_name(&self, name: &str) -> Result<Profile> {
        let rows = self
            .db
            .fetch_all(
                "SELECT id, game_id, source_type, source_data, overrides, load_order_lock
                 FROM profiles WHERE name = ?",
                &vals![name],
                |r| {
                    Ok((
                        r.i64(0)?,
                        r.string(1)?,
                        r.string(2)?,
                        r.opt_string(3)?,
                        r.string(4)?,
                        r.opt_string(5)?,
                    ))
                },
            )
            .await?;

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
                .await
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
    pub async fn update_profile(&self, profile: &Profile) -> Result<()> {
        let (source_type, source_data) = encode_source(&profile.source);
        let load_order_lock = encode_lock(profile.load_order_lock.as_ref());

        let profile_id = self
            .db
            .fetch_optional(
                "SELECT id FROM profiles WHERE name = ? AND game_id = ?",
                &vals![profile.name.clone(), &profile.game_id],
                |r| r.i64(0),
            )
            .await?
            .ok_or_else(|| {
                CoreError::ProfileNotFound(format!("{} (game: {})", profile.name, profile.game_id))
            })?;

        self.db
            .execute(
                "UPDATE profiles SET source_type = ?, source_data = ?, overrides = ?,
                        load_order_lock = ?, updated_at = {NOW}
                 WHERE id = ?",
                &vals![
                    source_type,
                    source_data,
                    profile.overrides.to_string_lossy().to_string(),
                    load_order_lock,
                    profile_id,
                ],
            )
            .await?;

        self.db
            .execute(
                "DELETE FROM profile_mods WHERE profile_id = ?",
                &vals![profile_id],
            )
            .await?;
        self.db
            .execute(
                "DELETE FROM load_order_rules WHERE profile_id = ?",
                &vals![profile_id],
            )
            .await?;

        self.insert_mods(profile_id, &profile.mods).await?;
        self.insert_rules(profile_id, &profile.load_order_rules)
            .await?;

        Ok(())
    }

    /// Delete a profile by name and `game_id`.
    pub async fn delete_profile(&self, name: &str, game_id: &GameId) -> Result<()> {
        let changes = self
            .db
            .execute(
                "DELETE FROM profiles WHERE name = ? AND game_id = ?",
                &vals![name, game_id],
            )
            .await?;
        if changes == 0 {
            return Err(CoreError::ProfileNotFound(format!(
                "{name} (game: {game_id})"
            )));
        }
        Ok(())
    }

    /// List profile summaries, optionally filtered by game.
    pub async fn list_profiles(&self, game_id: Option<&GameId>) -> Result<Vec<ProfileSummary>> {
        let mapper = |r: &dyn DbRow| {
            Ok(ProfileSummary {
                id: r.i64(0)?,
                name: r.string(1)?,
                game_id: GameId::from(r.string(2)?),
                source_type: r.string(3)?,
                mod_count: r.i64(4)? as usize,
            })
        };

        match game_id {
            Some(gid) => {
                self.db
                    .fetch_all(
                        "SELECT p.id, p.name, p.game_id, p.source_type,
                                (SELECT COUNT(*) FROM profile_mods WHERE profile_id = p.id) as mod_count
                         FROM profiles p WHERE p.game_id = ? ORDER BY p.name",
                        &vals![gid],
                        mapper,
                    )
                    .await
            }
            None => {
                self.db
                    .fetch_all(
                        "SELECT p.id, p.name, p.game_id, p.source_type,
                                (SELECT COUNT(*) FROM profile_mods WHERE profile_id = p.id) as mod_count
                         FROM profiles p ORDER BY p.game_id, p.name",
                        &[],
                        mapper,
                    )
                    .await
            }
        }
    }

    // ── Save CRUD ─────────────────────────────────────────────────

    /// Assign a save to a profile.
    pub async fn assign_save(
        &self,
        profile_id: i64,
        path: &Path,
        label: Option<&str>,
    ) -> Result<()> {
        let path_str = path.to_string_lossy().to_string();

        let existing = self
            .db
            .fetch_optional(
                "SELECT s.profile_id, p.name FROM saves s
                 JOIN profiles p ON p.id = s.profile_id
                 WHERE s.path = ?",
                &vals![path_str.clone()],
                |r| Ok((r.i64(0)?, r.string(1)?)),
            )
            .await?;

        if let Some((existing_id, existing_name)) = existing {
            if existing_id != profile_id {
                return Err(CoreError::SaveAlreadyAssigned {
                    path: path_str,
                    profile: existing_name,
                });
            }
            self.db
                .execute(
                    "UPDATE saves SET label = ? WHERE path = ?",
                    &vals![label.map(str::to_string), path_str],
                )
                .await?;
            return Ok(());
        }

        self.db
            .execute(
                "INSERT INTO saves (profile_id, path, label) VALUES (?, ?, ?)",
                &vals![profile_id, path_str, label.map(str::to_string)],
            )
            .await?;
        Ok(())
    }

    /// Remove a save assignment.
    pub async fn unassign_save(&self, path: &Path) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM saves WHERE path = ?",
                &vals![path.to_string_lossy().to_string()],
            )
            .await?;
        Ok(())
    }

    /// List all saves assigned to a profile.
    pub async fn list_saves(&self, profile_id: i64) -> Result<Vec<SaveEntry>> {
        self.db
            .fetch_all(
                "SELECT path, label, assigned_at FROM saves WHERE profile_id = ? ORDER BY assigned_at",
                &vals![profile_id],
                |r| {
                    Ok(SaveEntry {
                        path: PathBuf::from(r.string(0)?),
                        label: r.opt_string(1)?,
                        assigned_at: r.string(2)?,
                    })
                },
            )
            .await
    }

    /// Check if a save path is assigned to any profile.
    pub async fn is_save_assigned(&self, path: &Path) -> Result<bool> {
        let count = self
            .db
            .fetch_one(
                "SELECT COUNT(*) FROM saves WHERE path = ?",
                &vals![path.to_string_lossy().to_string()],
                |r| r.i64(0),
            )
            .await?;
        Ok(count > 0)
    }

    // ── Active Profile Tracking ────────────────────────────────────

    /// Set the active profile for a game, replacing any previous one.
    pub async fn set_active_profile(&self, game_id: &GameId, profile_id: i64) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO active_profiles (game_id, profile_id)
                 VALUES (?, ?)
                 ON CONFLICT(game_id) DO UPDATE SET
                    profile_id = excluded.profile_id,
                    activated_at = {NOW}",
                &vals![game_id, profile_id],
            )
            .await?;
        Ok(())
    }

    /// Get the active profile for a game, returning (`profile_id`, `profile_name`).
    pub async fn get_active_profile(&self, game_id: &GameId) -> Result<Option<(i64, String)>> {
        self.db
            .fetch_optional(
                "SELECT a.profile_id, p.name FROM active_profiles a
                 JOIN profiles p ON p.id = a.profile_id
                 WHERE a.game_id = ?",
                &vals![game_id],
                |r| Ok((r.i64(0)?, r.string(1)?)),
            )
            .await
    }

    /// Clear the active profile for a game.
    pub async fn clear_active_profile(&self, game_id: &GameId) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM active_profiles WHERE game_id = ?",
                &vals![game_id],
            )
            .await?;
        Ok(())
    }

    // ── Experiment Stack ──────────────────────────────────────────

    /// Push a profile onto the experiment stack for a game.
    pub async fn push_experiment(&self, game_id: &GameId, profile_id: i64) -> Result<()> {
        let depth = self.experiment_depth(game_id).await?;
        self.db
            .execute(
                "INSERT INTO experiment_stack (game_id, profile_id, depth)
                 VALUES (?, ?, ?)",
                &vals![game_id, profile_id, depth as i64],
            )
            .await?;
        Ok(())
    }

    /// Pop the top entry from the experiment stack, returning the `profile_id`.
    pub async fn pop_experiment(&self, game_id: &GameId) -> Result<Option<i64>> {
        let top = self
            .db
            .fetch_optional(
                "SELECT id, profile_id FROM experiment_stack
                 WHERE game_id = ? ORDER BY depth DESC LIMIT 1",
                &vals![game_id],
                |r| Ok((r.i64(0)?, r.i64(1)?)),
            )
            .await?;

        match top {
            Some((id, profile_id)) => {
                self.db
                    .execute("DELETE FROM experiment_stack WHERE id = ?", &vals![id])
                    .await?;
                Ok(Some(profile_id))
            }
            None => Ok(None),
        }
    }

    /// Get the experiment stack depth for a game.
    pub async fn experiment_depth(&self, game_id: &GameId) -> Result<usize> {
        let count = self
            .db
            .fetch_one(
                "SELECT COUNT(*) FROM experiment_stack WHERE game_id = ?",
                &vals![game_id],
                |r| r.i64(0),
            )
            .await?;
        Ok(count as usize)
    }

    /// Clear the entire experiment stack for a game.
    pub async fn clear_experiment_stack(&self, game_id: &GameId) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM experiment_stack WHERE game_id = ?",
                &vals![game_id],
            )
            .await?;
        Ok(())
    }

    // ── Stock Snapshots ───────────────────────────────────────────

    /// Insert or update a stock snapshot record.
    pub async fn upsert_snapshot(
        &self,
        game_id: &GameId,
        snapshot_path: &Path,
        tree_hash: &str,
        file_count: usize,
    ) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO stock_snapshots (game_id, snapshot_path, tree_hash, file_count)
                 VALUES (?, ?, ?, ?)
                 ON CONFLICT(game_id) DO UPDATE SET
                    snapshot_path = excluded.snapshot_path,
                    tree_hash = excluded.tree_hash,
                    file_count = excluded.file_count,
                    created_at = {NOW}",
                &vals![
                    game_id,
                    snapshot_path.to_string_lossy().to_string(),
                    tree_hash,
                    file_count as i64,
                ],
            )
            .await?;
        Ok(())
    }

    /// Get snapshot metadata for a game.
    pub async fn get_snapshot(&self, game_id: &GameId) -> Result<Option<SnapshotMeta>> {
        self.db
            .fetch_optional(
                "SELECT game_id, snapshot_path, tree_hash, file_count, created_at
                 FROM stock_snapshots WHERE game_id = ?",
                &vals![game_id],
                |r| {
                    Ok(SnapshotMeta {
                        game_id: GameId::from(r.string(0)?),
                        snapshot_path: PathBuf::from(r.string(1)?),
                        tree_hash: r.string(2)?,
                        file_count: r.i64(3)? as usize,
                        created_at: r.string(4)?,
                    })
                },
            )
            .await
    }

    // ── Hidden Files ─────────────────────────────────────────────

    /// Hide a file from a mod in a profile (prevents deployment).
    pub async fn hide_file(&self, profile_id: i64, mod_id: &ModId, rel_path: &str) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO hidden_files (profile_id, mod_id, rel_path)
                 VALUES (?, ?, ?)
                 ON CONFLICT(profile_id, mod_id, rel_path) DO NOTHING",
                &vals![profile_id, mod_id, rel_path],
            )
            .await?;
        Ok(())
    }

    /// Unhide a previously hidden file.
    pub async fn unhide_file(&self, profile_id: i64, mod_id: &ModId, rel_path: &str) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM hidden_files WHERE profile_id = ? AND mod_id = ? AND rel_path = ?",
                &vals![profile_id, mod_id, rel_path],
            )
            .await?;
        Ok(())
    }

    /// List all hidden files for a profile.
    pub async fn list_hidden_files(&self, profile_id: i64) -> Result<Vec<HiddenFile>> {
        self.db
            .fetch_all(
                "SELECT mod_id, rel_path FROM hidden_files WHERE profile_id = ?",
                &vals![profile_id],
                |r| {
                    Ok(HiddenFile {
                        mod_id: r.string(0)?,
                        rel_path: r.string(1)?,
                    })
                },
            )
            .await
    }

    /// List hidden files for a specific mod in a profile.
    pub async fn list_hidden_files_for_mod(
        &self,
        profile_id: i64,
        mod_id: &ModId,
    ) -> Result<Vec<String>> {
        self.db
            .fetch_all(
                "SELECT rel_path FROM hidden_files WHERE profile_id = ? AND mod_id = ?",
                &vals![profile_id, mod_id],
                |r| r.string(0),
            )
            .await
    }

    // ── Plugin Order ─────────────────────────────────────────────

    /// Set the plugin order for a profile (replaces any existing order).
    pub async fn set_plugin_order(&self, profile_id: i64, plugins: &[PluginEntry]) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM plugin_order WHERE profile_id = ?",
                &vals![profile_id],
            )
            .await?;
        for plugin in plugins {
            self.db
                .execute(
                    "INSERT INTO plugin_order (profile_id, plugin_name, sort_index, enabled)
                     VALUES (?, ?, ?, ?)",
                    &vals![
                        profile_id,
                        plugin.plugin_name.clone(),
                        plugin.sort_index,
                        plugin.enabled,
                    ],
                )
                .await?;
        }
        Ok(())
    }

    /// Get the plugin order for a profile.
    pub async fn get_plugin_order(&self, profile_id: i64) -> Result<Vec<PluginEntry>> {
        self.db
            .fetch_all(
                "SELECT plugin_name, sort_index, enabled FROM plugin_order
                 WHERE profile_id = ? ORDER BY sort_index",
                &vals![profile_id],
                |r| {
                    Ok(PluginEntry {
                        plugin_name: r.string(0)?,
                        sort_index: r.i64(1)?,
                        enabled: r.bool(2)?,
                    })
                },
            )
            .await
    }

    /// Toggle a plugin's enabled state.
    pub async fn toggle_plugin(
        &self,
        profile_id: i64,
        plugin_name: &str,
        enabled: bool,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE plugin_order SET enabled = ? WHERE profile_id = ? AND plugin_name = ?",
                &vals![enabled, profile_id, plugin_name],
            )
            .await?;
        Ok(())
    }

    // ── Mod Categories ───────────────────────────────────────────

    /// Create a mod category, returning its ID.
    pub async fn create_category(&self, profile_id: i64, category: &ModCategory) -> Result<i64> {
        self.db
            .fetch_one(
                "INSERT INTO mod_categories (profile_id, name, color, sort_index)
                 VALUES (?, ?, ?, ?) RETURNING id",
                &vals![
                    profile_id,
                    category.name.clone(),
                    category.color.clone(),
                    category.sort_index,
                ],
                |r| r.i64(0),
            )
            .await
    }

    /// Update a category.
    pub async fn update_category(
        &self,
        category_id: i64,
        name: &str,
        color: Option<&str>,
        sort_index: i64,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE mod_categories SET name = ?, color = ?, sort_index = ? WHERE id = ?",
                &vals![name, color.map(str::to_string), sort_index, category_id],
            )
            .await?;
        Ok(())
    }

    /// Delete a category (nullifies `category_id` on affected mods).
    pub async fn delete_category(&self, category_id: i64) -> Result<()> {
        self.db
            .execute(
                "UPDATE profile_mods SET category_id = NULL WHERE category_id = ?",
                &vals![category_id],
            )
            .await?;
        self.db
            .execute(
                "DELETE FROM mod_categories WHERE id = ?",
                &vals![category_id],
            )
            .await?;
        Ok(())
    }

    /// List categories for a profile.
    pub async fn list_categories(&self, profile_id: i64) -> Result<Vec<ModCategory>> {
        self.db
            .fetch_all(
                "SELECT id, name, color, sort_index FROM mod_categories
                 WHERE profile_id = ? ORDER BY sort_index",
                &vals![profile_id],
                |r| {
                    Ok(ModCategory {
                        id: Some(r.i64(0)?),
                        name: r.string(1)?,
                        color: r.opt_string(2)?,
                        sort_index: r.i64(3)?,
                    })
                },
            )
            .await
    }

    /// Assign a mod to a category.
    pub async fn set_mod_category(
        &self,
        profile_id: i64,
        mod_id: &ModId,
        category_id: Option<i64>,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE profile_mods SET category_id = ? WHERE profile_id = ? AND mod_id = ?",
                &vals![category_id, profile_id, mod_id],
            )
            .await?;
        Ok(())
    }

    /// Set notes for a mod.
    pub async fn set_mod_notes(
        &self,
        profile_id: i64,
        mod_id: &ModId,
        notes: Option<&str>,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE profile_mods SET notes = ? WHERE profile_id = ? AND mod_id = ?",
                &vals![notes.map(str::to_string), profile_id, mod_id],
            )
            .await?;
        Ok(())
    }

    /// Set tags for a mod (stored as a JSON array in the TEXT column).
    pub async fn set_mod_tags(
        &self,
        profile_id: i64,
        mod_id: &ModId,
        tags: &[String],
    ) -> Result<()> {
        let encoded_tags = encode_tags(tags)?;
        self.db
            .execute(
                "UPDATE profile_mods SET tags = ? WHERE profile_id = ? AND mod_id = ?",
                &vals![encoded_tags, profile_id, mod_id],
            )
            .await?;
        Ok(())
    }

    /// Set Nexus metadata for a mod.
    pub async fn set_mod_nexus_meta(
        &self,
        profile_id: i64,
        mod_id: &ModId,
        nexus_mod_id: NexusModId,
        nexus_file_id: NexusFileId,
        nexus_game_domain: &str,
        installed_timestamp: i64,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE profile_mods SET nexus_mod_id = ?, nexus_file_id = ?,
                        nexus_game_domain = ?, installed_timestamp = ?
                 WHERE profile_id = ? AND mod_id = ?",
                &vals![
                    nexus_mod_id.to_i64()?,
                    nexus_file_id.to_i64()?,
                    nexus_game_domain,
                    installed_timestamp,
                    profile_id,
                    mod_id,
                ],
            )
            .await?;
        Ok(())
    }

    // ── Installer tracking (V8) ───────────────────────────────────

    /// Persist an installer's decision and file manifest for a single mod row,
    /// atomically (in one transaction).
    pub async fn record_install(
        &self,
        profile_id: i64,
        mod_id: &ModId,
        plan: &InstallPlan,
        status: InstallStatus,
    ) -> Result<()> {
        let method_toml = encode_install_method(&plan.method)?;
        let mut tx = self.db.begin().await?;

        tx.execute(
            "UPDATE profile_mods
                SET install_method = ?, source_archive_hash = ?, install_status = ?
              WHERE profile_id = ? AND mod_id = ?",
            &vals![
                method_toml,
                plan.source_archive_hash.clone(),
                status.as_str(),
                profile_id,
                mod_id,
            ],
        )
        .await?;

        tx.execute(
            "DELETE FROM installed_mod_files WHERE profile_id = ? AND mod_id = ?",
            &vals![profile_id, mod_id],
        )
        .await?;

        for file in &plan.staged_files {
            tx.execute(
                "INSERT INTO installed_mod_files
                    (profile_id, mod_id, rel_path, origin_rel_path, size, merge_group)
                 VALUES (?, ?, ?, ?, ?, ?)",
                &vals![
                    profile_id,
                    mod_id,
                    file.rel_path.clone(),
                    file.origin_rel_path.clone(),
                    file.size as i64,
                    file.merge_group.clone(),
                ],
            )
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Return every file staged by `mod_id` in `profile_id`, sorted by relative
    /// path for deterministic uninstall order.
    pub async fn installed_files_for_mod(
        &self,
        profile_id: i64,
        mod_id: &ModId,
    ) -> Result<Vec<StagedFile>> {
        self.db
            .fetch_all(
                "SELECT rel_path, origin_rel_path, size, merge_group
                   FROM installed_mod_files
                  WHERE profile_id = ? AND mod_id = ?
               ORDER BY rel_path",
                &vals![profile_id, mod_id],
                |r| {
                    Ok(StagedFile {
                        rel_path: r.string(0)?,
                        origin_rel_path: r.string(1)?,
                        size: r.i64(2)?.max(0) as u64,
                        merge_group: r.opt_string(3)?,
                    })
                },
            )
            .await
    }

    /// Remove a mod from `profile_mods` and return its staged files so the
    /// caller can unlink them. Runs in one transaction.
    pub async fn remove_installed_mod(
        &self,
        profile_id: i64,
        mod_id: &ModId,
    ) -> Result<Vec<StagedFile>> {
        let files = self.installed_files_for_mod(profile_id, mod_id).await?;
        let mut tx = self.db.begin().await?;
        tx.execute(
            "DELETE FROM installed_mod_files WHERE profile_id = ? AND mod_id = ?",
            &vals![profile_id, mod_id],
        )
        .await?;
        tx.execute(
            "DELETE FROM profile_mods WHERE profile_id = ? AND mod_id = ?",
            &vals![profile_id, mod_id],
        )
        .await?;
        tx.commit().await?;
        Ok(files)
    }

    /// Return every file tagged with `merge_group`, across all mods in `profile_id`.
    pub async fn files_in_merge_group(
        &self,
        profile_id: i64,
        merge_group: &str,
    ) -> Result<Vec<(String, StagedFile)>> {
        self.db
            .fetch_all(
                "SELECT mod_id, rel_path, origin_rel_path, size, merge_group
                   FROM installed_mod_files
                  WHERE profile_id = ? AND merge_group = ?
               ORDER BY mod_id, rel_path",
                &vals![profile_id, merge_group],
                |r| {
                    Ok((
                        r.string(0)?,
                        StagedFile {
                            rel_path: r.string(1)?,
                            origin_rel_path: r.string(2)?,
                            size: r.i64(3)?.max(0) as u64,
                            merge_group: r.opt_string(4)?,
                        },
                    ))
                },
            )
            .await
    }

    // ── TOML Import ───────────────────────────────────────────────

    /// Import existing TOML profile files into the database.
    /// Returns the number of profiles imported.
    pub async fn import_toml_profiles(&self, profiles_dir: &Path) -> Result<usize> {
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

            if profile.load_order_lock.is_none() {
                profile.load_order_lock = Some(LoadOrderLock::now(LockReason::TomlImport {
                    source_path: toml_path.display().to_string(),
                }));
            }

            let exists = self
                .db
                .fetch_one(
                    "SELECT COUNT(*) FROM profiles WHERE name = ? AND game_id = ?",
                    &vals![profile.name.clone(), &profile.game_id],
                    |r| r.i64(0),
                )
                .await?
                > 0;

            if exists {
                tracing::debug!(name = %profile.name, game = %profile.game_id, "profile already in DB, skipping");
                continue;
            }

            self.create_profile(&profile).await?;
            tracing::info!(name = %profile.name, game = %profile.game_id, "imported TOML profile");
            count += 1;
        }

        Ok(count)
    }

    // ── Internal helpers ──────────────────────────────────────────

    async fn insert_mods(&self, profile_id: i64, mods: &[EnabledMod]) -> Result<()> {
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

            self.db
                .execute(
                    "INSERT INTO profile_mods (profile_id, mod_id, display_name, enabled, version, fomod_config, sort_index,
                            nexus_mod_id, nexus_file_id, nexus_game_domain, installed_timestamp,
                            category_id, notes, tags, lock_reason,
                            install_method, source_archive_hash, install_status)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    &vals![
                        profile_id,
                        m.mod_id.clone(),
                        m.display_name.clone(),
                        m.enabled,
                        m.version.clone(),
                        m.fomod_config.clone(),
                        idx as i64,
                        nexus_mod_id,
                        nexus_file_id,
                        m.nexus_game_domain.clone(),
                        m.installed_timestamp,
                        m.category_id,
                        m.notes.clone(),
                        tags,
                        lock_reason,
                        install_method,
                        m.source_archive_hash.clone(),
                        install_status,
                    ],
                )
                .await?;
        }
        Ok(())
    }

    async fn insert_rules(&self, profile_id: i64, rules: &[LoadOrderRule]) -> Result<()> {
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
            self.db
                .execute(
                    "INSERT INTO load_order_rules (profile_id, rule_type, mod_a, mod_b)
                     VALUES (?, ?, ?, ?)",
                    &vals![profile_id, rule_type, mod_a, mod_b],
                )
                .await?;
        }
        Ok(())
    }

    async fn load_mods(&self, profile_id: i64) -> Result<Vec<EnabledMod>> {
        self.db
            .fetch_all(
                "SELECT mod_id, display_name, enabled, version, fomod_config,
                        nexus_mod_id, nexus_file_id, nexus_game_domain, installed_timestamp,
                        category_id, notes, tags, lock_reason,
                        install_method, source_archive_hash, install_status
                 FROM profile_mods WHERE profile_id = ? ORDER BY sort_index",
                &vals![profile_id],
                map_enabled_mod,
            )
            .await
    }

    async fn load_rules(&self, profile_id: i64) -> Result<SmallVec<[LoadOrderRule; 4]>> {
        let raw = self
            .db
            .fetch_all(
                "SELECT rule_type, mod_a, mod_b FROM load_order_rules WHERE profile_id = ?",
                &vals![profile_id],
                |r| Ok((r.string(0)?, r.string(1)?, r.string(2)?)),
            )
            .await?;

        let mut result = SmallVec::with_capacity(raw.len());
        for (rule_type, mod_a, mod_b) in raw {
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

    async fn assemble_profile(
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
        let mods = self.load_mods(id).await?;
        let load_order_rules = self.load_rules(id).await?;
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

    // ── Game Tool CRUD ────────────────────────────────────────────

    /// Save (insert or update) a tool configuration for a game.
    pub async fn save_tool_config(
        &self,
        game_id: &GameId,
        tool_id: &str,
        enabled: bool,
        settings_json: &str,
    ) -> Result<()> {
        self.save_tool_config_with_reason(game_id, tool_id, enabled, settings_json, "update")
            .await
    }

    /// Save a tool configuration and append a history node.
    pub async fn save_tool_config_with_reason(
        &self,
        game_id: &GameId,
        tool_id: &str,
        enabled: bool,
        settings_json: &str,
        reason: &str,
    ) -> Result<()> {
        let parent_node_id = self.current_tool_setting_node_id(game_id, tool_id).await?;
        let node_id = new_tool_setting_node_id(game_id, tool_id);
        let reason = if reason.trim().is_empty() {
            "update"
        } else {
            reason.trim()
        };

        self.db
            .execute(
                "INSERT INTO tool_setting_nodes (node_id, game_id, tool_id, enabled, settings, reason)
                 VALUES (?, ?, ?, ?, ?, ?)",
                &vals![node_id.clone(), game_id, tool_id, enabled, settings_json, reason],
            )
            .await?;
        if let Some(parent_node_id) = parent_node_id {
            self.db
                .execute(
                    "INSERT INTO tool_setting_edges (parent_node_id, child_node_id)
                     VALUES (?, ?)
                     ON CONFLICT(parent_node_id, child_node_id) DO NOTHING",
                    &vals![parent_node_id, node_id.clone()],
                )
                .await?;
        }
        self.db
            .execute(
                "INSERT INTO game_tools (game_id, tool_id, enabled, settings, updated_at, current_node_id)
                 VALUES (?, ?, ?, ?, {NOW}, ?)
                 ON CONFLICT(game_id, tool_id) DO UPDATE SET
                     enabled = excluded.enabled,
                     settings = excluded.settings,
                     updated_at = excluded.updated_at,
                     current_node_id = excluded.current_node_id",
                &vals![game_id, tool_id, enabled, settings_json, node_id],
            )
            .await?;
        Ok(())
    }

    /// Load recent settings history nodes for a tool.
    pub async fn list_tool_setting_history(
        &self,
        game_id: &GameId,
        tool_id: &str,
        limit: usize,
    ) -> Result<Vec<ToolSettingHistoryNode>> {
        let current_node_id = self.current_tool_setting_node_id(game_id, tool_id).await?;
        let current = current_node_id.clone();
        self.db
            .fetch_all(
                "SELECT node_id, game_id, tool_id, enabled, settings, reason, created_at
                 FROM tool_setting_nodes
                 WHERE game_id = ? AND tool_id = ?
                 ORDER BY id DESC
                 LIMIT ?",
                &vals![game_id, tool_id, limit as i64],
                move |r| {
                    let node_id = r.string(0)?;
                    Ok(ToolSettingHistoryNode {
                        is_current: current.as_deref() == Some(node_id.as_str()),
                        node_id,
                        game_id: r.string(1)?,
                        tool_id: r.string(2)?,
                        enabled: r.bool(3)?,
                        settings_json: r.string(4)?,
                        reason: r.string(5)?,
                        created_at: r.string(6)?,
                    })
                },
            )
            .await
    }

    /// Load DAG edges for a tool's recorded settings history.
    pub async fn list_tool_setting_edges(
        &self,
        game_id: &GameId,
        tool_id: &str,
    ) -> Result<Vec<ToolSettingHistoryEdge>> {
        self.db
            .fetch_all(
                "SELECT e.parent_node_id, e.child_node_id
                 FROM tool_setting_edges e
                 JOIN tool_setting_nodes child ON child.node_id = e.child_node_id
                 WHERE child.game_id = ? AND child.tool_id = ?
                 ORDER BY e.id",
                &vals![game_id, tool_id],
                |r| {
                    Ok(ToolSettingHistoryEdge {
                        parent_node_id: r.string(0)?,
                        child_node_id: r.string(1)?,
                    })
                },
            )
            .await
    }

    /// Restore a settings node by appending a new child node with copied state.
    pub async fn restore_tool_setting_node(
        &self,
        game_id: &GameId,
        tool_id: &str,
        node_id: &str,
    ) -> Result<()> {
        let (enabled, settings_json) = self
            .db
            .fetch_one(
                "SELECT enabled, settings FROM tool_setting_nodes
                 WHERE game_id = ? AND tool_id = ? AND node_id = ?",
                &vals![game_id, tool_id, node_id],
                |r| Ok((r.bool(0)?, r.string(1)?)),
            )
            .await?;
        let reason = format!("restore:{node_id}");
        self.save_tool_config_with_reason(game_id, tool_id, enabled, &settings_json, &reason)
            .await
    }

    async fn current_tool_setting_node_id(
        &self,
        game_id: &GameId,
        tool_id: &str,
    ) -> Result<Option<String>> {
        Ok(self
            .db
            .fetch_optional(
                "SELECT current_node_id FROM game_tools WHERE game_id = ? AND tool_id = ?",
                &vals![game_id, tool_id],
                |r| r.opt_string(0),
            )
            .await?
            .flatten())
    }

    /// Load all tool configurations for a game.
    pub async fn load_tool_configs(&self, game_id: &GameId) -> Result<Vec<ToolConfigRow>> {
        self.db
            .fetch_all(
                "SELECT tool_id, enabled, settings FROM game_tools WHERE game_id = ?",
                &vals![game_id],
                |r| {
                    Ok(ToolConfigRow {
                        tool_id: r.string(0)?,
                        enabled: r.bool(1)?,
                        settings_json: r.string(2)?,
                    })
                },
            )
            .await
    }

    /// Load a single tool configuration for a game.
    pub async fn load_tool_config(
        &self,
        game_id: &GameId,
        tool_id: &str,
    ) -> Result<Option<ToolConfigRow>> {
        self.db
            .fetch_optional(
                "SELECT tool_id, enabled, settings FROM game_tools
                 WHERE game_id = ? AND tool_id = ?",
                &vals![game_id, tool_id],
                |r| {
                    Ok(ToolConfigRow {
                        tool_id: r.string(0)?,
                        enabled: r.bool(1)?,
                        settings_json: r.string(2)?,
                    })
                },
            )
            .await
    }

    /// Record files applied by a tool to a game directory.
    pub async fn save_applied_files(
        &self,
        game_id: &GameId,
        tool_id: &str,
        rel_paths: &[String],
    ) -> Result<()> {
        for path in rel_paths {
            self.db
                .execute(
                    "INSERT INTO tool_applied_files (game_id, tool_id, rel_path)
                     VALUES (?, ?, ?)
                     ON CONFLICT(game_id, tool_id, rel_path) DO NOTHING",
                    &vals![game_id, tool_id, path.clone()],
                )
                .await?;
        }
        Ok(())
    }

    /// Load files previously applied by a tool.
    pub async fn load_applied_files(&self, game_id: &GameId, tool_id: &str) -> Result<Vec<String>> {
        self.db
            .fetch_all(
                "SELECT rel_path FROM tool_applied_files
                 WHERE game_id = ? AND tool_id = ?",
                &vals![game_id, tool_id],
                |r| r.string(0),
            )
            .await
    }

    /// Clear all applied file records for a tool on a game.
    pub async fn clear_applied_files(&self, game_id: &GameId, tool_id: &str) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM tool_applied_files WHERE game_id = ? AND tool_id = ?",
                &vals![game_id, tool_id],
            )
            .await?;
        Ok(())
    }

    // ── Executable Config CRUD ───────────────────────────────────

    /// Save or update a named executable for a game.
    pub async fn save_executable_config(&self, executable: &ExecutableConfigRow) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO executable_configs (
                    game_id, name, executable_path, arguments, working_dir,
                    environment, wine_dll_overrides, output_mod, enabled, updated_at
                 )
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, {NOW})
                 ON CONFLICT(game_id, name) DO UPDATE SET
                    executable_path = excluded.executable_path,
                    arguments = excluded.arguments,
                    working_dir = excluded.working_dir,
                    environment = excluded.environment,
                    wine_dll_overrides = excluded.wine_dll_overrides,
                    output_mod = excluded.output_mod,
                    enabled = excluded.enabled,
                    updated_at = excluded.updated_at",
                &vals![
                    executable.game_id.clone(),
                    executable.name.clone(),
                    executable.executable_path.to_string_lossy().to_string(),
                    executable.arguments_json.clone(),
                    executable
                        .working_dir
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                    executable.environment_json.clone(),
                    executable.wine_dll_overrides.clone(),
                    executable.output_mod.clone(),
                    executable.enabled,
                ],
            )
            .await?;
        Ok(())
    }

    /// Load every executable configured for a game, ordered by display name.
    pub async fn load_executable_configs(
        &self,
        game_id: &GameId,
    ) -> Result<Vec<ExecutableConfigRow>> {
        self.db
            .fetch_all(
                "SELECT game_id, name, executable_path, arguments, working_dir,
                        environment, wine_dll_overrides, output_mod, enabled
                 FROM executable_configs
                 WHERE game_id = ?
                 ORDER BY lower(name)",
                &vals![game_id],
                executable_from_row,
            )
            .await
    }

    /// Load a single named executable for a game.
    pub async fn load_executable_config(
        &self,
        game_id: &GameId,
        name: &str,
    ) -> Result<Option<ExecutableConfigRow>> {
        self.db
            .fetch_optional(
                "SELECT game_id, name, executable_path, arguments, working_dir,
                        environment, wine_dll_overrides, output_mod, enabled
                 FROM executable_configs
                 WHERE game_id = ? AND name = ?",
                &vals![game_id, name],
                executable_from_row,
            )
            .await
    }

    /// Delete a named executable for a game. Returns whether a row was removed.
    pub async fn delete_executable_config(&self, game_id: &GameId, name: &str) -> Result<bool> {
        let affected = self
            .db
            .execute(
                "DELETE FROM executable_configs WHERE game_id = ? AND name = ?",
                &vals![game_id, name],
            )
            .await?;
        Ok(affected > 0)
    }
}

fn map_enabled_mod(r: &dyn DbRow) -> Result<EnabledMod> {
    let nexus_mod_id = r.opt_i64(5)?.map(NexusModId::try_from).transpose()?;
    let nexus_file_id = r.opt_i64(6)?.map(NexusFileId::try_from).transpose()?;
    Ok(EnabledMod {
        mod_id: r.string(0)?,
        display_name: r.opt_string(1)?,
        enabled: r.bool(2)?,
        version: r.opt_string(3)?,
        fomod_config: r.opt_string(4)?,
        nexus_mod_id,
        nexus_file_id,
        nexus_game_domain: r.opt_string(7)?,
        installed_timestamp: r.opt_i64(8)?,
        category_id: r.opt_i64(9)?,
        notes: r.opt_string(10)?,
        tags: decode_tags(r.opt_string(11)?.as_deref())?,
        lock: decode_lock_reason(r.opt_string(12)?.as_deref())?,
        install_method: decode_install_method(r.opt_string(13)?.as_deref())?,
        source_archive_hash: r.opt_string(14)?,
        install_status: decode_install_status(r.opt_string(15)?.as_deref())?,
    })
}

fn executable_from_row(r: &dyn DbRow) -> Result<ExecutableConfigRow> {
    Ok(ExecutableConfigRow {
        game_id: r.string(0)?,
        name: r.string(1)?,
        executable_path: PathBuf::from(r.string(2)?),
        arguments_json: r.string(3)?,
        working_dir: r.opt_string(4)?.map(PathBuf::from),
        environment_json: r.string(5)?,
        wine_dll_overrides: r.opt_string(6)?,
        output_mod: r.string(7)?,
        enabled: r.bool(8)?,
    })
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

#[cfg(test)]
mod tests;
