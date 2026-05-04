use modde_core::db::ModdeDb;

#[test]
fn tool_config_save_creates_history_nodes_and_edges() {
    let db = ModdeDb::open_memory().expect("db");

    db.save_tool_config_with_reason("skyrim-se", "mangohud", true, r#"{"fps":true}"#, "enable")
        .expect("first save");
    db.save_tool_config_with_reason("skyrim-se", "mangohud", true, r#"{"fps":false}"#, "set:fps")
        .expect("second save");

    let history = db
        .list_tool_setting_history("skyrim-se", "mangohud", 10)
        .expect("history");
    assert_eq!(history.len(), 2);
    assert!(history[0].is_current);
    assert_eq!(history[0].settings_json, r#"{"fps":false}"#);
    assert!(!history[1].is_current);

    let edges = db
        .list_tool_setting_edges("skyrim-se", "mangohud")
        .expect("edges");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].parent_node_id, history[1].node_id);
    assert_eq!(edges[0].child_node_id, history[0].node_id);
}

#[test]
fn restoring_old_tool_settings_creates_branch_node() {
    let db = ModdeDb::open_memory().expect("db");

    db.save_tool_config_with_reason(
        "stellar-blade",
        "optiscaler",
        true,
        r#"{"proxy":"dxgi"}"#,
        "a",
    )
    .expect("first save");
    db.save_tool_config_with_reason(
        "stellar-blade",
        "optiscaler",
        true,
        r#"{"proxy":"winmm"}"#,
        "b",
    )
    .expect("second save");
    let old_node = db
        .list_tool_setting_history("stellar-blade", "optiscaler", 10)
        .expect("history")
        .into_iter()
        .find(|node| node.reason == "a")
        .expect("old node");

    db.restore_tool_setting_node("stellar-blade", "optiscaler", &old_node.node_id)
        .expect("restore");

    let current = db
        .load_tool_config("stellar-blade", "optiscaler")
        .expect("load")
        .expect("row");
    assert_eq!(current.settings_json, r#"{"proxy":"dxgi"}"#);

    let history = db
        .list_tool_setting_history("stellar-blade", "optiscaler", 10)
        .expect("history");
    assert_eq!(history.len(), 3);
    assert!(history[0].is_current);
    assert_eq!(history[0].reason, format!("restore:{}", old_node.node_id));

    let edges = db
        .list_tool_setting_edges("stellar-blade", "optiscaler")
        .expect("edges");
    assert_eq!(edges.len(), 2);
    assert!(
        edges
            .iter()
            .any(|edge| edge.parent_node_id == history[1].node_id
                && edge.child_node_id == history[0].node_id)
    );
}

#[test]
fn schema_v10_migration_is_idempotent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("modde.sqlite");
    {
        let db = ModdeDb::open_at(&path).expect("first open");
        db.save_tool_config("skyrim-se", "vkbasalt", false, "{}")
            .expect("save");
    }
    {
        let db = ModdeDb::open_at(&path).expect("second open");
        let history = db
            .list_tool_setting_history("skyrim-se", "vkbasalt", 10)
            .expect("history");
        assert_eq!(history.len(), 1);
        assert!(history[0].is_current);
    }
}
