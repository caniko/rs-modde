//! Schema creation and migration — the only genuinely per-backend code.
//!
//! **`SQLite`** keeps modde's original `PRAGMA user_version` migration ladder
//! byte-for-byte (`SCHEMA_V1..V10` plus the imperative column adds for the
//! squashed V4–V7 steps), so an existing on-disk database upgrades exactly as
//! it did under `rusqlite`. Do not "modernize" this ladder.
//!
//! **`PostgreSQL`** has no existing databases in the wild, so it gets a single
//! forward-only, fully `IF NOT EXISTS`-idempotent end-state schema (the V10
//! shape), version-tracked in a `schema_version` table. `AUTOINCREMENT` becomes
//! `GENERATED ALWAYS AS IDENTITY`, the `INTEGER`-boolean columns become real
//! `BOOLEAN`, and the timestamp `TEXT` columns keep a `to_char(now(), …)`
//! default so they read back as the same `String` format `SQLite` produces.

use tracing::info;

use crate::error::Result;

/// Current schema version. Bump this and add a migration step (`SQLite` ladder +
/// Postgres end-state DDL) when the schema changes.
pub(crate) const CURRENT_SCHEMA_VERSION: i64 = 10;

// ── SQLite schema constants (verbatim from the original rusqlite layer) ──────

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
CREATE TABLE IF NOT EXISTS hidden_files (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    mod_id     TEXT NOT NULL,
    rel_path   TEXT NOT NULL,
    hidden_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(profile_id, mod_id, rel_path)
);

CREATE TABLE IF NOT EXISTS plugin_order (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    plugin_name TEXT NOT NULL,
    sort_index  INTEGER NOT NULL,
    enabled     INTEGER NOT NULL DEFAULT 1,
    UNIQUE(profile_id, plugin_name)
);

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
CREATE TABLE IF NOT EXISTS game_tools (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id     TEXT NOT NULL,
    tool_id     TEXT NOT NULL,
    enabled     INTEGER NOT NULL DEFAULT 0,
    settings    TEXT NOT NULL DEFAULT '{}',
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(game_id, tool_id)
);

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

// ── SQLite migration ladder ─────────────────────────────────────────────────

async fn sqlite_user_version(pool: &sqlx::SqlitePool) -> Result<i64> {
    use sqlx::Row;
    let row = sqlx::query("PRAGMA user_version").fetch_one(pool).await?;
    Ok(row.try_get(0)?)
}

async fn sqlite_set_user_version(pool: &sqlx::SqlitePool, version: i64) -> Result<()> {
    // PRAGMA values cannot be bound; `version` is an internal constant.
    sqlx::query(&format!("PRAGMA user_version = {version}"))
        .execute(pool)
        .await?;
    Ok(())
}

async fn sqlite_column_exists(pool: &sqlx::SqlitePool, table: &str, column: &str) -> Result<bool> {
    use sqlx::Row;
    let rows = sqlx::query(&format!("PRAGMA table_info({table})"))
        .fetch_all(pool)
        .await?;
    for row in &rows {
        let name: String = row.try_get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn sqlite_add_column_if_missing(
    pool: &sqlx::SqlitePool,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    if sqlite_column_exists(pool, table, column).await? {
        return Ok(());
    }
    sqlx::raw_sql(&format!(
        "ALTER TABLE {table} ADD COLUMN {column} {definition};"
    ))
    .execute(pool)
    .await?;
    Ok(())
}

/// Apply the `SQLite` migration ladder, preserving the historical `user_version`
/// gates exactly (including the squashed V4/V5 steps).
pub(crate) async fn migrate_sqlite(pool: &sqlx::SqlitePool) -> Result<()> {
    let version = sqlite_user_version(pool).await?;

    if version < 1 {
        sqlx::raw_sql(SCHEMA_V1).execute(pool).await?;
        info!(from = version, to = 1, "database schema migrated to V1");
    }

    if version < 2 {
        sqlx::raw_sql(SCHEMA_V2).execute(pool).await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "nexus_mod_id", "INTEGER").await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "nexus_file_id", "INTEGER").await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "nexus_game_domain", "TEXT").await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "installed_timestamp", "INTEGER")
            .await?;
        sqlite_add_column_if_missing(
            pool,
            "profile_mods",
            "category_id",
            "INTEGER REFERENCES mod_categories(id)",
        )
        .await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "notes", "TEXT").await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "tags", "TEXT").await?;
        info!(
            from = version.max(1),
            to = 2,
            "database schema migrated to V2"
        );
    }

    if version < 3 {
        sqlx::raw_sql(SCHEMA_V3).execute(pool).await?;
        info!(
            from = version.max(2),
            to = 3,
            "database schema migrated to V3"
        );
    }

    if version < 6 {
        sqlite_add_column_if_missing(pool, "profile_mods", "display_name", "TEXT").await?;
        info!(
            from = version.max(5),
            to = 6,
            "database schema migrated to V6"
        );
    }

    if version < 7 {
        sqlite_add_column_if_missing(pool, "profiles", "load_order_lock", "TEXT").await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "lock_reason", "TEXT").await?;
        info!(
            from = version.max(6),
            to = 7,
            "database schema migrated to V7"
        );
    }

    if version < 8 {
        sqlite_add_column_if_missing(pool, "profile_mods", "install_method", "TEXT").await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "source_archive_hash", "TEXT").await?;
        sqlite_add_column_if_missing(pool, "profile_mods", "install_status", "TEXT").await?;
        sqlx::raw_sql(SCHEMA_V8).execute(pool).await?;
        info!(
            from = version.max(7),
            to = 8,
            "database schema migrated to V8"
        );
    }

    if version < 9 {
        sqlx::raw_sql(SCHEMA_V9).execute(pool).await?;
        info!(
            from = version.max(8),
            to = 9,
            "database schema migrated to V9"
        );
    }

    if version < 10 {
        sqlx::raw_sql(SCHEMA_V10).execute(pool).await?;
        sqlite_add_column_if_missing(pool, "game_tools", "current_node_id", "TEXT").await?;
        info!(
            from = version.max(9),
            to = 10,
            "database schema migrated to V10"
        );
    }

    if version < CURRENT_SCHEMA_VERSION {
        sqlite_set_user_version(pool, CURRENT_SCHEMA_VERSION).await?;
    }

    Ok(())
}

// ── PostgreSQL end-state schema ─────────────────────────────────────────────

#[cfg(feature = "postgres")]
const SCHEMA_PG: &str = "
CREATE TABLE IF NOT EXISTS profiles (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name        TEXT NOT NULL,
    game_id     TEXT NOT NULL,
    source_type TEXT NOT NULL DEFAULT 'manual',
    source_data TEXT,
    overrides   TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at  TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS')),
    load_order_lock TEXT,
    UNIQUE(name, game_id)
);

CREATE TABLE IF NOT EXISTS mod_categories (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    profile_id BIGINT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    color      TEXT,
    sort_index BIGINT NOT NULL,
    UNIQUE(profile_id, name)
);

CREATE TABLE IF NOT EXISTS profile_mods (
    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    profile_id          BIGINT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    mod_id              TEXT NOT NULL,
    enabled             BOOLEAN NOT NULL DEFAULT TRUE,
    version             TEXT,
    fomod_config        TEXT,
    sort_index          BIGINT NOT NULL,
    nexus_mod_id        BIGINT,
    nexus_file_id       BIGINT,
    nexus_game_domain   TEXT,
    installed_timestamp BIGINT,
    category_id         BIGINT REFERENCES mod_categories(id),
    notes               TEXT,
    tags                TEXT,
    display_name        TEXT,
    lock_reason         TEXT,
    install_method      TEXT,
    source_archive_hash TEXT,
    install_status      TEXT,
    UNIQUE(profile_id, mod_id)
);

CREATE TABLE IF NOT EXISTS load_order_rules (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    profile_id  BIGINT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    rule_type   TEXT NOT NULL,
    mod_a       TEXT NOT NULL,
    mod_b       TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS saves (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    profile_id  BIGINT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    path        TEXT NOT NULL UNIQUE,
    label       TEXT,
    assigned_at TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE IF NOT EXISTS stock_snapshots (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    game_id     TEXT NOT NULL UNIQUE,
    snapshot_path TEXT NOT NULL,
    tree_hash   TEXT NOT NULL,
    file_count  BIGINT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE IF NOT EXISTS active_profiles (
    game_id     TEXT PRIMARY KEY,
    profile_id  BIGINT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    activated_at TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE IF NOT EXISTS experiment_stack (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    game_id     TEXT NOT NULL,
    profile_id  BIGINT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    depth       BIGINT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE IF NOT EXISTS hidden_files (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    profile_id BIGINT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    mod_id     TEXT NOT NULL,
    rel_path   TEXT NOT NULL,
    hidden_at  TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE(profile_id, mod_id, rel_path)
);

CREATE TABLE IF NOT EXISTS plugin_order (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    profile_id  BIGINT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    plugin_name TEXT NOT NULL,
    sort_index  BIGINT NOT NULL,
    enabled     BOOLEAN NOT NULL DEFAULT TRUE,
    UNIQUE(profile_id, plugin_name)
);

CREATE TABLE IF NOT EXISTS game_tools (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    game_id         TEXT NOT NULL,
    tool_id         TEXT NOT NULL,
    enabled         BOOLEAN NOT NULL DEFAULT FALSE,
    settings        TEXT NOT NULL DEFAULT '{}',
    updated_at      TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS')),
    current_node_id TEXT,
    UNIQUE(game_id, tool_id)
);

CREATE TABLE IF NOT EXISTS tool_applied_files (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    game_id     TEXT NOT NULL,
    tool_id     TEXT NOT NULL,
    rel_path    TEXT NOT NULL,
    applied_at  TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE(game_id, tool_id, rel_path)
);

CREATE TABLE IF NOT EXISTS installed_mod_files (
    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    profile_id          BIGINT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    mod_id              TEXT NOT NULL,
    rel_path            TEXT NOT NULL,
    origin_rel_path     TEXT NOT NULL,
    size                BIGINT NOT NULL,
    merge_group         TEXT,
    UNIQUE(profile_id, mod_id, rel_path)
);

CREATE TABLE IF NOT EXISTS executable_configs (
    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    game_id             TEXT NOT NULL,
    name                TEXT NOT NULL,
    executable_path     TEXT NOT NULL,
    arguments           TEXT NOT NULL DEFAULT '[]',
    working_dir         TEXT,
    environment         TEXT NOT NULL DEFAULT '{}',
    wine_dll_overrides  TEXT,
    output_mod          TEXT NOT NULL DEFAULT '__overwrite__',
    enabled             BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at          TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE(game_id, name)
);

CREATE TABLE IF NOT EXISTS tool_setting_nodes (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    node_id     TEXT NOT NULL UNIQUE,
    game_id     TEXT NOT NULL,
    tool_id     TEXT NOT NULL,
    enabled     BOOLEAN NOT NULL,
    settings    TEXT NOT NULL,
    reason      TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE IF NOT EXISTS tool_setting_edges (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    parent_node_id  TEXT NOT NULL,
    child_node_id   TEXT NOT NULL,
    created_at      TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE(parent_node_id, child_node_id)
);

CREATE INDEX IF NOT EXISTS idx_profiles_game ON profiles(game_id);
CREATE INDEX IF NOT EXISTS idx_mods_profile ON profile_mods(profile_id);
CREATE INDEX IF NOT EXISTS idx_rules_profile ON load_order_rules(profile_id);
CREATE INDEX IF NOT EXISTS idx_saves_profile ON saves(profile_id);
CREATE INDEX IF NOT EXISTS idx_experiment_game ON experiment_stack(game_id, depth);
CREATE INDEX IF NOT EXISTS idx_hidden_profile ON hidden_files(profile_id);
CREATE INDEX IF NOT EXISTS idx_plugin_order_profile ON plugin_order(profile_id);
CREATE INDEX IF NOT EXISTS idx_categories_profile ON mod_categories(profile_id);
CREATE INDEX IF NOT EXISTS idx_game_tools_game ON game_tools(game_id);
CREATE INDEX IF NOT EXISTS idx_tool_files_game ON tool_applied_files(game_id, tool_id);
CREATE INDEX IF NOT EXISTS idx_imf_profile_mod ON installed_mod_files(profile_id, mod_id);
CREATE INDEX IF NOT EXISTS idx_imf_merge_group ON installed_mod_files(merge_group) WHERE merge_group IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_executable_configs_game ON executable_configs(game_id);
CREATE INDEX IF NOT EXISTS idx_tool_setting_nodes_tool ON tool_setting_nodes(game_id, tool_id, created_at);
CREATE INDEX IF NOT EXISTS idx_tool_setting_edges_child ON tool_setting_edges(child_node_id);
";

/// Create/upgrade the `PostgreSQL` schema. Forward-only and fully idempotent
/// (every statement is `IF NOT EXISTS`), gated on a `schema_version` row.
#[cfg(feature = "postgres")]
pub(crate) async fn migrate_postgres(pool: &sqlx::PgPool) -> Result<()> {
    use sqlx::Row;

    sqlx::query("CREATE TABLE IF NOT EXISTS schema_version (version BIGINT NOT NULL)")
        .execute(pool)
        .await?;

    let current: Option<i64> = sqlx::query("SELECT version FROM schema_version LIMIT 1")
        .fetch_optional(pool)
        .await?
        .map(|r| r.try_get(0))
        .transpose()?;

    if current.unwrap_or(0) >= CURRENT_SCHEMA_VERSION {
        return Ok(());
    }

    let mut tx = pool.begin().await?;
    sqlx::raw_sql(SCHEMA_PG).execute(&mut *tx).await?;
    if current.is_none() {
        sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
            .bind(CURRENT_SCHEMA_VERSION)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query("UPDATE schema_version SET version = $1")
            .bind(CURRENT_SCHEMA_VERSION)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    info!(to = CURRENT_SCHEMA_VERSION, "postgres schema ensured");
    Ok(())
}
