#![allow(clippy::wildcard_imports)]
use super::*;

pub(crate) async fn apply_tool_for_game(
    db: modde_core::db::ModdeDb,
    game_id: String,
    game_dir: PathBuf,
    tool_id: String,
    context: Option<modde_games::tools::ToolGameContext>,
) -> Result<ToolApplyResult, String> {
    tokio::task::spawn_blocking(move || {
        apply_tool_for_game_blocking(db, game_id, game_dir, tool_id, context)
    })
    .await
    .map_err(|err| err.to_string())?
}

fn apply_tool_for_game_blocking(
    db: modde_core::db::ModdeDb,
    game_id: String,
    game_dir: PathBuf,
    tool_id: String,
    context: Option<modde_games::tools::ToolGameContext>,
) -> Result<ToolApplyResult, String> {
    let typed_game_id = GameId::from(game_id.as_str());
    let tool = modde_games::tools::resolve_tool(&tool_id)
        .ok_or_else(|| format!("Unknown tool: {tool_id}"))?;
    let row = crate::app::block_on(db.load_tool_config(&typed_game_id, &tool_id))
        .map_err(|err| err.to_string())?;
    let mut config = row.map_or_else(
        || tool.default_config_for(context.as_ref()),
        |row| modde_games::tools::ToolConfig {
            tool_id: row.tool_id,
            enabled: row.enabled,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        },
    );
    config.enabled = true;
    config.set("_game_id", serde_json::json!(game_id));
    apply_derived_tool_settings(&mut config, context.as_ref());
    if tool_id == "optiscaler" {
        modde_games::tools::optiscaler::apply_game_defaults(&mut config, context.as_ref());
        modde_games::tools::optiscaler::apply_hardware_defaults(&mut config);
    }

    let applied = tool
        .apply_for(&game_dir, context.as_ref(), &config)
        .map_err(|err| err.to_string())?;
    let paths = applied
        .files
        .iter()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect::<Vec<_>>();
    let validation_message = if tool_id == "optiscaler" {
        validate_optiscaler_apply(&game_id, &game_dir, &config, &applied)?;
        Some("validated managed install".to_string())
    } else {
        None
    };

    if tool_id == "optiscaler" {
        config.set(
            "managed_manifest",
            modde_games::tools::optiscaler::managed_manifest_json(&game_dir, &applied),
        );
    }
    let apply_signature = tool_apply_signature(&config.settings);
    config.set("_last_applied_settings", apply_signature);
    let settings_json = serde_json::to_string(&config.settings).map_err(|err| err.to_string())?;
    crate::app::block_on(db.save_tool_config_with_reason(
        &typed_game_id,
        &tool_id,
        true,
        &settings_json,
        "ui:apply",
    ))
    .map_err(|err| err.to_string())?;
    crate::app::block_on(db.clear_applied_files(&typed_game_id, &tool_id))
        .map_err(|err| err.to_string())?;
    crate::app::block_on(db.save_applied_files(&typed_game_id, &tool_id, &paths))
        .map_err(|err| err.to_string())?;
    crate::app::block_on(modde_games::launcher::generate_tool_configs(
        &typed_game_id,
        &db,
    ))
    .map_err(|err| err.to_string())?;

    Ok(ToolApplyResult {
        display_name: tool.display_name().to_string(),
        applied_file_count: paths.len(),
        validation_message,
    })
}

pub(crate) async fn revert_tool_for_game(
    db: modde_core::db::ModdeDb,
    game_id: String,
    game_dir: PathBuf,
    tool_id: String,
) -> Result<ToolRevertResult, String> {
    tokio::task::spawn_blocking(move || {
        revert_tool_for_game_blocking(db, game_id, game_dir, tool_id)
    })
    .await
    .map_err(|err| err.to_string())?
}

fn revert_tool_for_game_blocking(
    db: modde_core::db::ModdeDb,
    game_id: String,
    game_dir: PathBuf,
    tool_id: String,
) -> Result<ToolRevertResult, String> {
    let typed_game_id = GameId::from(game_id.as_str());
    let tool = modde_games::tools::resolve_tool(&tool_id)
        .ok_or_else(|| format!("Unknown tool: {tool_id}"))?;
    let applied_paths = crate::app::block_on(db.load_applied_files(&typed_game_id, &tool_id))
        .map_err(|err| err.to_string())?;
    let applied = modde_games::tools::AppliedFiles {
        files: applied_paths.iter().map(PathBuf::from).collect(),
    };
    tool.revert(&game_dir, &applied)
        .map_err(|err| err.to_string())?;
    crate::app::block_on(db.clear_applied_files(&typed_game_id, &tool_id))
        .map_err(|err| err.to_string())?;
    crate::app::block_on(modde_games::launcher::generate_tool_configs(
        &typed_game_id,
        &db,
    ))
    .map_err(|err| err.to_string())?;
    Ok(ToolRevertResult {
        display_name: tool.display_name().to_string(),
    })
}

pub(crate) async fn deactivate_optiscaler_for_game(
    db: modde_core::db::ModdeDb,
    game_id: String,
    game_dir: PathBuf,
) -> Result<ToolRevertResult, String> {
    tokio::task::spawn_blocking(move || {
        deactivate_optiscaler_for_game_blocking(db, game_id, game_dir)
    })
    .await
    .map_err(|err| err.to_string())?
}

fn deactivate_optiscaler_for_game_blocking(
    db: modde_core::db::ModdeDb,
    game_id: String,
    game_dir: PathBuf,
) -> Result<ToolRevertResult, String> {
    let typed_game_id = GameId::from(game_id.as_str());
    let tool = modde_games::tools::resolve_tool("optiscaler")
        .ok_or_else(|| "OptiScaler tool is not registered".to_string())?;
    let applied_paths = crate::app::block_on(db.load_applied_files(&typed_game_id, "optiscaler"))
        .map_err(|err| err.to_string())?;
    if !applied_paths.is_empty() {
        let applied = modde_games::tools::AppliedFiles {
            files: applied_paths.iter().map(PathBuf::from).collect(),
        };
        tool.revert(&game_dir, &applied)
            .map_err(|err| err.to_string())?;
        crate::app::block_on(db.clear_applied_files(&typed_game_id, "optiscaler"))
            .map_err(|err| err.to_string())?;
    }

    let settings_json = crate::app::block_on(db.load_tool_config(&typed_game_id, "optiscaler"))
        .map_err(|err| err.to_string())?
        .map_or_else(|| "{}".to_string(), |row| row.settings_json);
    crate::app::block_on(db.save_tool_config_with_reason(
        &typed_game_id,
        "optiscaler",
        false,
        &settings_json,
        "ui:deactivate",
    ))
    .map_err(|err| err.to_string())?;
    crate::app::block_on(modde_games::launcher::generate_tool_configs(
        &typed_game_id,
        &db,
    ))
    .map_err(|err| err.to_string())?;

    Ok(ToolRevertResult {
        display_name: tool.display_name().to_string(),
    })
}

pub(crate) async fn save_executable_for_game(
    db: modde_core::db::ModdeDb,
    row: modde_core::db::ExecutableConfigRow,
) -> Result<String, String> {
    let name = row.name.clone();
    let game_id = row.game_id.clone();
    db.save_executable_config(&row)
        .await
        .map_err(|err| err.to_string())?;
    Ok(format!("Saved executable '{name}' for {game_id}"))
}

pub(crate) async fn remove_executable_for_game(
    db: modde_core::db::ModdeDb,
    game_id: String,
    name: String,
) -> Result<String, String> {
    if db
        .delete_executable_config(&GameId::from(game_id.as_str()), &name)
        .await
        .map_err(|err| err.to_string())?
    {
        Ok(format!("Removed executable '{name}'"))
    } else {
        Err(format!(
            "No executable named '{name}' is configured for {game_id}"
        ))
    }
}

pub(crate) async fn run_saved_executable_for_game(
    db: modde_core::db::ModdeDb,
    game_id: String,
    name: String,
    profile_name: Option<String>,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        run_saved_executable_for_game_blocking(db, game_id, name, profile_name)
    })
    .await
    .map_err(|err| err.to_string())?
}

fn run_saved_executable_for_game_blocking(
    db: modde_core::db::ModdeDb,
    game_id: String,
    name: String,
    profile_name: Option<String>,
) -> Result<String, String> {
    let row =
        crate::app::block_on(db.load_executable_config(&GameId::from(game_id.as_str()), &name))
            .map_err(|err| err.to_string())?
            .ok_or_else(|| format!("No executable named '{name}' is configured for {game_id}"))?;
    run_executable_row_blocking(db, row, profile_name)
}

pub(crate) fn run_executable_row_blocking(
    db: modde_core::db::ModdeDb,
    row: modde_core::db::ExecutableConfigRow,
    profile_name: Option<String>,
) -> Result<String, String> {
    let pm = ProfileManager::with_db(db);
    let row_game_id = GameId::from(row.game_id.as_str());
    let profile = if let Some(profile_name) = profile_name {
        crate::app::block_on(pm.load(&profile_name, Some(&row_game_id)))
            .map_err(|err| err.to_string())?
    } else {
        let summaries = crate::app::block_on(pm.list()).map_err(|err| err.to_string())?;
        let first = summaries
            .iter()
            .find(|profile| profile.game_id.as_str() == row.game_id)
            .ok_or_else(|| format!("No profile found for {}", row.game_id))?;
        crate::app::block_on(pm.load(&first.name, Some(&row_game_id)))
            .map_err(|err| err.to_string())?
    };
    let game_plugin = modde_games::resolve_game_plugin(profile.game_id.as_str())
        .ok_or_else(|| format!("Unsupported game: {}", profile.game_id))?;
    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        format!(
            "Could not detect install directory for {}",
            game_plugin.display_name()
        )
    })?;
    let mod_dir = game_plugin
        .mod_root(&install_dir)
        .map_err(|err| err.to_string())?;
    let before = snapshot_dir_for_executable(&mod_dir)?;
    let args: Vec<String> = serde_json::from_str(&row.arguments_json)
        .map_err(|err| format!("Stored arguments are invalid JSON: {err}"))?;
    let environment: HashMap<String, String> = serde_json::from_str(&row.environment_json)
        .map_err(|err| format!("Stored environment is invalid JSON: {err}"))?;
    let working_dir = row.working_dir.as_ref().unwrap_or(&install_dir);
    let mut command = std::process::Command::new(&row.executable_path);
    command.args(args).current_dir(working_dir);
    for (key, value) in environment {
        command.env(key, value);
    }
    if let Some(overrides) = &row.wine_dll_overrides {
        command.env("WINEDLLOVERRIDES", overrides);
    }
    let status = command
        .status()
        .map_err(|err| format!("Failed to execute {}: {err}", row.executable_path.display()))?;
    let after = snapshot_dir_for_executable(&mod_dir)?;
    let new_files = after.difference(&before).cloned().collect::<Vec<_>>();
    if !new_files.is_empty() {
        let output_dir = modde_core::paths::store_dir().join(&row.output_mod);
        std::fs::create_dir_all(&output_dir).map_err(|err| err.to_string())?;
        for rel_path in &new_files {
            let src = mod_dir.join(rel_path);
            let dst = output_dir.join(rel_path);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
            }
            std::fs::rename(&src, &dst)
                .or_else(|_| {
                    std::fs::copy(&src, &dst)?;
                    std::fs::remove_file(&src)
                })
                .map_err(|err| format!("Failed to move {rel_path} to output mod: {err}"))?;
        }
    }
    let suffix = if status.success() {
        String::new()
    } else {
        format!("; process exited with status {status}")
    };
    Ok(format!(
        "Ran '{}' and captured {} file(s) to {}{}",
        row.name,
        new_files.len(),
        row.output_mod,
        suffix
    ))
}

pub(crate) fn snapshot_dir_for_executable(dir: &Path) -> Result<HashSet<String>, String> {
    if !dir.exists() {
        return Ok(HashSet::new());
    }
    modde_core::fs::walk_files_relative(dir)
        .map(|files| files.into_iter().map(|(rel, _)| rel).collect())
        .map_err(|err| err.to_string())
}

pub fn parse_executable_environment(input: &str) -> Result<HashMap<String, String>, String> {
    let mut env = HashMap::new();
    for (idx, line) in input.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("Environment line {} must be KEY=VALUE", idx + 1));
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(format!("Environment line {} has an empty key", idx + 1));
        }
        env.insert(key.to_string(), value.trim().to_string());
    }
    Ok(env)
}

pub(crate) fn executable_draft_to_row(
    game_id: &str,
    draft: &ExecutableDraft,
) -> Result<modde_core::db::ExecutableConfigRow, String> {
    let name = draft.name.trim();
    if name.is_empty() {
        return Err("Executable name is required".to_string());
    }
    let executable_path = draft.executable_path.trim();
    if executable_path.is_empty() {
        return Err("Executable path is required".to_string());
    }
    let output_mod = if draft.output_mod.trim().is_empty() {
        "__overwrite__"
    } else {
        draft.output_mod.trim()
    };
    let args = draft
        .arguments
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let env = parse_executable_environment(&draft.environment)?;
    Ok(modde_core::db::ExecutableConfigRow {
        game_id: game_id.to_string(),
        name: name.to_string(),
        executable_path: PathBuf::from(executable_path),
        arguments_json: serde_json::to_string(&args).map_err(|err| err.to_string())?,
        working_dir: (!draft.working_dir.trim().is_empty())
            .then(|| PathBuf::from(draft.working_dir.trim())),
        environment_json: serde_json::to_string(&env).map_err(|err| err.to_string())?,
        wine_dll_overrides: (!draft.wine_dll_overrides.trim().is_empty())
            .then(|| draft.wine_dll_overrides.trim().to_string()),
        output_mod: output_mod.to_string(),
        enabled: true,
    })
}

pub(crate) fn validate_optiscaler_apply(
    game_id: &str,
    game_dir: &std::path::Path,
    config: &modde_games::tools::ToolConfig,
    applied: &modde_games::tools::AppliedFiles,
) -> Result<(), String> {
    let managed_paths = applied
        .files
        .iter()
        .map(|path| {
            path.to_string_lossy()
                .replace('\\', "/")
                .to_ascii_lowercase()
        })
        .collect::<BTreeSet<_>>();
    let state =
        modde_games::tools::optiscaler::scan_optiscaler_install(game_id, game_dir, &managed_paths)
            .map_err(|err| err.to_string())?;
    if !matches!(
        state.status,
        modde_games::tools::optiscaler::OptiScalerInstallStatus::Managed
            | modde_games::tools::optiscaler::OptiScalerInstallStatus::PartiallyManaged
    ) {
        return Err(format!(
            "OptiScaler validation failed: install is {} after apply",
            state.status
        ));
    }
    let proxy_dll = config
        .get_str("proxy_dll")
        .or_else(|| config.get_str("dll_name"))
        .unwrap_or("dxgi.dll");
    if !state
        .proxy_dlls
        .iter()
        .any(|dll| dll.eq_ignore_ascii_case(proxy_dll))
    {
        return Err(format!(
            "OptiScaler validation failed: missing configured proxy DLL {proxy_dll}"
        ));
    }
    if state.config_path.is_none() {
        return Err("OptiScaler validation failed: missing OptiScaler.ini".to_string());
    }
    Ok(())
}
