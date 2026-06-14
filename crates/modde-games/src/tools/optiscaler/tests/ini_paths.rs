use super::*;
use crate::tools::optiscaler::config::{optiscaler_config_reset_reason, set_ini_value};

#[test]
fn incompatible_existing_ini_triggers_reset_reason() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let new_ini = tmp.path().join("new.ini");
    std::fs::write(&new_ini, "[OptiScaler]\nDxgi=auto\n").expect("new ini");
    let state = OptiScalerInstallState {
        status: OptiScalerInstallStatus::Unmanaged,
        executable_dir: tmp.path().to_path_buf(),
        proxy_dlls: vec![],
        wine_dll_overrides: vec![],
        config_path: None,
        ini_settings: BTreeMap::from([("Removed.SectionKey".to_string(), "true".to_string())]),
        companion_files: vec![],
        recognized_files: vec![],
        version: OptiScalerVersionIdentity::Unknown,
        latest_backup: None,
    };
    let config = OptiScaler.default_config();
    assert_eq!(
        optiscaler_config_reset_reason(&state, &new_ini, &config).as_deref(),
        Some("schema mismatch")
    );
}

#[test]
fn set_ini_value_updates_existing_key_in_section() {
    let content = "[OptiScaler]\nDxgi=auto\nLoadAsiPlugins=false\n";
    let updated = set_ini_value(content, "OptiScaler.Dxgi", "manual");
    assert!(updated.contains("Dxgi=manual"));
    assert!(updated.contains("LoadAsiPlugins=false"));
}

#[test]
fn set_ini_value_appends_section_when_missing() {
    let updated = set_ini_value("", "Menu.Scale", "1.25");
    assert!(updated.contains("[Menu]"));
    assert!(updated.contains("Scale=1.25"));
}

#[test]
fn set_ini_value_returns_content_unchanged_when_key_has_no_section() {
    let content = "[OptiScaler]\nDxgi=auto\n";
    let updated = set_ini_value(content, "no_section_prefix", "x");
    assert_eq!(updated, content, "malformed key should not mutate content");
}

// ── relative_to_game ──────────────────────────────────────────────

#[test]
fn relative_to_game_strips_game_prefix() {
    let game = PathBuf::from("/games/skyrim");
    let dest = PathBuf::from("/games/skyrim/Data/SKSE/plugins/foo.dll");
    let rel = relative_to_game(&game, &dest).expect("strips prefix");
    assert_eq!(rel, PathBuf::from("Data/SKSE/plugins/foo.dll"));
}

#[test]
fn relative_to_game_errors_when_dest_outside_game_dir() {
    let game = PathBuf::from("/games/skyrim");
    let dest = PathBuf::from("/elsewhere/foo.dll");
    let err = relative_to_game(&game, &dest).expect_err("must fail");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("/elsewhere/foo.dll") && msg.contains("/games/skyrim"),
        "error must mention both paths: {msg}"
    );
}
