use super::ToolOptionCatalog;
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
