//! PostgreSQL end-state schema and migration entrypoint.

use tracing::info;

use super::CURRENT_SCHEMA_VERSION;
use crate::error::Result;

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

CREATE TABLE IF NOT EXISTS profile_patcher_stages (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    profile_id    BIGINT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    stage_kind    TEXT NOT NULL,
    enabled       BOOLEAN NOT NULL DEFAULT TRUE,
    sort_index    BIGINT NOT NULL,
    settings_json TEXT NOT NULL,
    output_mod    TEXT NOT NULL,
    last_cache_key TEXT,
    last_success_at TEXT,
    timeout_seconds BIGINT NOT NULL DEFAULT 1800,
    updated_at    TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE(profile_id, name)
);

CREATE TABLE IF NOT EXISTS crash_logs (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    game_id         TEXT NOT NULL,
    profile_id      BIGINT REFERENCES profiles(id) ON DELETE SET NULL,
    profile_name    TEXT,
    source_path     TEXT NOT NULL,
    logger_format   TEXT NOT NULL,
    imported_at     TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS')),
    raw_sha256      TEXT NOT NULL,
    raw_log         TEXT NOT NULL,
    signature_json  TEXT NOT NULL,
    report_json     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS performance_runs (
    run_id                      TEXT PRIMARY KEY,
    game_id                     TEXT NOT NULL,
    profile_id                  BIGINT REFERENCES profiles(id) ON DELETE SET NULL,
    profile_name                TEXT NOT NULL,
    mod_set_hash                TEXT NOT NULL,
    mod_snapshot                TEXT NOT NULL,
    experiment_depth            BIGINT NOT NULL DEFAULT 0,
    label                       TEXT,
    status                      TEXT NOT NULL DEFAULT 'pending',
    started_at                  TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS')),
    finished_at                 TEXT,
    mangohud_csv_path           TEXT,
    exit_status                 BIGINT,
    sample_count                BIGINT,
    duration_seconds            DOUBLE PRECISION,
    median_fps                  DOUBLE PRECISION,
    average_fps                 DOUBLE PRECISION,
    one_percent_low_fps         DOUBLE PRECISION,
    point_one_percent_low_fps   DOUBLE PRECISION,
    median_frame_time_ms        DOUBLE PRECISION,
    p95_frame_time_ms           DOUBLE PRECISION,
    p99_frame_time_ms           DOUBLE PRECISION
);

CREATE TABLE IF NOT EXISTS performance_samples (
    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    run_id              TEXT NOT NULL REFERENCES performance_runs(run_id) ON DELETE CASCADE,
    elapsed_seconds     DOUBLE PRECISION,
    fps                 DOUBLE PRECISION NOT NULL,
    frame_time_ms       DOUBLE PRECISION,
    cpu_load            DOUBLE PRECISION,
    gpu_load            DOUBLE PRECISION
);

CREATE TABLE IF NOT EXISTS profile_patcher_stage_outputs (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    profile_id  BIGINT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    stage_name  TEXT NOT NULL,
    mod_id      TEXT NOT NULL,
    rel_path    TEXT NOT NULL,
    size        BIGINT NOT NULL,
    sha256      TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE(profile_id, stage_name, rel_path)
);

CREATE TABLE IF NOT EXISTS bisect_sessions (
    session_id                 TEXT PRIMARY KEY,
    game_id                    TEXT NOT NULL,
    source_profile_id          BIGINT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    source_profile_name        TEXT NOT NULL,
    oracle_json                TEXT NOT NULL,
    status                     TEXT NOT NULL,
    suspect_mod_ids_json       TEXT NOT NULL,
    known_good_mod_ids_json    TEXT NOT NULL DEFAULT '[]',
    known_bad_mod_ids_json     TEXT NOT NULL DEFAULT '[]',
    current_step_id            BIGINT,
    current_candidate_profile  TEXT,
    save_safety                TEXT NOT NULL,
    keep_profiles              BOOLEAN NOT NULL DEFAULT FALSE,
    created_at                 TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS')),
    updated_at                 TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS'))
);

CREATE TABLE IF NOT EXISTS bisect_steps (
    id                     BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    session_id             TEXT NOT NULL REFERENCES bisect_sessions(session_id) ON DELETE CASCADE,
    step_index             BIGINT NOT NULL,
    candidate_profile      TEXT NOT NULL,
    candidate_mod_ids_json TEXT NOT NULL,
    enabled_mod_ids_json   TEXT NOT NULL,
    disabled_mod_ids_json  TEXT NOT NULL,
    result                 TEXT,
    observed_signal        TEXT,
    notes                  TEXT,
    launched_at            TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS')),
    UNIQUE(session_id, step_index)
);

CREATE TABLE IF NOT EXISTS profile_state_snapshots (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    profile_id      BIGINT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    game_id         TEXT NOT NULL,
    profile_name    TEXT NOT NULL,
    snapshot_json   TEXT NOT NULL,
    created_at      TEXT NOT NULL DEFAULT (to_char(now(), 'YYYY-MM-DD HH24:MI:SS'))
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
CREATE INDEX IF NOT EXISTS idx_patcher_stages_profile ON profile_patcher_stages(profile_id, sort_index);
CREATE INDEX IF NOT EXISTS idx_patcher_stage_outputs_profile ON profile_patcher_stage_outputs(profile_id, stage_name);
CREATE INDEX IF NOT EXISTS idx_crash_logs_game_profile ON crash_logs(game_id, profile_id, imported_at);
CREATE INDEX IF NOT EXISTS idx_crash_logs_sha ON crash_logs(raw_sha256);
CREATE INDEX IF NOT EXISTS idx_performance_runs_game_profile ON performance_runs(game_id, profile_name, started_at);
CREATE INDEX IF NOT EXISTS idx_performance_samples_run ON performance_samples(run_id);
CREATE INDEX IF NOT EXISTS idx_bisect_sessions_game_status ON bisect_sessions(game_id, status, updated_at);
CREATE INDEX IF NOT EXISTS idx_bisect_steps_session ON bisect_steps(session_id, step_index);
CREATE INDEX IF NOT EXISTS idx_profile_state_snapshots_profile ON profile_state_snapshots(profile_id, created_at);
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
        sqlx::raw_sql(SCHEMA_PG).execute(pool).await?;
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
