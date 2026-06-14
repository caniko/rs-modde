use super::*;

#[test]
fn stellar_blade_default_config_adds_community_metadata_only() {
    let context = ToolGameContext::from_parts(
        "stellar-blade",
        "Stellar Blade",
        Some(PathBuf::from("/fake/StellarBlade")),
        None,
    );
    let config = OptiScaler.default_config_for(Some(&context));

    assert_eq!(
        config.get_str("source_mode"),
        Some(OPTISCALER_SOURCE_GOVERLAY_FGMOD)
    );
    assert_eq!(config.get_str("goverlay_channel"), Some("edge"));
    assert_eq!(config.get_str("release_tag"), Some("latest"));
    assert_eq!(config.get_str("release_asset"), Some(""));
    assert_eq!(config.get_str("proxy_dll"), Some("dxgi.dll"));
    assert_eq!(config.get_str("dll_overrides"), Some(""));
    assert!(config.get_bool("copy_companion_files"));
    assert!(!config.get_bool("enable_optipatcher"));
    assert_eq!(
        config.get_str("fsr4_variant"),
        Some(FSR4_VARIANT_LATEST_FP8)
    );
    assert!(!config.get_bool("emulate_fp8"));
    assert!(!config.get_bool("spoof_dlss"));
    assert_eq!(config.get_str("optiscaler_profile"), Some("community-dxgi"));
    assert_eq!(config.get_str("tested_optiscaler_version"), Some("0.9"));
    assert_eq!(
        config.get_str("optiscaler_profile_source_url"),
        Some("https://github.com/optiscaler/OptiScaler/wiki/Stellar-Blade")
    );
    assert_eq!(
        config.settings.get("ini_overrides"),
        Some(&serde_json::json!({}))
    );
}

#[test]
fn games_without_optiscaler_profiles_keep_generic_defaults() {
    let context = ToolGameContext::from_parts("skyrim-se", "Skyrim Special Edition", None, None);
    let config = OptiScaler.default_config_for(Some(&context));

    assert_eq!(config.get_str("source_mode"), Some("goverlay_fgmod"));
    assert_eq!(config.get_str("release_tag"), Some("latest"));
    assert_eq!(config.get_str("proxy_dll"), Some("dxgi.dll"));
    assert_eq!(config.get_str("dll_overrides"), Some(""));
    assert_eq!(config.get_str("optiscaler_profile"), None);
}

#[test]
fn custom_profile_opt_out_prevents_community_defaults() {
    let context = ToolGameContext::from_parts(
        "stellar-blade",
        "Stellar Blade",
        Some(PathBuf::from("/fake/StellarBlade")),
        None,
    );
    let mut config = OptiScaler.default_config();
    config.set("optiscaler_profile", serde_json::json!("custom"));
    config.set("source_mode", serde_json::json!("local_dir"));
    config.set("release_tag", serde_json::json!("latest"));
    config.set("proxy_dll", serde_json::json!("winmm.dll"));

    apply_game_defaults(&mut config, Some(&context));

    assert_eq!(config.get_str("optiscaler_profile"), Some("custom"));
    assert_eq!(config.get_str("source_mode"), Some("local_dir"));
    assert_eq!(config.get_str("release_tag"), Some("latest"));
    assert_eq!(config.get_str("proxy_dll"), Some("winmm.dll"));
    assert_eq!(config.get_str("tested_optiscaler_version"), None);
}

#[test]
fn stellar_blade_game_defaults_preserve_custom_settings_for_profile() {
    let context = ToolGameContext::from_parts(
        "stellar-blade",
        "Stellar Blade",
        Some(PathBuf::from("/fake/StellarBlade")),
        None,
    );
    let mut config = OptiScaler.default_config();
    config.set("optiscaler_profile", serde_json::json!("community-dxgi"));
    config.set("source_mode", serde_json::json!("goverlay_builds"));
    config.set("goverlay_channel", serde_json::json!("edge"));
    config.set(
        "release_tag",
        serde_json::json!("goverlay-edge:edge-0.9.12.0323"),
    );
    config.set("release_asset", serde_json::json!("optiscaler-edge.7z"));
    config.set("proxy_dll", serde_json::json!("winmm.dll"));
    config.set("dll_overrides", serde_json::json!("winmm,nvngx"));
    config.set("copy_companion_files", serde_json::json!(false));
    config.set("enable_optipatcher", serde_json::json!(false));
    config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));
    config.set("emulate_fp8", serde_json::json!(true));
    config.set("spoof_dlss", serde_json::json!(true));
    config.set(
        "ini_overrides",
        serde_json::json!({"Spoofing": {"Dxgi": "false"}}),
    );

    apply_game_defaults(&mut config, Some(&context));

    assert_eq!(config.get_str("optiscaler_profile"), Some("community-dxgi"));
    assert_eq!(config.get_str("source_mode"), Some("goverlay_builds"));
    assert_eq!(config.get_str("goverlay_channel"), Some("edge"));
    assert_eq!(
        config.get_str("release_tag"),
        Some("goverlay-edge:edge-0.9.12.0323")
    );
    assert_eq!(config.get_str("release_asset"), Some("optiscaler-edge.7z"));
    assert_eq!(config.get_str("proxy_dll"), Some("winmm.dll"));
    assert_eq!(config.get_str("dll_overrides"), Some("winmm,nvngx"));
    assert!(!config.get_bool("copy_companion_files"));
    assert!(!config.get_bool("enable_optipatcher"));
    assert_eq!(config.get_str("fsr4_variant"), Some(FSR4_VARIANT_INT8_402));
    assert!(config.get_bool("emulate_fp8"));
    assert!(config.get_bool("spoof_dlss"));
    assert_eq!(
        config
            .settings
            .pointer("/ini_overrides/Spoofing/Dxgi")
            .and_then(serde_json::Value::as_str),
        Some("false")
    );
    assert_eq!(config.get_str("tested_optiscaler_version"), Some("0.9"));
}

#[test]
fn stellar_blade_game_defaults_preserve_existing_release_without_profile_marker() {
    let context = ToolGameContext::from_parts(
        "stellar-blade",
        "Stellar Blade",
        Some(PathBuf::from("/fake/StellarBlade")),
        None,
    );
    let mut config = OptiScaler.default_config();
    config.set("source_mode", serde_json::json!("github_release"));
    config.set("release_tag", serde_json::json!("official:v0.9.11"));
    config.set("release_asset", serde_json::json!("OptiScaler_0.9.11.7z"));

    apply_game_defaults(&mut config, Some(&context));

    assert_eq!(config.get_str("optiscaler_profile"), Some("community-dxgi"));
    assert_eq!(config.get_str("source_mode"), Some("github_release"));
    assert_eq!(config.get_str("release_tag"), Some("official:v0.9.11"));
    assert_eq!(
        config.get_str("release_asset"),
        Some("OptiScaler_0.9.11.7z")
    );
    assert_eq!(config.get_str("proxy_dll"), Some("dxgi.dll"));
    assert!(!config.get_bool("enable_optipatcher"));
}

#[test]
fn selecting_profile_after_custom_applies_metadata_only() {
    let mut config = OptiScaler.default_config();
    assert!(apply_profile_by_id(&mut config, "stellar-blade", "custom"));
    assert_eq!(config.get_str("optiscaler_profile"), Some("custom"));
    config.set("source_mode", serde_json::json!("local_dir"));
    config.set("release_tag", serde_json::json!("custom-build"));
    config.set("release_asset", serde_json::json!("CustomOptiScaler.7z"));
    config.set("proxy_dll", serde_json::json!("winmm.dll"));
    config.set("dll_overrides", serde_json::json!("winmm,nvngx"));
    config.set("copy_companion_files", serde_json::json!(false));
    config.set("enable_optipatcher", serde_json::json!(false));
    config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));
    config.set("emulate_fp8", serde_json::json!(true));
    config.set("spoof_dlss", serde_json::json!(true));

    assert!(apply_profile_by_id(
        &mut config,
        "stellar-blade",
        "community-dxgi"
    ));

    assert_eq!(config.get_str("optiscaler_profile"), Some("community-dxgi"));
    assert_eq!(config.get_str("source_mode"), Some("local_dir"));
    assert_eq!(config.get_str("goverlay_channel"), Some("edge"));
    assert_eq!(config.get_str("release_tag"), Some("custom-build"));
    assert_eq!(config.get_str("release_asset"), Some("CustomOptiScaler.7z"));
    assert_eq!(config.get_str("proxy_dll"), Some("winmm.dll"));
    assert_eq!(config.get_str("dll_overrides"), Some("winmm,nvngx"));
    assert!(!config.get_bool("copy_companion_files"));
    assert!(!config.get_bool("enable_optipatcher"));
    assert_eq!(config.get_str("fsr4_variant"), Some(FSR4_VARIANT_INT8_402));
    assert!(config.get_bool("emulate_fp8"));
    assert!(config.get_bool("spoof_dlss"));
    assert_eq!(config.get_str("tested_optiscaler_version"), Some("0.9"));
}

#[test]
fn optiscaler_profile_metadata_does_not_change_settings() {
    let profile = crate::optiscaler::OptiScalerProfile {
        id: "test-profile",
        name: "Test Profile",
        source_url: "https://example.test/profile",
        tested_optiscaler_version: "1.2.3",
        source_mode: None,
        goverlay_channel: None,
        proxy_dll: "winmm.dll",
        release_tag: Some("v1.2.3"),
        release_asset: Some("OptiScaler.7z"),
        wine_dll_overrides: &["winmm", "nvngx"],
        copy_companion_files: false,
        enable_optipatcher: true,
        fsr4_variant: Some(FSR4_VARIANT_INT8_402),
        emulate_fp8: true,
        spoof_dlss: true,
        ini_overrides: &[crate::optiscaler::OptiScalerIniOverride {
            key: "Spoofing.Dxgi",
            value: "false",
        }],
        notes: "Test notes",
    };
    let mut config = OptiScaler.default_config();
    config.set("source_mode", serde_json::json!("local_dir"));
    config.set("release_tag", serde_json::json!("custom-build"));
    config.set("release_asset", serde_json::json!("CustomOptiScaler.7z"));
    config.set("proxy_dll", serde_json::json!("dxgi.dll"));
    config.set("dll_overrides", serde_json::json!("dxgi"));
    config.set("copy_companion_files", serde_json::json!(true));
    config.set("enable_optipatcher", serde_json::json!(false));
    config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_LATEST_FP8));
    config.set("emulate_fp8", serde_json::json!(false));
    config.set("spoof_dlss", serde_json::json!(false));
    config.set(
        "ini_overrides",
        serde_json::json!({"OptiScaler": {"Dxgi": "auto"}}),
    );

    apply_optiscaler_profile_metadata(&mut config, &profile);

    assert_eq!(config.get_str("optiscaler_profile"), Some("test-profile"));
    assert_eq!(config.get_str("source_mode"), Some("local_dir"));
    assert_eq!(config.get_str("release_tag"), Some("custom-build"));
    assert_eq!(config.get_str("release_asset"), Some("CustomOptiScaler.7z"));
    assert_eq!(config.get_str("proxy_dll"), Some("dxgi.dll"));
    assert_eq!(config.get_str("dll_overrides"), Some("dxgi"));
    assert!(config.get_bool("copy_companion_files"));
    assert!(!config.get_bool("enable_optipatcher"));
    assert_eq!(
        config.get_str("fsr4_variant"),
        Some(FSR4_VARIANT_LATEST_FP8)
    );
    assert!(!config.get_bool("emulate_fp8"));
    assert!(!config.get_bool("spoof_dlss"));
    assert_eq!(
        config
            .settings
            .pointer("/ini_overrides/OptiScaler/Dxgi")
            .and_then(serde_json::Value::as_str),
        Some("auto")
    );
    assert_eq!(config.get_str("tested_optiscaler_version"), Some("1.2.3"));
    assert_eq!(
        config.get_str("optiscaler_profile_source_url"),
        Some("https://example.test/profile")
    );
}

#[test]
fn optiscaler_release_selection_preserves_custom_profile_deployment() {
    let mut config = OptiScaler.default_config();
    config.set("optiscaler_profile", serde_json::json!("community-dxgi"));
    config.set("proxy_dll", serde_json::json!("winmm.dll"));
    config.set("dll_overrides", serde_json::json!("winmm,nvngx"));
    config.set("copy_companion_files", serde_json::json!(false));
    config.set("enable_optipatcher", serde_json::json!(false));
    config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));
    config.set("emulate_fp8", serde_json::json!(true));
    config.set("spoof_dlss", serde_json::json!(true));
    config.set(
        "ini_overrides",
        serde_json::json!({"Spoofing": {"Dxgi": "false"}}),
    );

    apply_optiscaler_release_selection(&mut config, "official:v0.9.11", "OptiScaler_0.9.11.7z");

    assert_eq!(config.get_str("optiscaler_profile"), Some("community-dxgi"));
    assert_eq!(config.get_str("source_mode"), Some("github_release"));
    assert_eq!(config.get_str("release_tag"), Some("official:v0.9.11"));
    assert_eq!(
        config.get_str("release_asset"),
        Some("OptiScaler_0.9.11.7z")
    );
    assert_eq!(config.get_str("proxy_dll"), Some("winmm.dll"));
    assert_eq!(config.get_str("dll_overrides"), Some("winmm,nvngx"));
    assert!(!config.get_bool("copy_companion_files"));
    assert!(!config.get_bool("enable_optipatcher"));
    assert_eq!(config.get_str("fsr4_variant"), Some(FSR4_VARIANT_INT8_402));
    assert!(config.get_bool("emulate_fp8"));
    assert!(config.get_bool("spoof_dlss"));
    assert_eq!(
        config
            .settings
            .pointer("/ini_overrides/Spoofing/Dxgi")
            .and_then(serde_json::Value::as_str),
        Some("false")
    );
}
