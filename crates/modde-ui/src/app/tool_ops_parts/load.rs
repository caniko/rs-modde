#![allow(clippy::wildcard_imports)]
use super::*;

use modde_core::resolver::GameId;

pub(crate) async fn load_tool_releases(
    tool_id: String,
) -> Result<Vec<modde_games::tools::ToolReleaseSummary>, String> {
    let tool = modde_games::tools::resolve_tool(&tool_id)
        .ok_or_else(|| format!("Tool is not registered: {tool_id}"))?;
    if !tool.supports_releases() {
        return Err(format!(
            "{} does not support release selection",
            tool.display_name()
        ));
    }
    tool.list_releases().await.map_err(|err| err.to_string())
}

#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
pub(crate) fn proton_version_options_for_ui() -> Vec<String> {
    modde_games::tools::proton::proton_version_options()
}

#[cfg(not(all(target_os = "linux", feature = "linux-integrations")))]
pub(crate) fn proton_version_options_for_ui() -> Vec<String> {
    Vec::new()
}

#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
pub(crate) async fn load_proton_versions() -> Result<Vec<String>, String> {
    modde_games::tools::proton::list_ge_proton_versions()
        .await
        .map_err(|err| err.to_string())
}

#[cfg(not(all(target_os = "linux", feature = "linux-integrations")))]
pub(crate) async fn load_proton_versions() -> Result<Vec<String>, String> {
    Err("Proton integration is not enabled for this platform build".to_string())
}

pub(crate) async fn install_selected_tool_release(
    db: modde_core::db::ModdeDb,
    game_id: String,
    tool_id: String,
) -> Result<String, String> {
    let tool = modde_games::tools::resolve_tool(&tool_id)
        .ok_or_else(|| format!("Tool is not registered: {tool_id}"))?;
    let config = current_tool_config_async(&db, &game_id, &tool_id).await?;
    let selected_tag = config
        .get_str("release_tag")
        .unwrap_or("latest")
        .to_string();
    let selected_asset = config.get_str("release_asset").unwrap_or("").to_string();
    if selected_asset.trim().is_empty() {
        return Err(format!(
            "Select a {} release asset before installing",
            tool.display_name()
        ));
    }
    let config = tool
        .install_release(&game_id, config, &selected_tag, &selected_asset)
        .await
        .map_err(|err| err.to_string())?;
    save_tool_config_with_reason_async(
        &db,
        &GameId::from(game_id.as_str()),
        &tool_id,
        &config,
        "ui:update",
    )
    .await?;
    Ok(format!(
        "Installed {} {}",
        tool.display_name(),
        config.get_str("release_tag").unwrap_or("release")
    ))
}

#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
pub(crate) async fn install_selected_proton_version(
    db: modde_core::db::ModdeDb,
    game_id: String,
) -> Result<String, String> {
    let tool = modde_games::tools::resolve_tool("proton")
        .ok_or_else(|| "Proton tool is not registered".to_string())?;
    let row = db
        .load_tool_config(&GameId::from(game_id.as_str()), "proton")
        .await
        .map_err(|err| err.to_string())?;
    let config = row.map_or_else(
        || tool.default_config(),
        |row| modde_games::tools::ToolConfig {
            tool_id: row.tool_id,
            enabled: row.enabled,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        },
    );
    let version = config.get_str("selected_version").unwrap_or("latest");
    let target = config.get_str("install_target").unwrap_or("steam");
    modde_games::tools::proton::install_ge_proton_with_protonup_rs(version, target)
        .map_err(|err| err.to_string())?;
    Ok(format!("Installed GEProton {version} for {target}"))
}

#[cfg(not(all(target_os = "linux", feature = "linux-integrations")))]
pub(crate) async fn install_selected_proton_version(
    _db: modde_core::db::ModdeDb,
    _game_id: String,
) -> Result<String, String> {
    Err("Proton integration is not enabled for this platform build".to_string())
}

pub(crate) async fn load_tools_state(
    db: modde_core::db::ModdeDb,
    request: ToolLoadRequest,
) -> Result<ToolLoadSnapshot, String> {
    tokio::task::spawn_blocking(move || load_tools_state_blocking(db, request))
        .await
        .map_err(|err| err.to_string())?
}

pub(crate) async fn load_executables_for_game(
    db: modde_core::db::ModdeDb,
    game_id: String,
) -> Result<Vec<ExecutableUiEntry>, String> {
    db.load_executable_configs(&GameId::from(game_id.as_str()))
        .await
        .map_err(|err| err.to_string())
        .map(|rows| rows.into_iter().map(ExecutableUiEntry::from_row).collect())
}

pub(crate) fn load_tools_state_blocking(
    db: modde_core::db::ModdeDb,
    request: ToolLoadRequest,
) -> Result<ToolLoadSnapshot, String> {
    let detected =
        modde_games::detection::find_detected_game(&GameId::from(request.game_id.as_str()));
    let game_dir = request.configured_game_dir.clone().or_else(|| {
        detected
            .as_ref()
            .map(|detected| detected.install_path.clone())
            .or_else(|| {
                modde_games::resolve_game_plugin(&request.game_id)
                    .and_then(modde_games::GamePlugin::detect_install)
            })
    });
    let context = Some(modde_games::tools::ToolGameContext::from_parts(
        &request.game_id,
        request.display_name.clone(),
        game_dir.clone(),
        detected.as_ref(),
    ));
    let game_dir_configured = game_dir.is_some();
    let mut option_catalog = request.tool_option_catalog.clone();
    #[cfg(all(target_os = "linux", feature = "linux-integrations"))]
    if tool_options(&option_catalog, "proton", "selected_version").is_none() {
        set_tool_options(
            &mut option_catalog,
            "proton",
            "selected_version",
            proton_version_options_for_ui(),
        );
    }
    if !request.optiscaler_releases.is_empty()
        && let Ok(mut config) = current_tool_config_blocking(&db, &request.game_id, "optiscaler")
    {
        sync_optiscaler_release_options(
            &mut option_catalog,
            &request.optiscaler_releases,
            &mut config,
        );
    }

    let entries = modde_games::tools::all_tools()
        .iter()
        .map(|tool| {
            build_tool_ui_entry_blocking(
                &db,
                &request.game_id,
                game_dir.as_deref(),
                context.as_ref(),
                *tool,
                &option_catalog,
            )
        })
        .collect::<Vec<_>>();
    let active_tool_id = request
        .previous_active_tool_id
        .filter(|active| entries.iter().any(|entry| entry.tool_id == *active))
        .or_else(|| entries.first().map(|entry| entry.tool_id.clone()));
    let executables =
        crate::app::block_on(db.load_executable_configs(&GameId::from(request.game_id.as_str())))
            .map_err(|err| err.to_string())?
            .into_iter()
            .map(ExecutableUiEntry::from_row)
            .collect();

    Ok(ToolLoadSnapshot {
        entries,
        active_tool_id,
        game_label: Some(request.display_name),
        game_dir_configured,
        tool_option_catalog: option_catalog,
        executables,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_tool_ui_entry_blocking(
    db: &modde_core::db::ModdeDb,
    game_id: &str,
    game_dir: Option<&std::path::Path>,
    context: Option<&modde_games::tools::ToolGameContext>,
    tool: &'static dyn modde_games::tools::GameTool,
    option_catalog: &ToolOptionCatalog,
) -> ToolUiEntry {
    let typed_game_id = GameId::from(game_id);
    let row = crate::app::block_on(db.load_tool_config(&typed_game_id, tool.tool_id()))
        .ok()
        .flatten();
    let availability = tool.detect_available();
    let applied_files = crate::app::block_on(db.load_applied_files(&typed_game_id, tool.tool_id()))
        .unwrap_or_default();
    let status_message = match &availability {
        modde_games::tools::ToolAvailability::Available {
            version: Some(version),
        } => Some(format!("Detected {version}")),
        modde_games::tools::ToolAvailability::NotInstalled { install_hint } => {
            Some(install_hint.clone())
        }
        modde_games::tools::ToolAvailability::Available { version: None } => None,
    };
    let availability_text = format_tool_availability(&availability);
    let mut config = row.as_ref().map_or_else(
        || tool.default_config_for(context),
        |row| modde_games::tools::ToolConfig {
            tool_id: row.tool_id.clone(),
            enabled: row.enabled,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        },
    );
    let release_config_normalized = tool.tool_id() == "optiscaler"
        && modde_games::tools::optiscaler::normalize_optiscaler_release_config(&mut config);
    let mut setting_specs = tool.settings_schema_for(context, &config);
    let normalized_settings = normalize_tool_settings_for_specs(&config.settings, &setting_specs);
    if release_config_normalized || normalized_settings != config.settings {
        config.settings = normalized_settings;
        if let Ok(settings_json) = serde_json::to_string(&config.settings) {
            let _ = crate::app::block_on(db.save_tool_config(
                &typed_game_id,
                tool.tool_id(),
                config.enabled,
                &settings_json,
            ));
        }
        setting_specs = tool.settings_schema_for(context, &config);
    }
    config.set("_game_id", serde_json::json!(game_id));
    apply_derived_tool_settings(&mut config, context);
    let mut apply_pending = tool_apply_is_pending(&config, &applied_files);
    let mut apply_missing_inputs = Vec::new();
    let generated_config_path = tool
        .generate_config_for(context, &config)
        .map(|generated| generated.path.display().to_string());
    let env_preview = tool.env_vars_for(context, &config).into_iter().collect();
    let dll_overrides = tool
        .wine_dll_overrides_for(context, &config)
        .into_iter()
        .collect();
    let wrapper_preview = tool
        .wrapper_command(&config)
        .map(|wrapper| {
            if wrapper.args.is_empty() {
                vec![wrapper.exe]
            } else {
                vec![format!("{} {}", wrapper.exe, wrapper.args)]
            }
        })
        .unwrap_or_default();
    patch_tool_setting_options(tool.tool_id(), &mut setting_specs, option_catalog);
    let mut derived_facts = build_tool_derived_facts(context);
    if matches!(tool.tool_id(), "reshade" | "optiscaler")
        && let Some(game_dir) = game_dir
    {
        match tool.preview_apply_for(game_dir, context, &config) {
            Ok(preview) => {
                let has_changes = preview.has_changes();
                apply_missing_inputs = preview.missing_inputs.clone();
                apply_pending = apply_missing_inputs.is_empty() && has_changes;
                let summary = if !apply_missing_inputs.is_empty() {
                    format!("missing input: {}", apply_missing_inputs.join("; "))
                } else if has_changes {
                    format!(
                        "{} changed / {} unchanged",
                        preview.changed_files.len(),
                        preview.unchanged_files.len()
                    )
                } else {
                    format!("no changes ({} file(s))", preview.planned_files.len())
                };
                derived_facts.push(("Apply preview".to_string(), summary));
            }
            Err(err) => {
                derived_facts.push(("Apply preview".to_string(), format!("failed: {err}")));
            }
        }
    }
    let (optiscaler_state, optiscaler_latest_backup, optiscaler_detected_files) =
        if tool.tool_id() == "optiscaler" {
            let managed = modde_games::tools::optiscaler::managed_paths_from_config(&config);
            if let Some(game_dir) = game_dir {
                if let Ok(state) = modde_games::tools::optiscaler::scan_optiscaler_install(
                    game_id, game_dir, &managed,
                ) {
                    if !matches!(
                    state.status,
                    modde_games::tools::optiscaler::OptiScalerInstallStatus::Managed
                        | modde_games::tools::optiscaler::OptiScalerInstallStatus::PartiallyManaged
                ) {
                        apply_pending = true;
                    }
                    derived_facts.push(("OptiScaler state".to_string(), state.summary()));
                    if let Some(path) = &state.config_path {
                        derived_facts.push((
                            "OptiScaler config".to_string(),
                            format!(
                                "{} ({} setting(s))",
                                path.display(),
                                state.ini_settings.len()
                            ),
                        ));
                    }
                    if let Some(path) = &state.latest_backup {
                        derived_facts
                            .push(("OptiScaler backup".to_string(), path.display().to_string()));
                    }
                    (
                        Some(state.summary()),
                        state.latest_backup.map(|path| path.display().to_string()),
                        state.recognized_files.len(),
                    )
                } else {
                    (None, None, 0)
                }
            } else {
                (None, None, 0)
            }
        } else {
            (None, None, 0)
        };
    let setting_history =
        crate::app::block_on(db.list_tool_setting_history(&typed_game_id, tool.tool_id(), 8))
            .unwrap_or_default()
            .into_iter()
            .map(ToolHistoryUiEntry::from_node)
            .collect();

    ToolUiEntry {
        tool_id: tool.tool_id().to_string(),
        display_name: tool.display_name().to_string(),
        description: tool.description().to_string(),
        category: tool.category().to_string(),
        available: availability.is_available(),
        availability_text,
        enabled: config.enabled,
        settings: config.settings.clone(),
        setting_specs,
        generated_config_path,
        applied_files,
        has_file_patching: matches!(tool.tool_id(), "reshade" | "optiscaler"),
        release_support: ToolReleaseSupport::from_supports_releases(tool.supports_releases()),
        status_message,
        env_preview,
        dll_overrides,
        wrapper_preview,
        derived_facts,
        optiscaler_state,
        optiscaler_latest_backup,
        optiscaler_detected_files,
        apply_pending,
        apply_missing_inputs,
        setting_history,
    }
}
