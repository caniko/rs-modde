#![allow(clippy::wildcard_imports)]
use super::*;

use std::path::PathBuf;

pub(crate) async fn save_tool_setting_for_game(
    db: modde_core::db::ModdeDb,
    game_id: String,
    tool_id: String,
    key: String,
    value: serde_json::Value,
    context: Option<modde_games::tools::ToolGameContext>,
    optiscaler_releases: Vec<modde_games::tools::ToolReleaseSummary>,
    option_catalog: ToolOptionCatalog,
) -> Result<ToolSettingWriteResult, String> {
    tokio::task::spawn_blocking(move || {
        save_tool_setting_for_game_blocking(
            db,
            game_id,
            tool_id,
            key,
            value,
            context,
            optiscaler_releases,
            option_catalog,
        )
    })
    .await
    .map_err(|err| err.to_string())?
}

#[allow(clippy::too_many_arguments)]
fn save_tool_setting_for_game_blocking(
    db: modde_core::db::ModdeDb,
    game_id: String,
    tool_id: String,
    key: String,
    value: serde_json::Value,
    context: Option<modde_games::tools::ToolGameContext>,
    optiscaler_releases: Vec<modde_games::tools::ToolReleaseSummary>,
    mut option_catalog: ToolOptionCatalog,
) -> Result<ToolSettingWriteResult, String> {
    let tool = modde_games::tools::resolve_tool(&tool_id)
        .ok_or_else(|| format!("Unknown tool: {tool_id}"))?;
    let typed_game_id = GameId::from(game_id.as_str());
    let mut config =
        load_tool_config_or_default_blocking(&db, &typed_game_id, tool, context.as_ref())?;
    if tool_id == "optiscaler" {
        let _ = modde_games::tools::optiscaler::normalize_optiscaler_release_config(&mut config);
    }
    let setting_specs = tool.settings_schema_for(context.as_ref(), &config);
    let normalized = if let Some(spec) = setting_specs.iter().find(|spec| spec.key == key) {
        normalize_tool_setting_value(
            &config.settings,
            &key,
            normalize_tool_setting_for_kind(value, &spec.kind),
        )
    } else {
        normalize_tool_setting_value(&config.settings, &key, value)
    };
    set_nested_tool_setting(&mut config.settings, &key, normalized);
    if tool_id == "optiscaler" {
        apply_optiscaler_setting_side_effects(
            &mut config,
            &game_id,
            &key,
            &mut option_catalog,
            &optiscaler_releases,
        );
    }
    save_tool_config_with_reason_blocking(
        &db,
        &typed_game_id,
        &tool_id,
        &config,
        &format!("ui:set:{key}"),
    )?;
    if config.enabled {
        generate_tool_configs_blocking(&typed_game_id, &db)?;
    }
    Ok(ToolSettingWriteResult {
        status_message: format!("Updated {} setting", tool.display_name()),
        tool_option_catalog: Some(option_catalog),
    })
}

fn apply_optiscaler_setting_side_effects(
    config: &mut modde_games::tools::ToolConfig,
    game_id: &str,
    key: &str,
    option_catalog: &mut ToolOptionCatalog,
    optiscaler_releases: &[modde_games::tools::ToolReleaseSummary],
) {
    if key == "optiscaler_profile"
        && let Some(profile_id) = config.get_str("optiscaler_profile").map(str::to_string)
    {
        modde_games::tools::optiscaler::apply_profile_by_id(config, game_id, &profile_id);
    }
    if key == "release_tag"
        && let Some(tag) = config.get_str("release_tag").map(str::to_string)
    {
        if let Some(channel) =
            modde_games::tools::optiscaler::optiscaler_goverlay_channel_for_tag(&tag)
        {
            config.set("source_mode", serde_json::json!("goverlay_builds"));
            config.set("goverlay_channel", serde_json::json!(channel));
        } else {
            config.set("source_mode", serde_json::json!("github_release"));
        }
    }
    if matches!(key, "source_mode" | "goverlay_channel" | "release_tag") {
        if key == "source_mode" || key == "goverlay_channel" {
            if config.get_str("source_mode") == Some("goverlay_builds")
                && config.get_str("goverlay_channel").is_none()
            {
                config.set("goverlay_channel", serde_json::json!("edge"));
            }
            config.set("release_tag", serde_json::json!(""));
            config.set("release_asset", serde_json::json!(""));
        }
        sync_optiscaler_release_options(option_catalog, optiscaler_releases, config);
    }
}

pub(crate) async fn toggle_tool_for_game(
    db: modde_core::db::ModdeDb,
    game_id: String,
    tool_id: String,
    enabled: bool,
    context: Option<modde_games::tools::ToolGameContext>,
) -> Result<ToolSettingWriteResult, String> {
    tokio::task::spawn_blocking(move || {
        toggle_tool_for_game_blocking(db, game_id, tool_id, enabled, context)
    })
    .await
    .map_err(|err| err.to_string())?
}

fn toggle_tool_for_game_blocking(
    db: modde_core::db::ModdeDb,
    game_id: String,
    tool_id: String,
    enabled: bool,
    context: Option<modde_games::tools::ToolGameContext>,
) -> Result<ToolSettingWriteResult, String> {
    let tool = modde_games::tools::resolve_tool(&tool_id)
        .ok_or_else(|| format!("Unknown tool: {tool_id}"))?;
    let typed_game_id = GameId::from(game_id.as_str());
    let settings_json = crate::app::block_on(db.load_tool_config(&typed_game_id, &tool_id))
        .map_err(|err| err.to_string())?
        .map_or_else(
            || {
                serde_json::to_string(&tool.default_config_for(context.as_ref()).settings)
                    .unwrap_or_else(|_| "{}".to_string())
            },
            |row| row.settings_json,
        );
    crate::app::block_on(db.save_tool_config_with_reason(
        &typed_game_id,
        &tool_id,
        enabled,
        &settings_json,
        if enabled { "ui:enable" } else { "ui:disable" },
    ))
    .map_err(|err| err.to_string())?;
    generate_tool_configs_blocking(&typed_game_id, &db)?;
    Ok(ToolSettingWriteResult {
        status_message: format!(
            "{} {}",
            tool.display_name(),
            if enabled { "enabled" } else { "disabled" }
        ),
        tool_option_catalog: None,
    })
}

pub(crate) async fn restore_tool_settings_for_game(
    db: modde_core::db::ModdeDb,
    game_id: String,
    tool_id: String,
    node_id: String,
) -> Result<ToolSettingWriteResult, String> {
    tokio::task::spawn_blocking(move || {
        restore_tool_settings_for_game_blocking(db, game_id, tool_id, node_id)
    })
    .await
    .map_err(|err| err.to_string())?
}

fn restore_tool_settings_for_game_blocking(
    db: modde_core::db::ModdeDb,
    game_id: String,
    tool_id: String,
    node_id: String,
) -> Result<ToolSettingWriteResult, String> {
    let typed_game_id = GameId::from(game_id.as_str());
    crate::app::block_on(db.restore_tool_setting_node(&typed_game_id, &tool_id, &node_id))
        .map_err(|err| err.to_string())?;
    generate_tool_configs_blocking(&typed_game_id, &db)?;
    let display_name = modde_games::tools::resolve_tool(&tool_id)
        .map_or_else(|| tool_id.clone(), |tool| tool.display_name().to_string());
    Ok(ToolSettingWriteResult {
        status_message: format!("Restored {display_name} settings version"),
        tool_option_catalog: None,
    })
}

pub(crate) async fn save_optiscaler_release_selection_for_game(
    db: modde_core::db::ModdeDb,
    game_id: String,
    releases: Vec<modde_games::tools::ToolReleaseSummary>,
    option_catalog: ToolOptionCatalog,
    context: Option<modde_games::tools::ToolGameContext>,
) -> Result<ToolSettingWriteResult, String> {
    let tool = modde_games::tools::resolve_tool("optiscaler")
        .ok_or_else(|| "OptiScaler tool is not registered".to_string())?;
    let typed_game_id = GameId::from(game_id.as_str());
    let mut option_catalog = option_catalog;
    let mut config =
        load_tool_config_or_default_async(&db, &typed_game_id, tool, context.as_ref()).await?;
    let _ = modde_games::tools::optiscaler::normalize_optiscaler_release_config(&mut config);
    let selected = sync_optiscaler_release_options(&mut option_catalog, &releases, &mut config);
    if let Some((tag, asset)) = selected {
        config.set("release_tag", serde_json::json!(tag));
        config.set("release_asset", serde_json::json!(asset));
        save_tool_config_with_reason_async(&db, &typed_game_id, "optiscaler", &config, "ui:update")
            .await?;
    }
    Ok(ToolSettingWriteResult {
        status_message: format!("Loaded {} OptiScaler release(s)", releases.len()),
        tool_option_catalog: Some(option_catalog),
    })
}

pub(crate) async fn save_proton_selected_version_for_game(
    db: modde_core::db::ModdeDb,
    game_id: String,
    versions: Vec<String>,
    option_catalog: ToolOptionCatalog,
) -> Result<ToolSettingWriteResult, String> {
    let mut option_catalog = option_catalog;
    set_tool_options(
        &mut option_catalog,
        "proton",
        "selected_version",
        versions.clone(),
    );
    set_tool_options(
        &mut option_catalog,
        "proton",
        "_catalog_loaded",
        vec!["true".to_string()],
    );
    let tool = modde_games::tools::resolve_tool("proton")
        .ok_or_else(|| "Proton tool is not registered".to_string())?;
    let typed_game_id = GameId::from(game_id.as_str());
    let mut config = load_tool_config_or_default_async(&db, &typed_game_id, tool, None).await?;
    let selected = config.get_str("selected_version").unwrap_or("latest");
    if !versions.iter().any(|version| version == selected) {
        config.set("selected_version", serde_json::json!("latest"));
        save_tool_config_with_reason_async(&db, &typed_game_id, "proton", &config, "ui:update")
            .await?;
    }
    Ok(ToolSettingWriteResult {
        status_message: format!("Loaded {} Proton version option(s)", versions.len()),
        tool_option_catalog: Some(option_catalog),
    })
}

pub(crate) async fn adopt_optiscaler_for_game(
    db: modde_core::db::ModdeDb,
    game_id: String,
    game_dir: PathBuf,
    context: Option<modde_games::tools::ToolGameContext>,
) -> Result<ToolSettingWriteResult, String> {
    tokio::task::spawn_blocking(move || {
        adopt_optiscaler_for_game_blocking(db, game_id, game_dir, context)
    })
    .await
    .map_err(|err| err.to_string())?
}

fn adopt_optiscaler_for_game_blocking(
    db: modde_core::db::ModdeDb,
    game_id: String,
    game_dir: PathBuf,
    context: Option<modde_games::tools::ToolGameContext>,
) -> Result<ToolSettingWriteResult, String> {
    let typed_game_id = GameId::from(game_id.as_str());
    let tool = modde_games::tools::resolve_tool("optiscaler")
        .ok_or_else(|| "OptiScaler tool is not registered".to_string())?;
    let mut config =
        load_tool_config_or_default_blocking(&db, &typed_game_id, tool, context.as_ref())?;
    modde_games::tools::optiscaler::apply_game_defaults(&mut config, context.as_ref());
    let managed = modde_games::tools::optiscaler::managed_paths_from_config(&config);
    let state =
        modde_games::tools::optiscaler::scan_optiscaler_install(&game_id, &game_dir, &managed)
            .map_err(|err| err.to_string())?;
    let paths = state
        .recognized_files
        .iter()
        .map(|file| state.executable_dir.join(&file.rel_path))
        .map(|path| {
            path.strip_prefix(&game_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect::<Vec<_>>();
    let applied = modde_games::tools::AppliedFiles {
        files: paths.iter().map(PathBuf::from).collect(),
    };
    config.enabled = true;
    config.set(
        "managed_manifest",
        modde_games::tools::optiscaler::managed_manifest_json(&game_dir, &applied),
    );
    save_tool_config_with_reason_blocking(&db, &typed_game_id, "optiscaler", &config, "ui:adopt")?;
    crate::app::block_on(db.clear_applied_files(&typed_game_id, "optiscaler"))
        .map_err(|err| err.to_string())?;
    crate::app::block_on(db.save_applied_files(&typed_game_id, "optiscaler", &paths))
        .map_err(|err| err.to_string())?;
    Ok(ToolSettingWriteResult {
        status_message: format!("Adopted OptiScaler ({} file(s))", paths.len()),
        tool_option_catalog: None,
    })
}

pub(crate) async fn reset_optiscaler_config_for_game(
    db: modde_core::db::ModdeDb,
    game_id: String,
) -> Result<ToolSettingWriteResult, String> {
    let typed_game_id = GameId::from(game_id.as_str());
    let tool = modde_games::tools::resolve_tool("optiscaler")
        .ok_or_else(|| "OptiScaler tool is not registered".to_string())?;
    let mut config = load_tool_config_or_default_async(&db, &typed_game_id, tool, None).await?;
    if let serde_json::Value::Object(map) = &mut config.settings {
        map.remove("ini_overrides");
        map.insert("force_config_reset".to_string(), serde_json::json!(true));
    }
    save_tool_config_with_reason_async(&db, &typed_game_id, "optiscaler", &config, "ui:reset")
        .await?;
    Ok(ToolSettingWriteResult {
        status_message: "Reset OptiScaler config overrides".to_string(),
        tool_option_catalog: None,
    })
}

fn load_tool_config_or_default_blocking(
    db: &modde_core::db::ModdeDb,
    game_id: &GameId,
    tool: &'static dyn modde_games::tools::GameTool,
    context: Option<&modde_games::tools::ToolGameContext>,
) -> Result<modde_games::tools::ToolConfig, String> {
    let row = crate::app::block_on(db.load_tool_config(game_id, tool.tool_id()))
        .map_err(|err| err.to_string())?;
    Ok(row.map_or_else(
        || tool.default_config_for(context),
        |row| modde_games::tools::ToolConfig {
            tool_id: row.tool_id,
            enabled: row.enabled,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        },
    ))
}

async fn load_tool_config_or_default_async(
    db: &modde_core::db::ModdeDb,
    game_id: &GameId,
    tool: &'static dyn modde_games::tools::GameTool,
    context: Option<&modde_games::tools::ToolGameContext>,
) -> Result<modde_games::tools::ToolConfig, String> {
    let row = db
        .load_tool_config(game_id, tool.tool_id())
        .await
        .map_err(|err| err.to_string())?;
    Ok(row.map_or_else(
        || tool.default_config_for(context),
        |row| modde_games::tools::ToolConfig {
            tool_id: row.tool_id,
            enabled: row.enabled,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        },
    ))
}

fn save_tool_config_with_reason_blocking(
    db: &modde_core::db::ModdeDb,
    game_id: &GameId,
    tool_id: &str,
    config: &modde_games::tools::ToolConfig,
    reason: &str,
) -> Result<(), String> {
    let settings_json = serde_json::to_string(&config.settings).map_err(|err| err.to_string())?;
    crate::app::block_on(db.save_tool_config_with_reason(
        game_id,
        tool_id,
        config.enabled,
        &settings_json,
        reason,
    ))
    .map_err(|err| err.to_string())
}

pub(crate) async fn save_tool_config_with_reason_async(
    db: &modde_core::db::ModdeDb,
    game_id: &GameId,
    tool_id: &str,
    config: &modde_games::tools::ToolConfig,
    reason: &str,
) -> Result<(), String> {
    let settings_json = serde_json::to_string(&config.settings).map_err(|err| err.to_string())?;
    db.save_tool_config_with_reason(game_id, tool_id, config.enabled, &settings_json, reason)
        .await
        .map_err(|err| err.to_string())
}

fn generate_tool_configs_blocking(
    game_id: &GameId,
    db: &modde_core::db::ModdeDb,
) -> Result<(), String> {
    crate::app::block_on(modde_games::launcher::generate_tool_configs(game_id, db))
        .map_err(|err| err.to_string())
}
