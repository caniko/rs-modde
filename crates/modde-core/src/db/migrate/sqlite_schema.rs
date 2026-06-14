//! SQLite schema constants for the migration ladder.

pub(super) const SCHEMA_V1: &str = "
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

pub(super) const SCHEMA_V2: &str = "
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

pub(super) const SCHEMA_V3: &str = "
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

pub(super) const SCHEMA_V8: &str = "
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

pub(super) const SCHEMA_V9: &str = "
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

pub(super) const SCHEMA_V10: &str = "
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

pub(super) const SCHEMA_V11: &str = "
CREATE TABLE IF NOT EXISTS profile_patcher_stages (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id    INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    stage_kind    TEXT NOT NULL,
    enabled       INTEGER NOT NULL DEFAULT 1,
    sort_index    INTEGER NOT NULL,
    settings_json TEXT NOT NULL,
    output_mod    TEXT NOT NULL,
    updated_at    TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(profile_id, name)
);

CREATE INDEX IF NOT EXISTS idx_patcher_stages_profile
    ON profile_patcher_stages(profile_id, sort_index);
";

pub(super) const SCHEMA_V12: &str = "
CREATE TABLE IF NOT EXISTS crash_logs (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id         TEXT NOT NULL,
    profile_id      INTEGER REFERENCES profiles(id) ON DELETE SET NULL,
    profile_name    TEXT,
    source_path     TEXT NOT NULL,
    logger_format   TEXT NOT NULL,
    imported_at     TEXT NOT NULL DEFAULT (datetime('now')),
    raw_sha256      TEXT NOT NULL,
    raw_log         TEXT NOT NULL,
    signature_json  TEXT NOT NULL,
    report_json     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_crash_logs_game_profile
    ON crash_logs(game_id, profile_id, imported_at);
CREATE INDEX IF NOT EXISTS idx_crash_logs_sha
    ON crash_logs(raw_sha256);
";

pub(super) const SCHEMA_V13: &str = "
CREATE TABLE IF NOT EXISTS performance_runs (
    run_id                      TEXT PRIMARY KEY,
    game_id                     TEXT NOT NULL,
    profile_id                  INTEGER REFERENCES profiles(id) ON DELETE SET NULL,
    profile_name                TEXT NOT NULL,
    mod_set_hash                TEXT NOT NULL,
    mod_snapshot                TEXT NOT NULL,
    experiment_depth            INTEGER NOT NULL DEFAULT 0,
    label                       TEXT,
    status                      TEXT NOT NULL DEFAULT 'pending',
    started_at                  TEXT NOT NULL DEFAULT (datetime('now')),
    finished_at                 TEXT,
    mangohud_csv_path           TEXT,
    exit_status                 INTEGER,
    sample_count                INTEGER,
    duration_seconds            REAL,
    median_fps                  REAL,
    average_fps                 REAL,
    one_percent_low_fps         REAL,
    point_one_percent_low_fps   REAL,
    median_frame_time_ms        REAL,
    p95_frame_time_ms           REAL,
    p99_frame_time_ms           REAL
);

CREATE TABLE IF NOT EXISTS performance_samples (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id              TEXT NOT NULL REFERENCES performance_runs(run_id) ON DELETE CASCADE,
    elapsed_seconds     REAL,
    fps                 REAL NOT NULL,
    frame_time_ms       REAL,
    cpu_load            REAL,
    gpu_load            REAL
);

CREATE INDEX IF NOT EXISTS idx_performance_runs_game_profile
    ON performance_runs(game_id, profile_name, started_at);
CREATE INDEX IF NOT EXISTS idx_performance_samples_run
    ON performance_samples(run_id);
";

pub(super) const SCHEMA_V14: &str = "
CREATE TABLE IF NOT EXISTS profile_patcher_stage_outputs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    stage_name  TEXT NOT NULL,
    rel_path    TEXT NOT NULL,
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(profile_id, stage_name, rel_path)
);

CREATE INDEX IF NOT EXISTS idx_patcher_stage_outputs_profile
    ON profile_patcher_stage_outputs(profile_id, stage_name);
";

pub(super) const SCHEMA_V15: &str = "
CREATE TABLE IF NOT EXISTS bisect_sessions (
    session_id                 TEXT PRIMARY KEY,
    game_id                    TEXT NOT NULL,
    source_profile_id          INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    source_profile_name        TEXT NOT NULL,
    oracle_json                TEXT NOT NULL,
    status                     TEXT NOT NULL,
    suspect_mod_ids_json       TEXT NOT NULL,
    known_good_mod_ids_json    TEXT NOT NULL DEFAULT '[]',
    known_bad_mod_ids_json     TEXT NOT NULL DEFAULT '[]',
    current_step_id            INTEGER,
    current_candidate_profile  TEXT,
    save_safety                TEXT NOT NULL,
    keep_profiles              INTEGER NOT NULL DEFAULT 0,
    created_at                 TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at                 TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS bisect_steps (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id            TEXT NOT NULL REFERENCES bisect_sessions(session_id) ON DELETE CASCADE,
    step_index            INTEGER NOT NULL,
    candidate_profile     TEXT NOT NULL,
    candidate_mod_ids_json TEXT NOT NULL,
    enabled_mod_ids_json  TEXT NOT NULL,
    disabled_mod_ids_json TEXT NOT NULL,
    result                TEXT,
    observed_signal       TEXT,
    notes                 TEXT,
    launched_at           TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(session_id, step_index)
);

CREATE TABLE IF NOT EXISTS profile_state_snapshots (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id      INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    game_id         TEXT NOT NULL,
    profile_name    TEXT NOT NULL,
    snapshot_json   TEXT NOT NULL,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_bisect_sessions_game_status
    ON bisect_sessions(game_id, status, updated_at);
CREATE INDEX IF NOT EXISTS idx_bisect_steps_session
    ON bisect_steps(session_id, step_index);
";

pub(super) const SCHEMA_V16: &str = "
CREATE TABLE IF NOT EXISTS profile_state_snapshots (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id      INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    game_id         TEXT NOT NULL,
    profile_name    TEXT NOT NULL,
    snapshot_json   TEXT NOT NULL,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_profile_state_snapshots_profile
    ON profile_state_snapshots(profile_id, created_at);
";

pub(super) const SCHEMA_V17: &str = "
ALTER TABLE profile_patcher_stages ADD COLUMN last_cache_key TEXT;
ALTER TABLE profile_patcher_stages ADD COLUMN last_success_at TEXT;
ALTER TABLE profile_patcher_stages ADD COLUMN timeout_seconds INTEGER NOT NULL DEFAULT 1800;
";

// ── SQLite migration ladder ─────────────────────────────────────────────────
