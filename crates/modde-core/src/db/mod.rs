//! Persistent storage for modde, backed by `SQLite` (default) or `PostgreSQL`.
//!
//! The public [`ModdeDb`] API is identical across both backends and async
//! throughout. Each method is written once against the internal `Db` executor
//! using portable SQL (`?` placeholders, `RETURNING id`, `ON CONFLICT … DO
//! UPDATE … EXCLUDED`, `lower(name)`, `bool` columns, and a `{NOW}` token);
//! the executor rewrites placeholders/`now()` per dialect. The only genuinely
//! per-backend code is schema creation and migration.

mod backend;
mod migrate;

use std::path::{Path, PathBuf};
use std::str::FromStr;

use smallvec::SmallVec;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

use crate::bisect::{BisectResult, BisectSaveSafety, BisectSession, BisectStatus, BisectStep};
use crate::crash::CrashCorrelationReport;
use crate::doctor::{DoctorProfileModSnapshot, ProfileSnapshotRow};
use crate::error::{CoreError, Result};
use crate::installer::{InstallMethod, InstallPlan, InstallStatus, StagedFile};
use crate::nexus_id::{NexusFileId, NexusModId};
use crate::patcher::{
    PatcherStageKind, PatcherStageOutputRow, PatcherStageRow, PatcherStageSettings,
};
use crate::performance::{
    PerformanceModSnapshot, PerformanceSample, PerformanceSummary, mod_set_hash,
};
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

/// New performance capture run to persist before launch.
#[derive(Debug, Clone)]
pub struct NewPerformanceRun {
    pub run_id: String,
    pub game_id: GameId,
    pub profile_id: Option<i64>,
    pub profile_name: String,
    pub mod_snapshot: Vec<PerformanceModSnapshot>,
    pub experiment_depth: usize,
    pub label: Option<String>,
}

/// Stored performance capture run with parsed summary metrics when available.
#[derive(Debug, Clone, PartialEq)]
pub struct PerformanceRunRow {
    pub run_id: String,
    pub game_id: GameId,
    pub profile_id: Option<i64>,
    pub profile_name: String,
    pub mod_set_hash: String,
    pub mod_snapshot_json: String,
    pub experiment_depth: usize,
    pub label: Option<String>,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub mangohud_csv_path: Option<PathBuf>,
    pub exit_status: Option<i64>,
    pub summary: PerformanceSummary,
}

pub use crate::bisect::{NewBisectSession, NewBisectStep};

/// A configured patcher pipeline stage for one profile.
pub type PatcherConfigRow = PatcherStageRow;

/// SQLite/PostgreSQL-backed persistent storage for modde.
#[derive(Debug, Clone)]
pub struct ModdeDb {
    db: Db,
}

/// Build resolved `PostgreSQL` connection options from settings plus env.
///
/// The injected `env` accessor keeps this pure and deterministic for callers
/// such as `config show` and unit tests. `MODDE_DATABASE_URL` wins over all
/// discrete fields; otherwise each discrete env var wins over its setting.
#[cfg(feature = "postgres")]
pub fn build_pg_options(
    settings: &DatabaseSettings,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<sqlx::postgres::PgConnectOptions> {
    use sqlx::postgres::PgConnectOptions;

    let url = env("MODDE_DATABASE_URL").or_else(|| settings.url.clone());
    if let Some(url) = url {
        return Ok(PgConnectOptions::from_str(&url)?);
    }

    let mut opts = PgConnectOptions::new();

    if let Some(host) = env("MODDE_DATABASE_HOST").or_else(|| settings.host.clone()) {
        opts = opts.host(&host);
    }

    let port = match env("MODDE_DATABASE_PORT") {
        Some(raw) => Some(raw.parse::<u16>().map_err(|e| {
            CoreError::Other(
                format!("invalid MODDE_DATABASE_PORT value '{raw}': expected 0-65535 ({e})").into(),
            )
        })?),
        None => settings.port,
    };
    if let Some(port) = port {
        opts = opts.port(port);
    }

    // Home Manager exports MODDE_DATABASE_NAME; the settings field remains
    // `dbname` to match PostgreSQL terminology and existing TOML shape.
    let dbname = env("MODDE_DATABASE_NAME")
        .or_else(|| settings.dbname.clone())
        .ok_or_else(|| {
            CoreError::Other("postgres backend selected but no database name configured".into())
        })?;
    opts = opts.database(&dbname);

    if let Some(user) = env("MODDE_DATABASE_USER").or_else(|| settings.user.clone()) {
        opts = opts.username(&user);
    }

    Ok(opts)
}

#[cfg(feature = "postgres")]
pub fn describe_pg_options(opts: &sqlx::postgres::PgConnectOptions) -> String {
    let endpoint = opts
        .get_socket()
        .map(|socket| format!("socket={}", socket.display()))
        .unwrap_or_else(|| format!("host={}", opts.get_host()));
    let database = opts.get_database().unwrap_or("<unset>");

    format!(
        "{endpoint}, port={}, dbname={database}, user={}",
        opts.get_port(),
        opts.get_username()
    )
}

#[cfg(feature = "postgres")]
fn read_pg_password_file(path: &Path) -> Result<String> {
    let pw = std::fs::read_to_string(path).map_err(|e| {
        CoreError::Other(
            format!(
                "failed to read MODDE_DB_PASSWORD_FILE/database.password_file {}: {e}",
                path.display()
            )
            .into(),
        )
    })?;
    Ok(pw.trim().to_string())
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

    /// Run the lightest backend-agnostic query available to prove the open
    /// connection can execute SQL.
    pub async fn ping(&self) -> Result<()> {
        self.db
            .fetch_one("SELECT 1", &vals![], |r| r.i64(0))
            .await?;
        Ok(())
    }

    #[cfg(feature = "postgres")]
    async fn open_postgres(settings: &DatabaseSettings) -> Result<Self> {
        use sqlx::postgres::PgPoolOptions;

        let mut opts = build_pg_options(settings, &|key| std::env::var(key).ok())?;

        let pw_path = std::env::var("MODDE_DB_PASSWORD_FILE")
            .ok()
            .map(PathBuf::from)
            .or_else(|| settings.password_file.clone());
        if let Some(path) = pw_path {
            let pw = read_pg_password_file(&path)?;
            opts = opts.password(&pw);
        }

        let summary = describe_pg_options(&opts);
        let pool = PgPoolOptions::new().connect_with(opts).await.map_err(|e| {
            CoreError::Other(format!("failed to connect to postgres ({summary}): {e}").into())
        })?;
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
        self.record_profile_state_snapshot(id, profile).await?;

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
        self.record_profile_state_snapshot(profile_id, profile)
            .await?;

        Ok(())
    }

    /// Persist the profile mod state used by `modde doctor` for recent diffs.
    pub async fn record_profile_state_snapshot(
        &self,
        profile_id: i64,
        profile: &Profile,
    ) -> Result<()> {
        let snapshot = DoctorProfileModSnapshot::from_profile(profile);
        let snapshot_json = serde_json::to_string(&snapshot).map_err(|e| {
            CoreError::Other(format!("failed to encode profile state snapshot: {e}").into())
        })?;
        self.db
            .execute(
                "INSERT INTO profile_state_snapshots
                    (profile_id, game_id, profile_name, snapshot_json)
                 VALUES (?, ?, ?, ?)",
                &vals![
                    profile_id,
                    &profile.game_id,
                    profile.name.clone(),
                    snapshot_json,
                ],
            )
            .await?;
        Ok(())
    }

    /// Return recent profile state snapshots, newest first.
    pub async fn recent_profile_state_snapshots(
        &self,
        profile_id: i64,
        limit: usize,
    ) -> Result<Vec<ProfileSnapshotRow>> {
        self.db
            .fetch_all(
                "SELECT id, snapshot_json, created_at
                   FROM profile_state_snapshots
                  WHERE profile_id = ?
               ORDER BY created_at DESC, id DESC
                  LIMIT ?",
                &vals![profile_id, limit as i64],
                |r| {
                    let snapshot_json = r.string(1)?;
                    let snapshot =
                        serde_json::from_str::<Vec<DoctorProfileModSnapshot>>(&snapshot_json)
                            .map_err(|e| {
                                CoreError::Other(
                                    format!("failed to decode profile state snapshot: {e}").into(),
                                )
                            })?;
                    Ok(ProfileSnapshotRow {
                        id: r.i64(0)?,
                        snapshot,
                        created_at: r.string(2)?,
                    })
                },
            )
            .await
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

    /// Copy profile-adjacent state that is not represented inside [`Profile`].
    ///
    /// Used by bisect candidate profiles so hidden-file exclusions, native
    /// plugin order, and installer file manifests remain consistent with the
    /// source profile.
    pub async fn copy_profile_auxiliary_state(
        &self,
        source_profile_id: i64,
        target_profile_id: i64,
    ) -> Result<()> {
        let mut tx = self.db.begin().await?;
        tx.execute(
            "INSERT INTO hidden_files (profile_id, mod_id, rel_path)
             SELECT ?, mod_id, rel_path FROM hidden_files WHERE profile_id = ?",
            &vals![target_profile_id, source_profile_id],
        )
        .await?;
        tx.execute(
            "INSERT INTO plugin_order (profile_id, plugin_name, sort_index, enabled)
             SELECT ?, plugin_name, sort_index, enabled FROM plugin_order WHERE profile_id = ?",
            &vals![target_profile_id, source_profile_id],
        )
        .await?;
        tx.execute(
            "INSERT INTO installed_mod_files
                (profile_id, mod_id, rel_path, origin_rel_path, size, merge_group)
             SELECT ?, mod_id, rel_path, origin_rel_path, size, merge_group
               FROM installed_mod_files WHERE profile_id = ?",
            &vals![target_profile_id, source_profile_id],
        )
        .await?;
        tx.commit().await?;
        Ok(())
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

    /// Return every installer-tracked file in a profile, paired with its owning
    /// mod id. Crash-log correlation uses this to map mentioned DLLs/assets
    /// back to the managed install database.
    pub async fn installed_files_for_profile(
        &self,
        profile_id: i64,
    ) -> Result<Vec<(String, StagedFile)>> {
        self.db
            .fetch_all(
                "SELECT mod_id, rel_path, origin_rel_path, size, merge_group
                   FROM installed_mod_files
                  WHERE profile_id = ?
               ORDER BY mod_id, rel_path",
                &vals![profile_id],
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

    /// Persist a raw local crash log and its structured correlation report.
    ///
    /// This stores logs locally only; callers must not upload or transmit the
    /// raw crash text.
    pub async fn record_crash_log(
        &self,
        profile_id: Option<i64>,
        report: &CrashCorrelationReport,
        raw_log: &str,
    ) -> Result<()> {
        let signature_json = serde_json::to_string(&report.signature).map_err(|e| {
            CoreError::Other(format!("failed to encode crash signature: {e}").into())
        })?;
        let report_json = serde_json::to_string(report)
            .map_err(|e| CoreError::Other(format!("failed to encode crash report: {e}").into()))?;
        self.db
            .execute(
                "INSERT INTO crash_logs
                    (game_id, profile_id, profile_name, source_path, logger_format,
                     raw_sha256, raw_log, signature_json, report_json)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                &vals![
                    report.game_id.clone(),
                    profile_id,
                    report.profile_name.clone(),
                    report.source_path.display().to_string(),
                    report.format.as_str(),
                    report.raw_sha256.clone(),
                    raw_log,
                    signature_json,
                    report_json,
                ],
            )
            .await?;
        Ok(())
    }

    // ── Bisect sessions ──────────────────────────────────────────

    pub async fn create_bisect_session(&self, session: &NewBisectSession) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO bisect_sessions
                    (session_id, game_id, source_profile_id, source_profile_name, oracle_json,
                     status, suspect_mod_ids_json, known_good_mod_ids_json,
                     known_bad_mod_ids_json, save_safety, keep_profiles)
                 VALUES (?, ?, ?, ?, ?, ?, ?, '[]', '[]', ?, ?)",
                &vals![
                    session.session_id.clone(),
                    &session.game_id,
                    session.source_profile_id,
                    session.source_profile_name.clone(),
                    encode_json(&session.oracle, "bisect oracle")?,
                    BisectStatus::Active.as_str(),
                    encode_json(&session.suspect_mod_ids, "bisect suspect mod ids")?,
                    match session.save_safety {
                        BisectSaveSafety::Refuse => "refuse",
                        BisectSaveSafety::Force => "force",
                    },
                    session.keep_profiles,
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn load_bisect_session(&self, session_id: &str) -> Result<BisectSession> {
        self.db
            .fetch_optional(
                "SELECT session_id, game_id, source_profile_id, source_profile_name,
                        oracle_json, status, suspect_mod_ids_json,
                        known_good_mod_ids_json, known_bad_mod_ids_json,
                        current_step_id, current_candidate_profile, save_safety,
                        keep_profiles, created_at, updated_at
                   FROM bisect_sessions WHERE session_id = ?",
                &vals![session_id],
                bisect_session_from_row,
            )
            .await?
            .ok_or_else(|| {
                CoreError::Other(format!("bisect session not found: {session_id}").into())
            })
    }

    pub async fn update_bisect_session_state(
        &self,
        session_id: &str,
        status: BisectStatus,
        suspect_mod_ids: &[String],
        known_good_mod_ids: &[String],
        known_bad_mod_ids: &[String],
        current_step_id: Option<i64>,
        current_candidate_profile: Option<&str>,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE bisect_sessions SET
                    status = ?,
                    suspect_mod_ids_json = ?,
                    known_good_mod_ids_json = ?,
                    known_bad_mod_ids_json = ?,
                    current_step_id = ?,
                    current_candidate_profile = ?,
                    updated_at = {NOW}
                 WHERE session_id = ?",
                &vals![
                    status.as_str(),
                    encode_json(suspect_mod_ids, "bisect suspect mod ids")?,
                    encode_json(known_good_mod_ids, "bisect known good mod ids")?,
                    encode_json(known_bad_mod_ids, "bisect known bad mod ids")?,
                    current_step_id,
                    current_candidate_profile.map(str::to_string),
                    session_id,
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn create_bisect_step(&self, step: &NewBisectStep) -> Result<i64> {
        let id = self
            .db
            .fetch_one(
                "INSERT INTO bisect_steps
                    (session_id, step_index, candidate_profile, candidate_mod_ids_json,
                     enabled_mod_ids_json, disabled_mod_ids_json)
                 VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
                &vals![
                    step.session_id.clone(),
                    step.step_index as i64,
                    step.candidate_profile.clone(),
                    encode_json(&step.candidate_mod_ids, "bisect candidate mod ids")?,
                    encode_json(&step.enabled_mod_ids, "bisect enabled mod ids")?,
                    encode_json(&step.disabled_mod_ids, "bisect disabled mod ids")?,
                ],
                |r| r.i64(0),
            )
            .await?;
        Ok(id)
    }

    pub async fn load_bisect_step(&self, id: i64) -> Result<BisectStep> {
        self.db
            .fetch_optional(
                "SELECT id, session_id, step_index, candidate_profile,
                        candidate_mod_ids_json, enabled_mod_ids_json, disabled_mod_ids_json,
                        result, observed_signal, notes, launched_at
                   FROM bisect_steps WHERE id = ?",
                &vals![id],
                bisect_step_from_row,
            )
            .await?
            .ok_or_else(|| CoreError::Other(format!("bisect step not found: {id}").into()))
    }

    pub async fn list_bisect_steps(&self, session_id: &str) -> Result<Vec<BisectStep>> {
        self.db
            .fetch_all(
                "SELECT id, session_id, step_index, candidate_profile,
                        candidate_mod_ids_json, enabled_mod_ids_json, disabled_mod_ids_json,
                        result, observed_signal, notes, launched_at
                   FROM bisect_steps WHERE session_id = ? ORDER BY step_index",
                &vals![session_id],
                bisect_step_from_row,
            )
            .await
    }

    pub async fn complete_bisect_step(
        &self,
        id: i64,
        result: BisectResult,
        observed_signal: Option<&str>,
        notes: Option<&str>,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE bisect_steps SET result = ?, observed_signal = ?, notes = ? WHERE id = ?",
                &vals![
                    result.as_str(),
                    observed_signal.map(str::to_string),
                    notes.map(str::to_string),
                    id,
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn bisect_candidate_profiles(&self, session_id: &str) -> Result<Vec<String>> {
        self.db
            .fetch_all(
                "SELECT candidate_profile FROM bisect_steps WHERE session_id = ? ORDER BY step_index",
                &vals![session_id],
                |r| r.string(0),
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

    /// Load every file recorded as applied by any managed tool for this game.
    pub async fn load_all_applied_files(&self, game_id: &GameId) -> Result<Vec<String>> {
        self.db
            .fetch_all(
                "SELECT rel_path FROM tool_applied_files
                 WHERE game_id = ?
                 ORDER BY tool_id, rel_path",
                &vals![game_id],
                |r| r.string(0),
            )
            .await
    }

    /// Load every file recorded as applied by any managed tool for this game,
    /// preserving the owning tool id for provenance exports.
    pub async fn load_all_applied_file_rows(
        &self,
        game_id: &GameId,
    ) -> Result<Vec<ToolAppliedFileRow>> {
        self.db
            .fetch_all(
                "SELECT tool_id, rel_path FROM tool_applied_files
                 WHERE game_id = ?
                 ORDER BY tool_id, rel_path",
                &vals![game_id],
                |r| {
                    Ok(ToolAppliedFileRow {
                        tool_id: r.string(0)?,
                        rel_path: r.string(1)?,
                    })
                },
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

    // ── Patcher Pipeline Config CRUD ─────────────────────────────

    /// Save or update a profile-scoped patcher stage.
    pub async fn save_patcher_stage(&self, stage: &PatcherStageRow) -> Result<()> {
        if stage.name.trim().is_empty() {
            return Err(CoreError::Validation(
                "patcher stage name cannot be empty".into(),
            ));
        }
        if stage.output_mod.trim().is_empty() {
            return Err(CoreError::Validation(
                "patcher output mod cannot be empty".into(),
            ));
        }
        if matches!(stage.settings, PatcherStageSettings::RustNative) {
            return Err(CoreError::Validation(
                "rust-native patcher stages are reserved for a future release".into(),
            ));
        }
        if self
            .db
            .fetch_optional(
                "SELECT name FROM profile_patcher_stages
                 WHERE profile_id = ? AND output_mod = ? AND name <> ?",
                &vals![
                    stage.profile_id,
                    stage.output_mod.clone(),
                    stage.name.clone()
                ],
                |r| r.string(0),
            )
            .await?
            .is_some()
        {
            return Err(CoreError::Validation(
                format!(
                    "patcher output mod '{}' is already used by another stage in this profile",
                    stage.output_mod
                )
                .into(),
            ));
        }
        let settings_json = serde_json::to_string(&stage.settings).map_err(|e| {
            CoreError::Other(format!("failed to serialize patcher settings: {e}").into())
        })?;
        self.db
            .execute(
                "INSERT INTO profile_patcher_stages (
                    profile_id, name, stage_kind, enabled, sort_index,
                    settings_json, output_mod, timeout_seconds, updated_at
                 )
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, {NOW})
                 ON CONFLICT(profile_id, name) DO UPDATE SET
                    stage_kind = excluded.stage_kind,
                    enabled = excluded.enabled,
                    sort_index = excluded.sort_index,
                    settings_json = excluded.settings_json,
                    output_mod = excluded.output_mod,
                    timeout_seconds = excluded.timeout_seconds,
                    updated_at = excluded.updated_at",
                &vals![
                    stage.profile_id,
                    stage.name.clone(),
                    stage.stage_kind.as_str(),
                    stage.enabled,
                    stage.sort_index,
                    settings_json,
                    stage.output_mod.clone(),
                    stage.timeout_seconds as i64,
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn mark_patcher_stage_cache_success(
        &self,
        profile_id: i64,
        stage_name: &str,
        cache_key: &str,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE profile_patcher_stages
                 SET last_cache_key = ?, last_success_at = {NOW}, updated_at = {NOW}
                 WHERE profile_id = ? AND name = ?",
                &vals![cache_key, profile_id, stage_name],
            )
            .await?;
        Ok(())
    }

    /// List patcher stages for a profile in execution order.
    pub async fn list_patcher_stages(&self, profile_id: i64) -> Result<Vec<PatcherStageRow>> {
        self.db
            .fetch_all(
                "SELECT profile_id, name, stage_kind, enabled, sort_index, settings_json, output_mod,
                        last_cache_key, last_success_at, timeout_seconds
                 FROM profile_patcher_stages
                 WHERE profile_id = ?
                 ORDER BY sort_index, lower(name)",
                &vals![profile_id],
                patcher_stage_from_row,
            )
            .await
    }

    /// Load one patcher stage by name.
    pub async fn load_patcher_stage(
        &self,
        profile_id: i64,
        name: &str,
    ) -> Result<Option<PatcherStageRow>> {
        self.db
            .fetch_optional(
                "SELECT profile_id, name, stage_kind, enabled, sort_index, settings_json, output_mod,
                        last_cache_key, last_success_at, timeout_seconds
                 FROM profile_patcher_stages
                 WHERE profile_id = ? AND name = ?",
                &vals![profile_id, name],
                patcher_stage_from_row,
            )
            .await
    }

    /// Delete a patcher stage. Returns whether a row was removed.
    pub async fn delete_patcher_stage(&self, profile_id: i64, name: &str) -> Result<bool> {
        let affected = self
            .db
            .execute(
                "DELETE FROM profile_patcher_stages WHERE profile_id = ? AND name = ?",
                &vals![profile_id, name],
            )
            .await?;
        Ok(affected > 0)
    }

    /// Toggle a patcher stage's enabled state.
    pub async fn set_patcher_stage_enabled(
        &self,
        profile_id: i64,
        name: &str,
        enabled: bool,
    ) -> Result<bool> {
        let affected = self
            .db
            .execute(
                "UPDATE profile_patcher_stages SET enabled = ?, updated_at = {NOW}
                 WHERE profile_id = ? AND name = ?",
                &vals![enabled, profile_id, name],
            )
            .await?;
        Ok(affected > 0)
    }

    /// Replace stage ordering for the supplied names.
    pub async fn reorder_patcher_stages(&self, profile_id: i64, names: &[String]) -> Result<()> {
        for (idx, name) in names.iter().enumerate() {
            self.db
                .execute(
                    "UPDATE profile_patcher_stages SET sort_index = ?, updated_at = {NOW}
                     WHERE profile_id = ? AND name = ?",
                    &vals![idx as i64, profile_id, name.clone()],
                )
                .await?;
        }
        Ok(())
    }

    /// Load every managed output path recorded for one patcher stage.
    pub async fn list_patcher_stage_outputs(
        &self,
        profile_id: i64,
        stage_name: &str,
    ) -> Result<Vec<PatcherStageOutputRow>> {
        self.db
            .fetch_all(
                "SELECT profile_id, stage_name, rel_path
                 FROM profile_patcher_stage_outputs
                 WHERE profile_id = ? AND stage_name = ?
                 ORDER BY rel_path",
                &vals![profile_id, stage_name],
                patcher_stage_output_from_row,
            )
            .await
    }

    /// Replace the recorded managed output manifest for one stage.
    pub async fn replace_patcher_stage_outputs(
        &self,
        profile_id: i64,
        stage_name: &str,
        rel_paths: &[String],
    ) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM profile_patcher_stage_outputs
                 WHERE profile_id = ? AND stage_name = ?",
                &vals![profile_id, stage_name],
            )
            .await?;
        for rel_path in rel_paths {
            self.db
                .execute(
                    "INSERT INTO profile_patcher_stage_outputs (
                        profile_id, stage_name, rel_path, updated_at
                     ) VALUES (?, ?, ?, {NOW})",
                    &vals![profile_id, stage_name, rel_path.clone()],
                )
                .await?;
        }
        Ok(())
    }

    /// Load every managed output path for all stages on this profile.
    pub async fn list_all_patcher_stage_outputs(
        &self,
        profile_id: i64,
    ) -> Result<Vec<PatcherStageOutputRow>> {
        self.db
            .fetch_all(
                "SELECT profile_id, stage_name, rel_path
                 FROM profile_patcher_stage_outputs
                 WHERE profile_id = ?
                 ORDER BY stage_name, rel_path",
                &vals![profile_id],
                patcher_stage_output_from_row,
            )
            .await
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

    // ── Performance telemetry CRUD ───────────────────────────────

    /// Create a pending performance run before launching the game.
    pub async fn create_performance_run(&self, run: &NewPerformanceRun) -> Result<()> {
        let mod_snapshot_json = serde_json::to_string(&run.mod_snapshot)?;
        let hash = mod_set_hash(&run.mod_snapshot);
        self.db
            .execute(
                "INSERT INTO performance_runs (
                    run_id, game_id, profile_id, profile_name, mod_set_hash,
                    mod_snapshot, experiment_depth, label, status
                 )
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'pending')",
                &vals![
                    run.run_id.clone(),
                    &run.game_id,
                    run.profile_id,
                    run.profile_name.clone(),
                    hash,
                    mod_snapshot_json,
                    run.experiment_depth as i64,
                    run.label.clone(),
                ],
            )
            .await?;
        Ok(())
    }

    /// Mark a performance run complete and replace its parsed samples.
    pub async fn complete_performance_run(
        &self,
        run_id: &str,
        csv_path: &Path,
        exit_status: Option<i64>,
        summary: &PerformanceSummary,
        samples: &[PerformanceSample],
    ) -> Result<()> {
        let mut tx = self.db.begin().await?;
        tx.execute(
            "UPDATE performance_runs SET
                status = 'complete',
                finished_at = {NOW},
                mangohud_csv_path = ?,
                exit_status = ?,
                sample_count = ?,
                duration_seconds = ?,
                median_fps = ?,
                average_fps = ?,
                one_percent_low_fps = ?,
                point_one_percent_low_fps = ?,
                median_frame_time_ms = ?,
                p95_frame_time_ms = ?,
                p99_frame_time_ms = ?
             WHERE run_id = ?",
            &vals![
                csv_path.to_string_lossy().to_string(),
                exit_status,
                summary.sample_count as i64,
                summary.duration_seconds,
                summary.median_fps,
                summary.average_fps,
                summary.one_percent_low_fps,
                summary.point_one_percent_low_fps,
                summary.median_frame_time_ms,
                summary.p95_frame_time_ms,
                summary.p99_frame_time_ms,
                run_id,
            ],
        )
        .await?;
        tx.execute(
            "DELETE FROM performance_samples WHERE run_id = ?",
            &vals![run_id],
        )
        .await?;
        for sample in samples {
            tx.execute(
                "INSERT INTO performance_samples (
                    run_id, elapsed_seconds, fps, frame_time_ms, cpu_load, gpu_load
                 )
                 VALUES (?, ?, ?, ?, ?, ?)",
                &vals![
                    run_id,
                    sample.elapsed_seconds,
                    sample.fps,
                    sample.frame_time_ms,
                    sample.cpu_load,
                    sample.gpu_load,
                ],
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Attach a CSV path to a run that could not be observed to completion.
    pub async fn mark_performance_run_pending(
        &self,
        run_id: &str,
        csv_path: Option<&Path>,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE performance_runs SET status = 'pending', mangohud_csv_path = ?
                 WHERE run_id = ?",
                &vals![csv_path.map(|p| p.to_string_lossy().to_string()), run_id,],
            )
            .await?;
        Ok(())
    }

    /// Load one performance run.
    pub async fn load_performance_run(&self, run_id: &str) -> Result<PerformanceRunRow> {
        self.db
            .fetch_optional(
                "SELECT run_id, game_id, profile_id, profile_name, mod_set_hash,
                        mod_snapshot, experiment_depth, label, status, started_at,
                        finished_at, mangohud_csv_path, exit_status, sample_count,
                        duration_seconds, median_fps, average_fps,
                        one_percent_low_fps, point_one_percent_low_fps,
                        median_frame_time_ms, p95_frame_time_ms, p99_frame_time_ms
                 FROM performance_runs
                 WHERE run_id = ?",
                &vals![run_id],
                performance_run_from_row,
            )
            .await?
            .ok_or_else(|| CoreError::Other(format!("performance run not found: {run_id}").into()))
    }

    /// List parsed `MangoHud` samples for a performance run in capture order.
    pub async fn list_performance_samples(&self, run_id: &str) -> Result<Vec<PerformanceSample>> {
        self.db
            .fetch_all(
                "SELECT elapsed_seconds, fps, frame_time_ms, cpu_load, gpu_load
                 FROM performance_samples
                 WHERE run_id = ?
                 ORDER BY id",
                &vals![run_id],
                performance_sample_from_row,
            )
            .await
    }

    /// List performance runs for a game, newest first.
    pub async fn list_performance_runs(
        &self,
        game_id: &GameId,
        profile_name: Option<&str>,
        limit: usize,
    ) -> Result<Vec<PerformanceRunRow>> {
        match profile_name {
            Some(profile_name) => {
                self.db
                    .fetch_all(
                        "SELECT run_id, game_id, profile_id, profile_name, mod_set_hash,
                                mod_snapshot, experiment_depth, label, status, started_at,
                                finished_at, mangohud_csv_path, exit_status, sample_count,
                                duration_seconds, median_fps, average_fps,
                                one_percent_low_fps, point_one_percent_low_fps,
                                median_frame_time_ms, p95_frame_time_ms, p99_frame_time_ms
                         FROM performance_runs
                         WHERE game_id = ? AND profile_name = ?
                         ORDER BY started_at DESC
                         LIMIT ?",
                        &vals![game_id, profile_name, limit as i64],
                        performance_run_from_row,
                    )
                    .await
            }
            None => {
                self.db
                    .fetch_all(
                        "SELECT run_id, game_id, profile_id, profile_name, mod_set_hash,
                                mod_snapshot, experiment_depth, label, status, started_at,
                                finished_at, mangohud_csv_path, exit_status, sample_count,
                                duration_seconds, median_fps, average_fps,
                                one_percent_low_fps, point_one_percent_low_fps,
                                median_frame_time_ms, p95_frame_time_ms, p99_frame_time_ms
                         FROM performance_runs
                         WHERE game_id = ?
                         ORDER BY started_at DESC
                         LIMIT ?",
                        &vals![game_id, limit as i64],
                        performance_run_from_row,
                    )
                    .await
            }
        }
    }

    /// Clear cross-crate UI test state stored outside profiles.
    ///
    /// The UI integration tests share one isolated on-disk database because
    /// the data directory override is process-global. This keeps test cleanup
    /// in the database layer, where table ownership and ordering are explicit.
    #[doc(hidden)]
    pub async fn clear_ui_test_state(&self) -> Result<()> {
        for table in [
            "tool_setting_edges",
            "tool_setting_nodes",
            "tool_applied_files",
            "profile_patcher_stage_outputs",
            "profile_patcher_stages",
            "game_tools",
            "executable_configs",
            "performance_samples",
            "performance_runs",
            "bisect_steps",
            "bisect_sessions",
        ] {
            self.db
                .execute(&format!("DELETE FROM {table}"), &vals![])
                .await?;
        }
        Ok(())
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

fn performance_run_from_row(r: &dyn DbRow) -> Result<PerformanceRunRow> {
    Ok(PerformanceRunRow {
        run_id: r.string(0)?,
        game_id: GameId::from(r.string(1)?),
        profile_id: r.opt_i64(2)?,
        profile_name: r.string(3)?,
        mod_set_hash: r.string(4)?,
        mod_snapshot_json: r.string(5)?,
        experiment_depth: r.i64(6)?.max(0) as usize,
        label: r.opt_string(7)?,
        status: r.string(8)?,
        started_at: r.string(9)?,
        finished_at: r.opt_string(10)?,
        mangohud_csv_path: r.opt_string(11)?.map(PathBuf::from),
        exit_status: r.opt_i64(12)?,
        summary: PerformanceSummary {
            sample_count: r.opt_i64(13)?.unwrap_or(0).max(0) as usize,
            duration_seconds: r.opt_f64(14)?,
            median_fps: r.opt_f64(15)?,
            average_fps: r.opt_f64(16)?,
            one_percent_low_fps: r.opt_f64(17)?,
            point_one_percent_low_fps: r.opt_f64(18)?,
            median_frame_time_ms: r.opt_f64(19)?,
            p95_frame_time_ms: r.opt_f64(20)?,
            p99_frame_time_ms: r.opt_f64(21)?,
        },
    })
}

fn performance_sample_from_row(r: &dyn DbRow) -> Result<PerformanceSample> {
    Ok(PerformanceSample {
        elapsed_seconds: r.opt_f64(0)?,
        fps: r.opt_f64(1)?.unwrap_or_default(),
        frame_time_ms: r.opt_f64(2)?,
        cpu_load: r.opt_f64(3)?,
        gpu_load: r.opt_f64(4)?,
    })
}

fn bisect_session_from_row(r: &dyn DbRow) -> Result<BisectSession> {
    let status_raw = r.string(5)?;
    let status = BisectStatus::parse(&status_raw).ok_or_else(|| {
        CoreError::Other(format!("invalid bisect session status: {status_raw}").into())
    })?;
    let safety_raw = r.string(11)?;
    let save_safety = match safety_raw.as_str() {
        "refuse" => BisectSaveSafety::Refuse,
        "force" => BisectSaveSafety::Force,
        _ => {
            return Err(CoreError::Other(
                format!("invalid bisect save safety mode: {safety_raw}").into(),
            ));
        }
    };
    Ok(BisectSession {
        session_id: r.string(0)?,
        game_id: GameId::from(r.string(1)?),
        source_profile_id: r.i64(2)?,
        source_profile_name: r.string(3)?,
        oracle: decode_json(&r.string(4)?, "bisect oracle")?,
        status,
        suspect_mod_ids: decode_json(&r.string(6)?, "bisect suspect mod ids")?,
        known_good_mod_ids: decode_json(&r.string(7)?, "bisect known good mod ids")?,
        known_bad_mod_ids: decode_json(&r.string(8)?, "bisect known bad mod ids")?,
        current_step_id: r.opt_i64(9)?,
        current_candidate_profile: r.opt_string(10)?,
        save_safety,
        keep_profiles: r.bool(12)?,
        created_at: r.string(13)?,
        updated_at: r.string(14)?,
    })
}

fn bisect_step_from_row(r: &dyn DbRow) -> Result<BisectStep> {
    let result = match r.opt_string(7)?.as_deref() {
        None => None,
        Some("good") => Some(BisectResult::Good),
        Some("bad") => Some(BisectResult::Bad),
        Some(raw) => {
            return Err(CoreError::Other(
                format!("invalid bisect step result: {raw}").into(),
            ));
        }
    };
    Ok(BisectStep {
        id: r.i64(0)?,
        session_id: r.string(1)?,
        step_index: r.i64(2)?.max(0) as usize,
        candidate_profile: r.string(3)?,
        candidate_mod_ids: decode_json(&r.string(4)?, "bisect candidate mod ids")?,
        enabled_mod_ids: decode_json(&r.string(5)?, "bisect enabled mod ids")?,
        disabled_mod_ids: decode_json(&r.string(6)?, "bisect disabled mod ids")?,
        result,
        observed_signal: r.opt_string(8)?,
        notes: r.opt_string(9)?,
        launched_at: r.string(10)?,
    })
}

fn encode_json(value: &(impl serde::Serialize + ?Sized), label: &str) -> Result<String> {
    serde_json::to_string(value)
        .map_err(|e| CoreError::Other(format!("failed to encode {label}: {e}").into()))
}

fn decode_json<T: serde::de::DeserializeOwned>(raw: &str, label: &str) -> Result<T> {
    serde_json::from_str(raw)
        .map_err(|e| CoreError::Other(format!("failed to decode {label}: {e}").into()))
}

fn patcher_stage_from_row(r: &dyn DbRow) -> Result<PatcherStageRow> {
    let stage_kind = PatcherStageKind::parse(&r.string(2)?)?;
    let settings_json = r.string(5)?;
    let settings = serde_json::from_str::<PatcherStageSettings>(&settings_json).map_err(|e| {
        CoreError::Other(format!("failed to parse patcher stage settings: {e}").into())
    })?;
    if settings.kind() != stage_kind {
        return Err(CoreError::Validation(
            format!(
                "patcher stage kind '{}' does not match serialized settings kind '{}'",
                stage_kind.as_str(),
                settings.kind().as_str()
            )
            .into(),
        ));
    }
    Ok(PatcherStageRow {
        profile_id: r.i64(0)?,
        name: r.string(1)?,
        stage_kind,
        enabled: r.bool(3)?,
        sort_index: r.i64(4)?,
        settings,
        output_mod: r.string(6)?,
        last_cache_key: r.opt_string(7)?,
        last_success_at: r.opt_string(8)?,
        timeout_seconds: r.opt_i64(9)?.unwrap_or(1_800).max(1) as u64,
    })
}

fn patcher_stage_output_from_row(r: &dyn DbRow) -> Result<PatcherStageOutputRow> {
    Ok(PatcherStageOutputRow {
        profile_id: r.i64(0)?,
        stage_name: r.string(1)?,
        rel_path: r.string(2)?,
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
