use super::backend::vals;
use super::*;
use std::path::Path;

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
