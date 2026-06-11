use super::backend::vals;
use super::*;
#[cfg(feature = "postgres")]
use serial_test::serial;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::patcher::{CommandSettings, PatcherStageRow, PatcherStageSettings};

/// Test-only raw query helpers, replacing the previous direct `conn` access.
#[cfg(test)]
impl ModdeDb {
    async fn test_exec(&self, sql: &str, params: &[Val]) -> Result<u64> {
        self.db.execute(sql, params).await
    }

    async fn test_two_i64(&self, sql: &str, params: &[Val]) -> Result<(i64, i64)> {
        self.db
            .fetch_one(sql, params, |r| Ok((r.i64(0)?, r.i64(1)?)))
            .await
    }

    async fn test_three_str(&self, sql: &str, params: &[Val]) -> Result<(String, String, String)> {
        self.db
            .fetch_one(sql, params, |r| {
                Ok((r.string(0)?, r.string(1)?, r.string(2)?))
            })
            .await
    }
}

async fn test_db() -> ModdeDb {
    ModdeDb::open_memory().await.unwrap()
}

#[cfg(feature = "postgres")]
fn env_from(values: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    let values: HashMap<&'static str, String> = values
        .iter()
        .map(|(key, value)| (*key, (*value).to_string()))
        .collect();
    move |key| values.get(key).cloned()
}

#[cfg(feature = "postgres")]
fn empty_env(_key: &str) -> Option<String> {
    None
}

#[cfg(feature = "postgres")]
struct EnvRestore {
    key: &'static str,
    previous: Option<String>,
}

#[cfg(feature = "postgres")]
impl EnvRestore {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        // SAFETY: this test helper is used only by #[serial] tests that restore
        // the exact variables they mutate before returning.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }
}

#[cfg(feature = "postgres")]
impl Drop for EnvRestore {
    fn drop(&mut self) {
        // SAFETY: the serial test guard restores process environment after a
        // test-scoped mutation and is not shared across concurrently running
        // tests in this module.
        unsafe {
            if let Some(value) = &self.previous {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_accepts_discrete_env_only_config() {
    let opts = build_pg_options(
        &DatabaseSettings::default(),
        &env_from(&[
            ("MODDE_DATABASE_HOST", "pg.example.test"),
            ("MODDE_DATABASE_PORT", "15432"),
            ("MODDE_DATABASE_NAME", "modde_env"),
            ("MODDE_DATABASE_USER", "modde_user"),
        ]),
    )
    .unwrap();

    assert_eq!(opts.get_host(), "pg.example.test");
    assert_eq!(opts.get_port(), 15432);
    assert_eq!(opts.get_database(), Some("modde_env"));
    assert_eq!(opts.get_username(), "modde_user");
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_env_overrides_each_discrete_setting() {
    let settings = DatabaseSettings {
        host: Some("settings-host".to_string()),
        port: Some(5432),
        dbname: Some("settings_db".to_string()),
        user: Some("settings_user".to_string()),
        ..DatabaseSettings::default()
    };

    let opts = build_pg_options(
        &settings,
        &env_from(&[
            ("MODDE_DATABASE_HOST", "env-host"),
            ("MODDE_DATABASE_PORT", "6543"),
            ("MODDE_DATABASE_NAME", "env_db"),
            ("MODDE_DATABASE_USER", "env_user"),
        ]),
    )
    .unwrap();

    assert_eq!(opts.get_host(), "env-host");
    assert_eq!(opts.get_port(), 6543);
    assert_eq!(opts.get_database(), Some("env_db"));
    assert_eq!(opts.get_username(), "env_user");
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_uses_settings_when_env_is_empty() {
    let settings = DatabaseSettings {
        host: Some("settings-host".to_string()),
        port: Some(15432),
        dbname: Some("settings_db".to_string()),
        user: Some("settings_user".to_string()),
        ..DatabaseSettings::default()
    };

    let opts = build_pg_options(&settings, &empty_env).unwrap();

    assert_eq!(opts.get_host(), "settings-host");
    assert_eq!(opts.get_port(), 15432);
    assert_eq!(opts.get_database(), Some("settings_db"));
    assert_eq!(opts.get_username(), "settings_user");
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_requires_merged_database_name() {
    let err = build_pg_options(
        &DatabaseSettings {
            host: Some("settings-host".to_string()),
            ..DatabaseSettings::default()
        },
        &empty_env,
    )
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("postgres backend selected but no database name configured")
    );
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_reads_modde_database_name_not_dbname() {
    let opts = build_pg_options(
        &DatabaseSettings::default(),
        &env_from(&[
            ("MODDE_DATABASE_DBNAME", "wrong_key"),
            ("MODDE_DATABASE_NAME", "right_key"),
        ]),
    )
    .unwrap();

    assert_eq!(opts.get_database(), Some("right_key"));
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_url_takes_precedence_over_discrete_fields() {
    let settings = DatabaseSettings {
        url: Some("postgres://settings_user@settings-host:15432/settings_db".to_string()),
        host: Some("settings-discrete-host".to_string()),
        port: Some(5432),
        dbname: Some("settings_discrete_db".to_string()),
        user: Some("settings_discrete_user".to_string()),
        ..DatabaseSettings::default()
    };

    let opts = build_pg_options(
        &settings,
        &env_from(&[
            ("MODDE_DATABASE_HOST", "env-discrete-host"),
            ("MODDE_DATABASE_PORT", "6543"),
            ("MODDE_DATABASE_NAME", "env_discrete_db"),
            ("MODDE_DATABASE_USER", "env_discrete_user"),
        ]),
    )
    .unwrap();

    assert_eq!(opts.get_host(), "settings-host");
    assert_eq!(opts.get_port(), 15432);
    assert_eq!(opts.get_database(), Some("settings_db"));
    assert_eq!(opts.get_username(), "settings_user");
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_settings_url_takes_precedence_over_env_discrete_fields() {
    let settings = DatabaseSettings {
        url: Some("postgres://settings_user@settings-host:15432/settings_db".to_string()),
        ..DatabaseSettings::default()
    };

    let opts = build_pg_options(
        &settings,
        &env_from(&[
            ("MODDE_DATABASE_HOST", "env-host"),
            ("MODDE_DATABASE_PORT", "6543"),
            ("MODDE_DATABASE_NAME", "env_db"),
            ("MODDE_DATABASE_USER", "env_user"),
        ]),
    )
    .unwrap();

    assert_eq!(opts.get_host(), "settings-host");
    assert_eq!(opts.get_port(), 15432);
    assert_eq!(opts.get_database(), Some("settings_db"));
    assert_eq!(opts.get_username(), "settings_user");
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_env_url_takes_precedence_over_settings_url() {
    let settings = DatabaseSettings {
        url: Some("postgres://settings_user@settings-host:15432/settings_db".to_string()),
        ..DatabaseSettings::default()
    };

    let opts = build_pg_options(
        &settings,
        &env_from(&[(
            "MODDE_DATABASE_URL",
            "postgres://env_user@env-host:6543/env_db",
        )]),
    )
    .unwrap();

    assert_eq!(opts.get_host(), "env-host");
    assert_eq!(opts.get_port(), 6543);
    assert_eq!(opts.get_database(), Some("env_db"));
    assert_eq!(opts.get_username(), "env_user");
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_rejects_bad_env_port() {
    let err = build_pg_options(
        &DatabaseSettings {
            dbname: Some("settings_db".to_string()),
            ..DatabaseSettings::default()
        },
        &env_from(&[("MODDE_DATABASE_PORT", "not-a-port")]),
    )
    .unwrap_err();

    let message = err.to_string();
    assert!(message.contains("invalid MODDE_DATABASE_PORT value 'not-a-port'"));
    assert!(!message.contains("settings_db"));
}

#[cfg(feature = "postgres")]
#[test]
fn build_pg_options_rejects_out_of_range_env_port() {
    let err = build_pg_options(
        &DatabaseSettings {
            dbname: Some("settings_db".to_string()),
            ..DatabaseSettings::default()
        },
        &env_from(&[("MODDE_DATABASE_PORT", "70000")]),
    )
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("invalid MODDE_DATABASE_PORT value '70000'")
    );
}

#[cfg(feature = "postgres")]
#[test]
fn read_pg_password_file_trims_trailing_newline() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("pg-password");
    std::fs::write(&path, "secret-password\n").unwrap();

    let password = read_pg_password_file(&path).unwrap();

    assert_eq!(password, "secret-password");
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[serial]
async fn open_with_settings_honors_env_backend_override() {
    let tmp = tempfile::tempdir().unwrap();
    let _backend = EnvRestore::set("MODDE_DATABASE_BACKEND", "sqlite");
    let _url = EnvRestore::set(
        "MODDE_DATABASE_URL",
        "postgres://env_user@127.0.0.1:1/env_db",
    );
    let data_home = tmp.path().join("data");
    let config_home = tmp.path().join("config");
    let _xdg_data = EnvRestore::set("XDG_DATA_HOME", data_home.to_str().unwrap());
    let _xdg_config = EnvRestore::set("XDG_CONFIG_HOME", config_home.to_str().unwrap());

    let settings = AppSettings {
        database: DatabaseSettings {
            backend: DbBackend::Postgres,
            url: Some("postgres://settings_user@127.0.0.1:1/settings_db".to_string()),
            ..DatabaseSettings::default()
        },
        ..AppSettings::default()
    };

    let db = ModdeDb::open_with_settings(&settings).await.unwrap();

    db.ping().await.unwrap();
    assert!(data_home.join("modde/modde.db").exists());
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

#[tokio::test]
async fn create_and_load_profile() {
    let db = test_db().await;
    let profile = sample_profile("test", "skyrim-se");

    let id = db.create_profile(&profile).await.unwrap();
    assert!(id > 0);

    let loaded = db
        .load_profile("test", &GameId::from("skyrim-se"))
        .await
        .unwrap();
    assert_eq!(loaded.name, "test");
    assert_eq!(loaded.game_id, "skyrim-se");
    assert_eq!(loaded.mods.len(), 2);
    assert_eq!(loaded.mods[0].mod_id, "mod_a");
    assert!(loaded.mods[0].enabled);
    assert_eq!(loaded.mods[1].mod_id, "mod_b");
    assert!(!loaded.mods[1].enabled);
    assert_eq!(loaded.load_order_rules.len(), 1);
}

#[tokio::test]
async fn nexus_ids_roundtrip_with_unchanged_stored_values() {
    let db = test_db().await;
    let mut profile = sample_profile("test", "skyrim-se");
    profile.mods[0].nexus_mod_id = Some(NexusModId::from(42));
    profile.mods[0].nexus_file_id = Some(NexusFileId::from(99));

    db.create_profile(&profile).await.unwrap();

    let stored = db
        .test_two_i64(
            "SELECT nexus_mod_id, nexus_file_id FROM profile_mods WHERE mod_id = 'mod_a'",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(stored, (42, 99));

    let loaded = db
        .load_profile("test", &GameId::from("skyrim-se"))
        .await
        .unwrap();
    assert_eq!(loaded.mods[0].nexus_mod_id, Some(NexusModId::from(42)));
    assert_eq!(loaded.mods[0].nexus_file_id, Some(NexusFileId::from(99)));
}

#[tokio::test]
async fn negative_nexus_ids_fail_closed_on_load() {
    let db = test_db().await;
    let profile = sample_profile("test", "skyrim-se");
    db.create_profile(&profile).await.unwrap();
    db.test_exec(
        "UPDATE profile_mods SET nexus_mod_id = -1 WHERE mod_id = 'mod_a'",
        &[],
    )
    .await
    .unwrap();

    let err = db
        .load_profile("test", &GameId::from("skyrim-se"))
        .await
        .unwrap_err();
    // The negative id now surfaces as a typed Nexus-id error (was wrapped as a
    // rusqlite conversion failure under the old backend).
    assert!(matches!(err, CoreError::NexusId(_)));
}

#[tokio::test]
async fn legacy_installer_metadata_loads_typed_and_roundtrips_storage() {
    let db = test_db().await;
    let profile = sample_profile("test", "skyrim-se");
    db.create_profile(&profile).await.unwrap();

    let method_raw = encode_install_method(&InstallMethod::BareExtract).unwrap();
    let tags_raw = r#"["quest","ui"]"#;
    db.test_exec(
        "UPDATE profile_mods
            SET install_status = ?, install_method = ?, tags = ?
          WHERE mod_id = 'mod_a'",
        &vals!["pending_user_input", method_raw.clone(), tags_raw],
    )
    .await
    .unwrap();

    let loaded = db
        .load_profile("test", &GameId::from("skyrim-se"))
        .await
        .unwrap();
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

    db.update_profile(&loaded).await.unwrap();
    let stored = db
        .test_three_str(
            "SELECT install_status, install_method, tags
               FROM profile_mods
              WHERE mod_id = 'mod_a'",
            &[],
        )
        .await
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

#[tokio::test]
async fn load_by_name_unique() {
    let db = test_db().await;
    let profile = sample_profile("default", "skyrim-se");
    db.create_profile(&profile).await.unwrap();

    let loaded = db.load_profile_by_name("default").await.unwrap();
    assert_eq!(loaded.game_id, "skyrim-se");
}

#[tokio::test]
async fn load_by_name_ambiguous() {
    let db = test_db().await;
    db.create_profile(&sample_profile("default", "skyrim-se"))
        .await
        .unwrap();
    db.create_profile(&sample_profile("default", "fallout4"))
        .await
        .unwrap();

    let err = db.load_profile_by_name("default").await.unwrap_err();
    match err {
        CoreError::AmbiguousProfile { name, games } => {
            assert_eq!(name, "default");
            assert!(games.contains(&GameId::from("skyrim-se")));
            assert!(games.contains(&GameId::from("fallout4")));
        }
        other => panic!("expected AmbiguousProfile, got: {other}"),
    }
}

#[tokio::test]
async fn multi_profile_per_game() {
    let db = test_db().await;
    db.create_profile(&sample_profile("vanilla", "skyrim-se"))
        .await
        .unwrap();
    db.create_profile(&sample_profile("modded", "skyrim-se"))
        .await
        .unwrap();
    db.create_profile(&sample_profile("hardcore", "skyrim-se"))
        .await
        .unwrap();

    let profiles = db
        .list_profiles(Some(&GameId::from("skyrim-se")))
        .await
        .unwrap();
    assert_eq!(profiles.len(), 3);
}

#[tokio::test]
async fn update_profile() {
    let db = test_db().await;
    let mut profile = sample_profile("test", "skyrim-se");
    db.create_profile(&profile).await.unwrap();

    profile.mods.push(EnabledMod {
        mod_id: "mod_c".to_string(),
        enabled: true,
        version: None,
        fomod_config: None,
        ..Default::default()
    });

    db.update_profile(&profile).await.unwrap();

    let loaded = db
        .load_profile("test", &GameId::from("skyrim-se"))
        .await
        .unwrap();
    assert_eq!(loaded.mods.len(), 3);
}

#[tokio::test]
async fn delete_profile() {
    let db = test_db().await;
    db.create_profile(&sample_profile("test", "skyrim-se"))
        .await
        .unwrap();
    db.delete_profile("test", &GameId::from("skyrim-se"))
        .await
        .unwrap();

    let err = db
        .load_profile("test", &GameId::from("skyrim-se"))
        .await
        .unwrap_err();
    assert!(matches!(err, CoreError::ProfileNotFound(_)));
}

#[tokio::test]
async fn delete_cascades_to_mods_and_saves() {
    let db = test_db().await;
    let id = db
        .create_profile(&sample_profile("test", "skyrim-se"))
        .await
        .unwrap();
    db.assign_save(id, Path::new("/saves/save1.ess"), Some("my save"))
        .await
        .unwrap();

    let saves = db.list_saves(id).await.unwrap();
    assert_eq!(saves.len(), 1);

    db.delete_profile("test", &GameId::from("skyrim-se"))
        .await
        .unwrap();

    let saves = db.list_saves(id).await.unwrap();
    assert_eq!(saves.len(), 0);
}

#[tokio::test]
async fn save_assignment() {
    let db = test_db().await;
    let id = db
        .create_profile(&sample_profile("test", "skyrim-se"))
        .await
        .unwrap();

    db.assign_save(id, Path::new("/saves/save1.ess"), Some("Level 50"))
        .await
        .unwrap();
    db.assign_save(id, Path::new("/saves/save2.ess"), None)
        .await
        .unwrap();

    let saves = db.list_saves(id).await.unwrap();
    assert_eq!(saves.len(), 2);
    assert_eq!(saves[0].label.as_deref(), Some("Level 50"));
    assert!(saves[1].label.is_none());

    db.unassign_save(Path::new("/saves/save1.ess"))
        .await
        .unwrap();
    let saves = db.list_saves(id).await.unwrap();
    assert_eq!(saves.len(), 1);
}

#[tokio::test]
async fn save_already_assigned_to_different_profile() {
    let db = test_db().await;
    let id1 = db
        .create_profile(&sample_profile("profile1", "skyrim-se"))
        .await
        .unwrap();
    let id2 = db
        .create_profile(&sample_profile("profile2", "skyrim-se"))
        .await
        .unwrap();

    db.assign_save(id1, Path::new("/saves/save1.ess"), None)
        .await
        .unwrap();

    let err = db
        .assign_save(id2, Path::new("/saves/save1.ess"), None)
        .await
        .unwrap_err();
    assert!(matches!(err, CoreError::SaveAlreadyAssigned { .. }));
}

#[tokio::test]
async fn snapshot_upsert_and_get() {
    let db = test_db().await;

    db.upsert_snapshot(
        &GameId::from("skyrim-se"),
        Path::new("/stock/skyrim-se"),
        "abc123",
        5000,
    )
    .await
    .unwrap();
    let meta = db
        .get_snapshot(&GameId::from("skyrim-se"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(meta.tree_hash, "abc123");
    assert_eq!(meta.file_count, 5000);

    db.upsert_snapshot(
        &GameId::from("skyrim-se"),
        Path::new("/stock/skyrim-se"),
        "def456",
        5001,
    )
    .await
    .unwrap();
    let meta = db
        .get_snapshot(&GameId::from("skyrim-se"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(meta.tree_hash, "def456");
    assert_eq!(meta.file_count, 5001);
}

#[tokio::test]
async fn snapshot_not_found() {
    let db = test_db().await;
    assert!(
        db.get_snapshot(&GameId::from("nonexistent"))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn list_profiles_all_and_by_game() {
    let db = test_db().await;
    db.create_profile(&sample_profile("vanilla", "skyrim-se"))
        .await
        .unwrap();
    db.create_profile(&sample_profile("modded", "skyrim-se"))
        .await
        .unwrap();
    db.create_profile(&sample_profile("default", "fallout4"))
        .await
        .unwrap();

    let all = db.list_profiles(None).await.unwrap();
    assert_eq!(all.len(), 3);

    let skyrim = db
        .list_profiles(Some(&GameId::from("skyrim-se")))
        .await
        .unwrap();
    assert_eq!(skyrim.len(), 2);

    let fallout = db
        .list_profiles(Some(&GameId::from("fallout4")))
        .await
        .unwrap();
    assert_eq!(fallout.len(), 1);
}

#[tokio::test]
async fn source_roundtrip_nexus_collection() {
    let db = test_db().await;
    let mut profile = sample_profile("test", "skyrim-se");
    profile.source = ProfileSource::NexusCollection {
        slug: "my-collection".to_string(),
        version: "1.2.3".to_string(),
    };

    db.create_profile(&profile).await.unwrap();
    let loaded = db
        .load_profile("test", &GameId::from("skyrim-se"))
        .await
        .unwrap();

    match loaded.source {
        ProfileSource::NexusCollection { slug, version } => {
            assert_eq!(slug, "my-collection");
            assert_eq!(version, "1.2.3");
        }
        other => panic!("expected NexusCollection, got: {other:?}"),
    }
}

#[tokio::test]
async fn source_roundtrip_wabbajack() {
    let db = test_db().await;
    let mut profile = sample_profile("test", "skyrim-se");
    profile.source = ProfileSource::Wabbajack {
        manifest_hash: "deadbeef".to_string(),
    };

    db.create_profile(&profile).await.unwrap();
    let loaded = db
        .load_profile("test", &GameId::from("skyrim-se"))
        .await
        .unwrap();

    match loaded.source {
        ProfileSource::Wabbajack { manifest_hash } => {
            assert_eq!(manifest_hash, "deadbeef");
        }
        other => panic!("expected Wabbajack, got: {other:?}"),
    }
}

#[tokio::test]
async fn profile_not_found() {
    let db = test_db().await;
    let err = db
        .load_profile("nonexistent", &GameId::from("skyrim-se"))
        .await
        .unwrap_err();
    assert!(matches!(err, CoreError::ProfileNotFound(_)));
}

#[tokio::test]
async fn duplicate_profile_errors() {
    let db = test_db().await;
    db.create_profile(&sample_profile("test", "skyrim-se"))
        .await
        .unwrap();

    let err = db
        .create_profile(&sample_profile("test", "skyrim-se"))
        .await
        .unwrap_err();
    assert!(matches!(err, CoreError::Database(_)));
}

#[tokio::test]
async fn executable_config_roundtrip() {
    let db = test_db().await;
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

    db.save_executable_config(&row).await.unwrap();
    let loaded = db
        .load_executable_config(&GameId::from("skyrim-se"), "xEdit")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded, row);

    let all = db
        .load_executable_configs(&GameId::from("skyrim-se"))
        .await
        .unwrap();
    assert_eq!(all.len(), 1);
    assert!(
        db.delete_executable_config(&GameId::from("skyrim-se"), "xEdit")
            .await
            .unwrap()
    );
    assert!(
        db.load_executable_config(&GameId::from("skyrim-se"), "xEdit")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn patcher_stage_roundtrip_and_manifest_replace() {
    let db = test_db().await;
    let profile = sample_profile("patchers", "skyrim-se");
    db.create_profile(&profile).await.unwrap();
    let loaded = db
        .load_profile("patchers", &GameId::from("skyrim-se"))
        .await
        .unwrap();
    let profile_id = loaded.id.unwrap();

    let stage = PatcherStageRow::new(
        profile_id,
        "synth",
        true,
        4,
        PatcherStageSettings::Command(CommandSettings {
            executable: PathBuf::from("/bin/true"),
            args: vec!["--flag".to_string()],
            environment: HashMap::new(),
            working_dir: Some(PathBuf::from("/tmp")),
        }),
        "generated-synth",
    )
    .unwrap();
    db.save_patcher_stage(&stage).await.unwrap();

    let stages = db.list_patcher_stages(profile_id).await.unwrap();
    assert_eq!(stages.len(), 1);
    assert_eq!(stages[0], stage);

    db.replace_patcher_stage_outputs(
        profile_id,
        "synth",
        &[
            "Meshes/a.nif".to_string(),
            "Plugins/Synthesis.esp".to_string(),
        ],
    )
    .await
    .unwrap();
    let outputs = db
        .list_patcher_stage_outputs(profile_id, "synth")
        .await
        .unwrap();
    let rels: Vec<_> = outputs.into_iter().map(|row| row.rel_path).collect();
    assert_eq!(rels, vec!["Meshes/a.nif", "Plugins/Synthesis.esp"]);

    db.replace_patcher_stage_outputs(profile_id, "synth", &["Only/new.esp".to_string()])
        .await
        .unwrap();
    let outputs = db
        .list_patcher_stage_outputs(profile_id, "synth")
        .await
        .unwrap();
    let rels: Vec<_> = outputs.into_iter().map(|row| row.rel_path).collect();
    assert_eq!(rels, vec!["Only/new.esp"]);
}

#[tokio::test]
async fn patcher_stage_duplicate_output_mod_is_rejected() {
    let db = test_db().await;
    let profile = sample_profile("patchers", "skyrim-se");
    db.create_profile(&profile).await.unwrap();
    let loaded = db
        .load_profile("patchers", &GameId::from("skyrim-se"))
        .await
        .unwrap();
    let profile_id = loaded.id.unwrap();

    let first = PatcherStageRow::new(
        profile_id,
        "first",
        true,
        0,
        PatcherStageSettings::Command(CommandSettings {
            executable: PathBuf::from("/bin/true"),
            args: Vec::new(),
            environment: HashMap::new(),
            working_dir: None,
        }),
        "shared-output",
    )
    .unwrap();
    db.save_patcher_stage(&first).await.unwrap();

    let second = PatcherStageRow::new(
        profile_id,
        "second",
        true,
        1,
        PatcherStageSettings::Command(CommandSettings {
            executable: PathBuf::from("/bin/true"),
            args: Vec::new(),
            environment: HashMap::new(),
            working_dir: None,
        }),
        "shared-output",
    )
    .unwrap();
    let err = db.save_patcher_stage(&second).await.unwrap_err();
    assert!(matches!(err, CoreError::Validation(_)));
    assert!(err.to_string().contains("already used"));
}
