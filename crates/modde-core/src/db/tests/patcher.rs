use super::*;

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
