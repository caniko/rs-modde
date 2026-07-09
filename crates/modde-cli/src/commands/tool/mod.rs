use std::path::PathBuf;

use anyhow::{Context, Result};

use modde_core::db::ModdeDb;
use modde_core::resolver::GameId;

mod exec;
mod releases;

pub use exec::{
    handle_add_executable, handle_list_executables, handle_remove_executable, handle_run,
    handle_run_executable,
};
pub use releases::{handle_install_release, handle_install_release_from_path, handle_releases};

pub async fn handle_list(game_id: &str) -> Result<()> {
    let game_plugin = modde_games::resolve_game_plugin(game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{game_id}'"))?;

    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install directory for {}",
            game_plugin.display_name()
        )
    })?;

    println!("Game: {} ({})", game_plugin.display_name(), game_id);
    println!("Install: {}", install_dir.display());
    println!("\nDetected tools:");

    // Scan for common tool executables
    let common_tools = [
        ("xEdit", &["SSEEdit.exe", "FO4Edit.exe", "xEdit.exe"][..]),
        ("FNIS", &["GenerateFNISforUsers.exe"]),
        ("Nemesis", &["Nemesis Unlimited Behavior Engine.exe"]),
        ("BodySlide", &["BodySlide.exe", "BodySlide x64.exe"]),
        ("Creation Kit", &["CreationKit.exe"]),
        ("LOOT", &["LOOT.exe"]),
        ("zEdit", &["zEdit.exe"]),
    ];

    let mut found = false;
    for (name, executables) in &common_tools {
        for exe in *executables {
            let path = install_dir.join(exe);
            if path.exists() {
                println!("  {name}: {}", path.display());
                found = true;
            }
        }
    }

    if !found {
        println!("  (none detected)");
    }

    println!("\nRun tools with: modde tool run <executable> [-- args...]");

    Ok(())
}

// ── Gaming tool/overlay management ──────────────────────────────────────

/// Show status of all gaming tools for a game.
pub async fn handle_status(game_id: &str) -> Result<()> {
    let db = ModdeDb::open().await.context("failed to open database")?;
    let stored = db.load_tool_configs(&GameId::from(game_id)).await?;
    let game_plugin = modde_games::resolve_game_plugin(game_id);
    let install_dir = game_plugin.and_then(modde_games::GamePlugin::detect_install);

    println!("Game: {game_id}\n");
    println!(
        "{:<14} {:<14} {:<10} Available",
        "Tool", "Category", "Status"
    );
    println!("{}", "-".repeat(60));

    for tool in modde_games::tools::all_tools() {
        let avail = tool.detect_available();
        let avail_str = match &avail {
            modde_games::tools::ToolAvailability::Available { version } => match version {
                Some(v) => format!("yes ({v})"),
                None => "yes".into(),
            },
            modde_games::tools::ToolAvailability::NotInstalled { .. } => "not installed".into(),
        };

        let enabled = stored
            .iter()
            .find(|r| r.tool_id == tool.tool_id())
            .is_some_and(|r| r.enabled);

        let status = if enabled { "enabled" } else { "disabled" };

        // Check if files are applied
        let applied_count = db
            .load_applied_files(&GameId::from(game_id), tool.tool_id())
            .await
            .map_or(0, |f| f.len());

        let status_str = if applied_count > 0 {
            format!("{status} ({applied_count} files)")
        } else {
            status.to_string()
        };

        println!(
            "{:<14} {:<14} {:<10} {}",
            tool.display_name(),
            tool.category(),
            status_str,
            avail_str,
        );

        if tool.tool_id() == "optiscaler" {
            let context = install_dir.as_ref().map(|install_dir| {
                modde_games::tools::ToolGameContext::from_parts(
                    game_id,
                    game_plugin
                        .map(modde_games::GamePlugin::display_name)
                        .unwrap_or(game_id),
                    Some(install_dir.clone()),
                    None,
                )
            });
            let config = stored
                .iter()
                .find(|r| r.tool_id == "optiscaler")
                .map_or_else(
                    || tool.default_config_for(context.as_ref()),
                    |row| modde_games::tools::ToolConfig {
                        tool_id: row.tool_id.clone(),
                        enabled: row.enabled,
                        settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
                    },
                );
            let managed = modde_games::tools::optiscaler::managed_paths_from_config(&config);
            if let Some(install_dir) = &install_dir
                && let Ok(state) = modde_games::tools::optiscaler::scan_optiscaler_install(
                    game_id,
                    install_dir,
                    &managed,
                )
            {
                println!("  OptiScaler install: {}", state.summary());
                println!("  Executable dir: {}", state.executable_dir.display());
                if let Some(path) = &state.config_path {
                    println!(
                        "  Config: {} ({} parsed setting(s))",
                        path.display(),
                        state.ini_settings.len()
                    );
                }
                if !state.wine_dll_overrides.is_empty() {
                    println!("  Wine overrides: {}", state.wine_dll_overrides.join(", "));
                }
                if let Some(path) = &state.latest_backup {
                    println!("  Latest backup: {}", path.display());
                }
            }
        }
    }

    Ok(())
}

/// Enable a tool for a game.
pub async fn handle_enable(tool_id: &str, game_id: &str) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown tool: '{tool_id}'\nAvailable: {}",
            modde_games::tools::all_tools()
                .iter()
                .map(|t| t.tool_id())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;

    let db = ModdeDb::open().await.context("failed to open database")?;

    // Load existing or use defaults
    let context = modde_games::resolve_game_plugin(game_id).map(|plugin| {
        modde_games::tools::ToolGameContext::from_parts(game_id, plugin.display_name(), None, None)
    });
    let mut config = if let Some(row) = db.load_tool_config(&GameId::from(game_id), tool_id).await?
    {
        modde_games::tools::ToolConfig {
            tool_id: row.tool_id,
            enabled: true,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        }
    } else {
        let mut cfg = tool.default_config_for(context.as_ref());
        cfg.enabled = true;
        cfg
    };

    config.enabled = true;

    let settings_json = serde_json::to_string(&config.settings)?;
    db.save_tool_config(&GameId::from(game_id), tool_id, true, &settings_json)
        .await?;

    println!("Enabled {} for {game_id}", tool.display_name());

    // Generate config file if applicable
    config.set("_game_id", serde_json::json!(game_id));
    if let Some(generated) = tool.generate_config(&config) {
        if let Some(parent) = generated.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&generated.path, &generated.content)?;
        println!("  Config: {}", generated.path.display());
    }

    Ok(())
}

/// Disable a tool for a game.
pub async fn handle_disable(tool_id: &str, game_id: &str) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| anyhow::anyhow!("unknown tool: '{tool_id}'"))?;

    let db = ModdeDb::open().await.context("failed to open database")?;

    // Load existing config to preserve settings
    let settings_json = db
        .load_tool_config(&GameId::from(game_id), tool_id)
        .await?
        .map_or_else(|| "{}".into(), |r| r.settings_json);

    db.save_tool_config(&GameId::from(game_id), tool_id, false, &settings_json)
        .await?;

    println!("Disabled {} for {game_id}", tool.display_name());

    Ok(())
}

/// Configure a tool's settings.
pub async fn handle_configure(tool_id: &str, game_id: &str, settings: &[String]) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| anyhow::anyhow!("unknown tool: '{tool_id}'"))?;

    let db = ModdeDb::open().await.context("failed to open database")?;

    // Load existing or defaults
    let context = modde_games::resolve_game_plugin(game_id).map(|plugin| {
        modde_games::tools::ToolGameContext::from_parts(game_id, plugin.display_name(), None, None)
    });
    let mut config = match db.load_tool_config(&GameId::from(game_id), tool_id).await? {
        Some(row) => modde_games::tools::ToolConfig {
            tool_id: row.tool_id,
            enabled: row.enabled,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        },
        None => tool.default_config_for(context.as_ref()),
    };

    // Parse key=value pairs
    for setting in settings {
        let (key, value) = setting.split_once('=').ok_or_else(|| {
            anyhow::anyhow!("invalid setting format: '{setting}' (expected key=value)")
        })?;

        // Try to parse as bool, number, or fallback to string
        let json_value = if value == "true" {
            serde_json::json!(true)
        } else if value == "false" {
            serde_json::json!(false)
        } else if let Ok(n) = value.parse::<f64>() {
            serde_json::json!(n)
        } else {
            serde_json::json!(value)
        };

        config.set(key, json_value);
        if tool_id == "optiscaler" && key == "optiscaler_profile" {
            modde_games::tools::optiscaler::apply_profile_by_id(&mut config, game_id, value);
        }
        println!("  {key} = {value}");
    }

    let settings_json = serde_json::to_string(&config.settings)?;
    db.save_tool_config(
        &GameId::from(game_id),
        tool_id,
        config.enabled,
        &settings_json,
    )
    .await?;

    // Regenerate config file
    config.set("_game_id", serde_json::json!(game_id));
    if let Some(generated) = tool.generate_config(&config) {
        if let Some(parent) = generated.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&generated.path, &generated.content)?;
        println!("  Config written: {}", generated.path.display());
    }

    println!("Updated {} config for {game_id}", tool.display_name());

    Ok(())
}

/// Apply tool patches to the game directory.
pub async fn handle_apply(tool_id: &str, game_id: &str) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| anyhow::anyhow!("unknown tool: '{tool_id}'"))?;

    let game_plugin = modde_games::resolve_game_plugin(game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{game_id}'"))?;

    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install dir for {}",
            game_plugin.display_name()
        )
    })?;

    let db = ModdeDb::open().await.context("failed to open database")?;

    let context = modde_games::tools::ToolGameContext::from_parts(
        game_id,
        game_plugin.display_name(),
        Some(install_dir.clone()),
        None,
    );
    let mut config = match db.load_tool_config(&GameId::from(game_id), tool_id).await? {
        Some(row) => modde_games::tools::ToolConfig {
            tool_id: row.tool_id,
            enabled: row.enabled,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        },
        None => tool.default_config_for(Some(&context)),
    };
    if tool_id == "optiscaler" {
        modde_games::tools::optiscaler::apply_game_defaults(&mut config, Some(&context));
    }

    let applied = tool.apply_for(&install_dir, Some(&context), &config)?;

    if applied.files.is_empty() {
        println!("No files to apply for {}", tool.display_name());
        return Ok(());
    }

    // Record applied files in the database
    let rel_paths: Vec<String> = applied
        .files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    db.save_applied_files(&GameId::from(game_id), tool_id, &rel_paths)
        .await?;
    if tool_id == "optiscaler" {
        let mut updated_config = config.clone();
        updated_config.set(
            "managed_manifest",
            modde_games::tools::optiscaler::managed_manifest_json(&install_dir, &applied),
        );
        let settings_json = serde_json::to_string(&updated_config.settings)?;
        db.save_tool_config(&GameId::from(game_id), tool_id, true, &settings_json)
            .await?;
    }

    println!(
        "Applied {} ({} files) to {}",
        tool.display_name(),
        applied.files.len(),
        install_dir.display(),
    );
    for f in &applied.files {
        println!("  {}", f.display());
    }

    Ok(())
}

/// Preview tool patches without writing to the game directory.
pub async fn handle_preview(tool_id: &str, game_id: &str) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| anyhow::anyhow!("unknown tool: '{tool_id}'"))?;

    let game_plugin = modde_games::resolve_game_plugin(game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{game_id}'"))?;

    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install dir for {}",
            game_plugin.display_name()
        )
    })?;

    let db = ModdeDb::open().await.context("failed to open database")?;
    let context = modde_games::tools::ToolGameContext::from_parts(
        game_id,
        game_plugin.display_name(),
        Some(install_dir.clone()),
        None,
    );
    let mut config = match db.load_tool_config(&GameId::from(game_id), tool_id).await? {
        Some(row) => modde_games::tools::ToolConfig {
            tool_id: row.tool_id,
            enabled: row.enabled,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        },
        None => tool.default_config_for(Some(&context)),
    };

    if tool_id == "optiscaler" {
        modde_games::tools::optiscaler::apply_game_defaults(&mut config, Some(&context));
    }

    let preview = tool.preview_apply_for(&install_dir, Some(&context), &config)?;

    println!(
        "Preview {} for {} at {}",
        tool.display_name(),
        game_id,
        install_dir.display()
    );
    println!("  Planned files: {}", preview.planned_files.len());
    println!("  Changed files: {}", preview.changed_files.len());
    println!("  Unchanged files: {}", preview.unchanged_files.len());

    if !preview.missing_inputs.is_empty() {
        println!("\nMissing inputs:");
        for input in &preview.missing_inputs {
            println!("  {input}");
        }
    }
    if !preview.changed_files.is_empty() {
        println!("\nWould write/update:");
        for path in &preview.changed_files {
            println!("  {}", path.display());
        }
    }
    if !preview.unchanged_files.is_empty() {
        println!("\nAlready current:");
        for path in &preview.unchanged_files {
            println!("  {}", path.display());
        }
    }

    if tool_id == "optiscaler" {
        let mut effective_config = config.clone();
        modde_games::tools::optiscaler::apply_hardware_defaults(&mut effective_config);
        println!("\nEffective OptiScaler settings:");
        for key in [
            "optiscaler_profile",
            "hardware_tuning",
            "fsr4_variant",
            "emulate_fp8",
        ] {
            if let Some(value) = effective_config.settings.get(key) {
                println!("  {key} = {value}");
            }
        }
    }

    let env = tool.env_vars(&config);
    if !env.is_empty() {
        println!("\nEnvironment preview:");
        for (key, value) in env {
            println!("  {key}={value}");
        }
    }

    Ok(())
}

/// Revert tool patches from the game directory.
pub async fn handle_revert(tool_id: &str, game_id: &str) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| anyhow::anyhow!("unknown tool: '{tool_id}'"))?;

    let game_plugin = modde_games::resolve_game_plugin(game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{game_id}'"))?;

    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install dir for {}",
            game_plugin.display_name()
        )
    })?;

    let db = ModdeDb::open().await.context("failed to open database")?;

    let files = db
        .load_applied_files(&GameId::from(game_id), tool_id)
        .await?;
    if files.is_empty() {
        println!("No applied files to revert for {}", tool.display_name());
        return Ok(());
    }

    let applied = modde_games::tools::AppliedFiles {
        files: files.iter().map(PathBuf::from).collect(),
    };

    tool.revert(&install_dir, &applied)?;
    db.clear_applied_files(&GameId::from(game_id), tool_id)
        .await?;

    println!(
        "Reverted {} ({} files) from {}",
        tool.display_name(),
        files.len(),
        install_dir.display(),
    );

    Ok(())
}
