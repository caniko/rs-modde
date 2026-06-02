use std::path::PathBuf;

use super::ToolOptionCatalog;
use super::state::ToolSettingWriteResult;
use modde_core::resolver::GameId;

pub(super) fn tool_option_key(tool_id: &str, setting_key: &str) -> String {
    format!("{tool_id}.{setting_key}")
}

pub(super) fn set_tool_options(
    catalog: &mut ToolOptionCatalog,
    tool_id: &str,
    setting_key: &str,
    options: Vec<String>,
) {
    catalog.insert(tool_option_key(tool_id, setting_key), options);
}

pub(super) fn tool_options<'a>(
    catalog: &'a ToolOptionCatalog,
    tool_id: &str,
    setting_key: &str,
) -> Option<&'a Vec<String>> {
    catalog.get(&tool_option_key(tool_id, setting_key))
}

pub(super) fn normalize_tool_setting_value(
    settings: &serde_json::Value,
    key: &str,
    value: serde_json::Value,
) -> serde_json::Value {
    if get_tool_setting_value(settings, key).is_some_and(serde_json::Value::is_array)
        && let Some(raw) = value.as_str()
    {
        return serde_json::Value::Array(
            raw.split([',', ':'])
                .filter_map(|part| {
                    let trimmed = part.trim();
                    (!trimmed.is_empty()).then(|| serde_json::Value::String(trimmed.to_string()))
                })
                .collect(),
        );
    }
    value
}

pub(super) fn normalize_tool_settings_for_specs(
    settings: &serde_json::Value,
    specs: &[modde_games::tools::ToolSettingSpec],
) -> serde_json::Value {
    let mut normalized = settings.clone();
    for spec in specs {
        let Some(value) = get_tool_setting_value(&normalized, spec.key).cloned() else {
            continue;
        };
        let value = normalize_tool_setting_for_kind(value, &spec.kind);
        set_nested_tool_setting(&mut normalized, spec.key, value);
    }
    normalized
}

pub(super) fn normalize_tool_setting_for_kind(
    value: serde_json::Value,
    kind: &modde_games::tools::ToolSettingKind,
) -> serde_json::Value {
    match kind {
        modde_games::tools::ToolSettingKind::Bool => value
            .as_bool()
            .or_else(|| value.as_str().and_then(parse_bool_setting))
            .map_or(value, |value| serde_json::json!(value)),
        modde_games::tools::ToolSettingKind::TriStateBool => {
            if value
                .as_str()
                .is_some_and(|value| value.eq_ignore_ascii_case("auto"))
            {
                serde_json::json!("auto")
            } else {
                value
                    .as_bool()
                    .or_else(|| value.as_str().and_then(parse_bool_setting))
                    .map_or(value, |value| serde_json::json!(value))
            }
        }
        modde_games::tools::ToolSettingKind::Number { .. } => value
            .as_f64()
            .or_else(|| value.as_str().and_then(|value| value.trim().parse().ok()))
            .map_or(value, |value| serde_json::json!(value)),
        _ => value,
    }
}

pub(super) fn parse_bool_setting(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub(super) fn get_tool_setting_value<'a>(
    settings: &'a serde_json::Value,
    key: &str,
) -> Option<&'a serde_json::Value> {
    settings
        .get(key)
        .or_else(|| get_nested_tool_setting(settings, key))
        .or_else(|| get_legacy_flat_child_tool_setting(settings, key))
}

fn get_nested_tool_setting<'a>(
    settings: &'a serde_json::Value,
    key: &str,
) -> Option<&'a serde_json::Value> {
    let mut current = settings;
    for part in key.split('.') {
        current = current.as_object()?.get(part)?;
    }
    Some(current)
}

fn get_legacy_flat_child_tool_setting<'a>(
    settings: &'a serde_json::Value,
    key: &str,
) -> Option<&'a serde_json::Value> {
    let (root, child) = key.split_once('.')?;
    settings.as_object()?.get(root)?.as_object()?.get(child)
}

pub(super) fn set_nested_tool_setting(
    settings: &mut serde_json::Value,
    key: &str,
    value: serde_json::Value,
) {
    if !key.contains('.') {
        if !settings.is_object() {
            *settings = serde_json::json!({});
        }
        let map = settings.as_object_mut().expect("settings object");
        map.insert(key.to_string(), value);
        return;
    }

    if !settings.is_object() {
        *settings = serde_json::json!({});
    }
    if let Some(map) = settings.as_object_mut() {
        map.remove(key);
    }

    let mut current = settings;
    let parts: Vec<&str> = key.split('.').collect();
    for (index, part) in parts.iter().enumerate() {
        let object = current.as_object_mut().expect("settings object");
        if index == parts.len() - 1 {
            object.insert((*part).to_string(), value);
            return;
        }

        if index == 0 && parts.len() > 2 {
            object
                .entry((*part).to_string())
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
                .map(|root| root.remove(&parts[1..].join(".")));
        }

        current = object
            .entry((*part).to_string())
            .or_insert_with(|| serde_json::json!({}));
        if !current.is_object() {
            *current = serde_json::json!({});
        }
    }
}

pub(super) fn apply_derived_tool_settings(
    config: &mut modde_games::tools::ToolConfig,
    context: Option<&modde_games::tools::ToolGameContext>,
) {
    if let Some(context) = context {
        if let Some(path) = &context.executable_dir {
            config.set(
                "derived_executable_dir",
                serde_json::json!(path.display().to_string()),
            );
        }
        config.set(
            "derived_launcher",
            serde_json::json!(context.launcher_label()),
        );
        if let Some(app_id) = &context.steam_app_id {
            config.set("derived_steam_app_id", serde_json::json!(app_id));
        }
    }
}

pub(super) fn tool_apply_signature(settings: &serde_json::Value) -> serde_json::Value {
    let mut signature = settings.clone();
    if let Some(map) = signature.as_object_mut() {
        map.remove("_game_id");
        map.remove("_last_applied_settings");
        map.remove("managed_manifest");
    }
    signature
}

pub(super) fn tool_apply_is_pending(
    config: &modde_games::tools::ToolConfig,
    applied_files: &[String],
) -> bool {
    if applied_files.is_empty() {
        return true;
    }
    let current = tool_apply_signature(&config.settings);
    config.settings.get("_last_applied_settings") != Some(&current)
}

pub(super) fn format_tool_availability(
    availability: &modde_games::tools::ToolAvailability,
) -> String {
    match availability {
        modde_games::tools::ToolAvailability::Available {
            version: Some(version),
        } => format!("available ({version})"),
        modde_games::tools::ToolAvailability::Available { version: None } => {
            "available".to_string()
        }
        modde_games::tools::ToolAvailability::NotInstalled { .. } => "missing".to_string(),
    }
}

pub(super) fn build_tool_derived_facts(
    context: Option<&modde_games::tools::ToolGameContext>,
) -> Vec<(String, String)> {
    let Some(context) = context else {
        return Vec::new();
    };
    let mut facts = vec![
        ("Game".to_string(), context.display_name.clone()),
        ("Launcher".to_string(), context.launcher_label()),
    ];
    if let Some(path) = &context.install_path {
        facts.push(("Install path".to_string(), path.display().to_string()));
    }
    if let Some(path) = &context.executable_dir {
        facts.push((
            "Executable directory".to_string(),
            path.display().to_string(),
        ));
    }
    if let Some(app_id) = &context.steam_app_id {
        facts.push(("Steam app id".to_string(), app_id.clone()));
    }
    facts
}

pub(super) fn patch_tool_setting_options(
    tool_id: &str,
    specs: &mut [modde_games::tools::ToolSettingSpec],
    option_catalog: &ToolOptionCatalog,
) {
    for spec in specs {
        if let Some(options) = tool_options(option_catalog, tool_id, spec.key)
            && !options.is_empty()
        {
            spec.kind = modde_games::tools::ToolSettingKind::Select {
                options: options
                    .iter()
                    .cloned()
                    .map(modde_games::tools::ToolSelectOption::value_label)
                    .collect(),
            };
        }
    }
}

pub(super) fn installable_tool_assets(
    tool_id: &str,
    release: &modde_games::tools::ToolReleaseSummary,
) -> Vec<String> {
    modde_games::tools::resolve_tool(tool_id)
        .map(|tool| tool.installable_release_assets(release))
        .unwrap_or_default()
}

pub(super) fn tool_assets_for_tag(
    tool_id: &str,
    releases: &[modde_games::tools::ToolReleaseSummary],
    tag: &str,
) -> Vec<String> {
    releases
        .iter()
        .find(|release| release.tag == tag)
        .map(|release| installable_tool_assets(tool_id, release))
        .unwrap_or_default()
}

pub(super) fn optiscaler_release_tags_for_config(
    releases: &[modde_games::tools::ToolReleaseSummary],
    config: &modde_games::tools::ToolConfig,
) -> Vec<String> {
    releases
        .iter()
        .filter(|release| {
            modde_games::tools::optiscaler::optiscaler_release_matches_config(release, config)
                && !installable_tool_assets("optiscaler", release).is_empty()
        })
        .map(|release| release.tag.clone())
        .collect()
}

pub(super) fn first_optiscaler_release_for_config(
    releases: &[modde_games::tools::ToolReleaseSummary],
    config: &modde_games::tools::ToolConfig,
) -> Option<(String, String)> {
    releases.iter().find_map(|release| {
        modde_games::tools::optiscaler::optiscaler_release_matches_config(release, config)
            .then(|| {
                installable_tool_assets("optiscaler", release)
                    .into_iter()
                    .next()
                    .map(|asset| (release.tag.clone(), asset))
            })
            .flatten()
    })
}

pub(super) fn sync_optiscaler_release_options(
    option_catalog: &mut ToolOptionCatalog,
    releases: &[modde_games::tools::ToolReleaseSummary],
    config: &mut modde_games::tools::ToolConfig,
) -> Option<(String, String)> {
    if !matches!(
        config.get_str("source_mode"),
        Some("github_release" | "goverlay_builds")
    ) {
        set_tool_options(option_catalog, "optiscaler", "release_tag", Vec::new());
        set_tool_options(option_catalog, "optiscaler", "release_asset", Vec::new());
        config.set("release_tag", serde_json::json!(""));
        config.set("release_asset", serde_json::json!(""));
        return None;
    }
    let _ = modde_games::tools::optiscaler::normalize_optiscaler_release_config(config);
    let release_tags = optiscaler_release_tags_for_config(releases, config);
    set_tool_options(
        option_catalog,
        "optiscaler",
        "release_tag",
        release_tags.clone(),
    );
    let configured_tag = config.get_str("release_tag").unwrap_or("");
    let configured_asset = config.get_str("release_asset").unwrap_or("");
    let selected = if release_tags.iter().any(|tag| tag == configured_tag) {
        let assets = tool_assets_for_tag("optiscaler", releases, configured_tag);
        let asset = if assets.iter().any(|asset| asset == configured_asset) {
            configured_asset.to_string()
        } else {
            assets.first().cloned().unwrap_or_default()
        };
        Some((configured_tag.to_string(), asset))
    } else {
        first_optiscaler_release_for_config(releases, config)
    };
    if let Some((tag, asset)) = &selected {
        set_tool_options(
            option_catalog,
            "optiscaler",
            "release_asset",
            tool_assets_for_tag("optiscaler", releases, tag),
        );
        config.set("release_tag", serde_json::json!(tag));
        config.set("release_asset", serde_json::json!(asset));
    } else {
        set_tool_options(option_catalog, "optiscaler", "release_asset", Vec::new());
        config.set("release_asset", serde_json::json!(""));
    }
    selected
}

pub(super) fn current_tool_config(
    db: &modde_core::db::ModdeDb,
    game_id: &str,
    tool_id: &str,
) -> Result<modde_games::tools::ToolConfig, String> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| format!("Tool is not registered: {tool_id}"))?;
    let Some(row) = crate::app::block_on(db.load_tool_config(&GameId::from(game_id), tool_id))
        .ok()
        .flatten()
    else {
        return Ok(tool.default_config());
    };
    let mut config = modde_games::tools::ToolConfig {
        tool_id: row.tool_id,
        enabled: row.enabled,
        settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
    };
    if tool_id == "optiscaler" {
        let _ = modde_games::tools::optiscaler::normalize_optiscaler_release_config(&mut config);
    }
    Ok(config)
}

pub(super) fn save_tool_settings(
    db: &modde_core::db::ModdeDb,
    game_id: &str,
    tool_id: &str,
    config: &modde_games::tools::ToolConfig,
) -> Result<(), String> {
    save_tool_settings_with_reason(db, game_id, tool_id, config, "ui:update")
}

pub(super) fn save_tool_settings_with_reason(
    db: &modde_core::db::ModdeDb,
    game_id: &str,
    tool_id: &str,
    config: &modde_games::tools::ToolConfig,
    reason: &str,
) -> Result<(), String> {
    let settings_json = serde_json::to_string(&config.settings).map_err(|err| err.to_string())?;
    crate::app::block_on(db.save_tool_config_with_reason(
        &GameId::from(game_id),
        tool_id,
        config.enabled,
        &settings_json,
        reason,
    ))
    .map_err(|err| err.to_string())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn save_tool_setting_for_game(
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
    let mut config = load_tool_config_or_default(&db, &typed_game_id, tool, context.as_ref())?;
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
    if tool_id == "optiscaler"
        && key == "optiscaler_profile"
        && let Some(profile_id) = config.get_str("optiscaler_profile").map(str::to_string)
    {
        modde_games::tools::optiscaler::apply_profile_by_id(&mut config, &game_id, &profile_id);
    }
    if tool_id == "optiscaler"
        && key == "release_tag"
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
    if tool_id == "optiscaler"
        && matches!(
            key.as_str(),
            "source_mode" | "goverlay_channel" | "release_tag"
        )
    {
        if key == "source_mode" || key == "goverlay_channel" {
            if config.get_str("source_mode") == Some("goverlay_builds")
                && config.get_str("goverlay_channel").is_none()
            {
                config.set("goverlay_channel", serde_json::json!("edge"));
            }
            config.set("release_tag", serde_json::json!(""));
            config.set("release_asset", serde_json::json!(""));
        }
        sync_optiscaler_release_options(&mut option_catalog, &optiscaler_releases, &mut config);
    }
    save_tool_config_with_reason(
        &db,
        &typed_game_id,
        &tool_id,
        &config,
        &format!("ui:set:{key}"),
    )?;
    if config.enabled {
        generate_tool_configs(&typed_game_id, &db)?;
    }
    Ok(ToolSettingWriteResult {
        status_message: format!("Updated {} setting", tool.display_name()),
        tool_option_catalog: Some(option_catalog),
    })
}

pub(super) async fn toggle_tool_for_game(
    db: modde_core::db::ModdeDb,
    game_id: String,
    tool_id: String,
    enabled: bool,
    context: Option<modde_games::tools::ToolGameContext>,
) -> Result<ToolSettingWriteResult, String> {
    tokio::task::spawn_blocking(move || {
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
        generate_tool_configs(&typed_game_id, &db)?;
        Ok(ToolSettingWriteResult {
            status_message: format!(
                "{} {}",
                tool.display_name(),
                if enabled { "enabled" } else { "disabled" }
            ),
            tool_option_catalog: None,
        })
    })
    .await
    .map_err(|err| err.to_string())?
}

pub(super) async fn restore_tool_settings_for_game(
    db: modde_core::db::ModdeDb,
    game_id: String,
    tool_id: String,
    node_id: String,
) -> Result<ToolSettingWriteResult, String> {
    tokio::task::spawn_blocking(move || {
        let typed_game_id = GameId::from(game_id.as_str());
        crate::app::block_on(db.restore_tool_setting_node(&typed_game_id, &tool_id, &node_id))
            .map_err(|err| err.to_string())?;
        generate_tool_configs(&typed_game_id, &db)?;
        let display_name = modde_games::tools::resolve_tool(&tool_id)
            .map_or_else(|| tool_id.clone(), |tool| tool.display_name().to_string());
        Ok(ToolSettingWriteResult {
            status_message: format!("Restored {display_name} settings version"),
            tool_option_catalog: None,
        })
    })
    .await
    .map_err(|err| err.to_string())?
}

pub(super) async fn save_optiscaler_release_selection_for_game(
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

pub(super) async fn save_proton_selected_version_for_game(
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

pub(super) async fn adopt_optiscaler_for_game(
    db: modde_core::db::ModdeDb,
    game_id: String,
    game_dir: PathBuf,
    context: Option<modde_games::tools::ToolGameContext>,
) -> Result<ToolSettingWriteResult, String> {
    tokio::task::spawn_blocking(move || {
        let typed_game_id = GameId::from(game_id.as_str());
        let tool = modde_games::tools::resolve_tool("optiscaler")
            .ok_or_else(|| "OptiScaler tool is not registered".to_string())?;
        let mut config = load_tool_config_or_default(&db, &typed_game_id, tool, context.as_ref())?;
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
        save_tool_config_with_reason(&db, &typed_game_id, "optiscaler", &config, "ui:adopt")?;
        crate::app::block_on(db.clear_applied_files(&typed_game_id, "optiscaler"))
            .map_err(|err| err.to_string())?;
        crate::app::block_on(db.save_applied_files(&typed_game_id, "optiscaler", &paths))
            .map_err(|err| err.to_string())?;
        Ok(ToolSettingWriteResult {
            status_message: format!("Adopted OptiScaler ({} file(s))", paths.len()),
            tool_option_catalog: None,
        })
    })
    .await
    .map_err(|err| err.to_string())?
}

pub(super) async fn reset_optiscaler_config_for_game(
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

fn load_tool_config_or_default(
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

fn save_tool_config_with_reason(
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

async fn save_tool_config_with_reason_async(
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

fn generate_tool_configs(game_id: &GameId, db: &modde_core::db::ModdeDb) -> Result<(), String> {
    crate::app::block_on(modde_games::launcher::generate_tool_configs(game_id, db))
        .map_err(|err| err.to_string())
}
