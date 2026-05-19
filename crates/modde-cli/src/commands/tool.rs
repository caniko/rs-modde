use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use tracing::{info, warn};

use modde_core::db::{ExecutableConfigRow, ModdeDb};
use modde_core::fs::walk_files_relative;
use modde_core::paths;
use modde_core::profile::ProfileManager;

use super::load_profile_or_default;

struct ExternalToolRun {
    executable: PathBuf,
    args: Vec<String>,
    profile_name: Option<String>,
    game_id: Option<String>,
    working_dir: Option<PathBuf>,
    environment: HashMap<String, String>,
    wine_dll_overrides: Option<String>,
    output_mod: String,
}

/// Run an external tool and capture any new files it writes into the game directory
/// as an "Overwrite" mod.
pub async fn handle_run(
    executable: PathBuf,
    args: Vec<String>,
    profile_name: Option<String>,
    game_id: Option<String>,
) -> Result<()> {
    run_external_tool(ExternalToolRun {
        executable,
        args,
        profile_name,
        game_id,
        working_dir: None,
        environment: HashMap::new(),
        wine_dll_overrides: None,
        output_mod: "__overwrite__".to_string(),
    })
    .await
}

async fn run_external_tool(options: ExternalToolRun) -> Result<()> {
    let pm = ProfileManager::open().context("failed to open profile database")?;
    let profile = load_profile_or_default(
        &pm,
        options.profile_name.as_deref(),
        options.game_id.as_deref(),
    )?;

    let game_plugin = modde_games::resolve_game_plugin(&profile.game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{}'", profile.game_id))?;

    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install directory for {}",
            game_plugin.display_name()
        )
    })?;

    let mod_dir = game_plugin
        .mod_root(&install_dir)
        .context("failed to resolve game mod root")?;

    // Step 1: Snapshot current state of mod directory
    info!(mod_dir = %mod_dir.display(), "snapshotting mod directory before tool run");
    let before = snapshot_dir(&mod_dir)?;

    // Step 2: Run the tool
    let working_dir = options.working_dir.as_ref().unwrap_or(&install_dir);
    let mut command = Command::new(&options.executable);
    command.args(&options.args).current_dir(working_dir);
    for (key, value) in &options.environment {
        command.env(key, value);
    }
    if let Some(overrides) = &options.wine_dll_overrides {
        command.env("WINEDLLOVERRIDES", overrides);
    }

    println!(
        "Running: {} {}",
        options.executable.display(),
        options.args.join(" ")
    );
    println!("Working directory: {}", working_dir.display());
    if !options.environment.is_empty() || options.wine_dll_overrides.is_some() {
        println!("Environment overrides:");
        for key in options.environment.keys() {
            println!("  {key}=<configured>");
        }
        if let Some(overrides) = &options.wine_dll_overrides {
            println!("  WINEDLLOVERRIDES={overrides}");
        }
    }

    let status = command
        .status()
        .with_context(|| format!("failed to execute: {}", options.executable.display()))?;

    if !status.success() {
        warn!(
            exit_code = status.code(),
            "tool exited with non-zero status"
        );
    }

    // Step 3: Diff to find new files
    let after = snapshot_dir(&mod_dir)?;
    let new_files: Vec<String> = after.difference(&before).cloned().collect();

    if new_files.is_empty() {
        println!("Tool completed. No new files written to mod directory.");
        return Ok(());
    }

    // Step 4: Move new files to the configured output mod
    let overwrite_dir = paths::store_dir().join(&options.output_mod);
    std::fs::create_dir_all(&overwrite_dir)?;

    println!(
        "\nTool wrote {} new file(s). Moving to output mod '{}':",
        new_files.len(),
        options.output_mod
    );

    for rel_path in &new_files {
        let src = mod_dir.join(rel_path);
        let dst = overwrite_dir.join(rel_path);

        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::rename(&src, &dst)
            .or_else(|_| {
                // Cross-device: copy + remove
                std::fs::copy(&src, &dst)?;
                std::fs::remove_file(&src)
            })
            .with_context(|| format!("failed to move {rel_path} to overwrite"))?;

        println!("  {rel_path}");
    }

    println!("\nOutput mod: {}", overwrite_dir.display());
    println!(
        "Add '{}' to your profile mod list to include these files in future deploys.",
        options.output_mod
    );

    Ok(())
}

/// Save a named executable launch target.
pub fn handle_add_executable(
    name: &str,
    executable: PathBuf,
    game_id: &str,
    working_dir: Option<PathBuf>,
    output_mod: &str,
    wine_dll_overrides: Option<String>,
    environment: &[String],
    args: &[String],
) -> Result<()> {
    if modde_games::resolve_game_plugin(game_id).is_none() {
        anyhow::bail!(
            "unsupported game: '{game_id}'. Supported games: {}",
            modde_games::supported_game_ids().join(", ")
        );
    }
    if name.trim().is_empty() {
        anyhow::bail!("executable name cannot be empty");
    }
    if output_mod.trim().is_empty() {
        anyhow::bail!("output mod cannot be empty");
    }

    let env_map = parse_environment(environment)?;
    let db = ModdeDb::open().context("failed to open database")?;
    let row = ExecutableConfigRow {
        game_id: game_id.to_string(),
        name: name.to_string(),
        executable_path: executable,
        arguments_json: serde_json::to_string(args)?,
        working_dir,
        environment_json: serde_json::to_string(&env_map)?,
        wine_dll_overrides,
        output_mod: output_mod.to_string(),
        enabled: true,
    };
    db.save_executable_config(&row)?;

    println!("Saved executable '{}' for {game_id}", row.name);
    println!("  Path: {}", row.executable_path.display());
    println!("  Output mod: {}", row.output_mod);
    Ok(())
}

/// List saved executable launch targets for a game.
pub fn handle_list_executables(game_id: &str) -> Result<()> {
    let db = ModdeDb::open().context("failed to open database")?;
    let rows = db.load_executable_configs(game_id)?;

    println!("Executables for {game_id}:");
    if rows.is_empty() {
        println!("  (none configured)");
        return Ok(());
    }

    for row in rows {
        let args: Vec<String> = serde_json::from_str(&row.arguments_json).unwrap_or_default();
        println!("  {}", row.name);
        println!("    path: {}", row.executable_path.display());
        if !args.is_empty() {
            println!("    args: {}", args.join(" "));
        }
        if let Some(working_dir) = row.working_dir {
            println!("    working dir: {}", working_dir.display());
        }
        if let Some(overrides) = row.wine_dll_overrides {
            println!("    WINEDLLOVERRIDES: {overrides}");
        }
        println!("    output mod: {}", row.output_mod);
    }

    Ok(())
}

/// Remove a saved executable launch target.
pub fn handle_remove_executable(name: &str, game_id: &str) -> Result<()> {
    let db = ModdeDb::open().context("failed to open database")?;
    if db.delete_executable_config(game_id, name)? {
        println!("Removed executable '{name}' for {game_id}");
    } else {
        anyhow::bail!("no executable named '{name}' is configured for {game_id}");
    }
    Ok(())
}

/// Run a saved executable launch target.
pub async fn handle_run_executable(
    name: &str,
    game_id: &str,
    profile_name: Option<String>,
    extra_args: Vec<String>,
) -> Result<()> {
    let db = ModdeDb::open().context("failed to open database")?;
    let row = db.load_executable_config(game_id, name)?.ok_or_else(|| {
        anyhow::anyhow!("no executable named '{name}' is configured for {game_id}")
    })?;
    if !row.enabled {
        anyhow::bail!("executable '{name}' is disabled for {game_id}");
    }

    let mut args: Vec<String> = serde_json::from_str(&row.arguments_json)
        .with_context(|| format!("stored arguments for '{name}' are invalid JSON"))?;
    args.extend(extra_args);
    let environment: HashMap<String, String> = serde_json::from_str(&row.environment_json)
        .with_context(|| format!("stored environment for '{name}' is invalid JSON"))?;

    run_external_tool(ExternalToolRun {
        executable: row.executable_path,
        args,
        profile_name,
        game_id: Some(game_id.to_string()),
        working_dir: row.working_dir,
        environment,
        wine_dll_overrides: row.wine_dll_overrides,
        output_mod: row.output_mod,
    })
    .await
}

fn parse_environment(settings: &[String]) -> Result<HashMap<String, String>> {
    let mut env = HashMap::new();
    for setting in settings {
        let (key, value) = setting.split_once('=').ok_or_else(|| {
            anyhow::anyhow!("invalid env format: '{setting}' (expected KEY=VALUE)")
        })?;
        if key.is_empty() {
            anyhow::bail!("invalid env format: '{setting}' (empty key)");
        }
        env.insert(key.to_string(), value.to_string());
    }
    Ok(env)
}

/// List known tools for a game.
pub fn handle_list(game_id: &str) -> Result<()> {
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
pub fn handle_status(game_id: &str) -> Result<()> {
    let db = ModdeDb::open().context("failed to open database")?;
    let stored = db.load_tool_configs(game_id)?;
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
            .load_applied_files(game_id, tool.tool_id())
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
pub fn handle_enable(tool_id: &str, game_id: &str) -> Result<()> {
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

    let db = ModdeDb::open().context("failed to open database")?;

    // Load existing or use defaults
    let context = modde_games::resolve_game_plugin(game_id).map(|plugin| {
        modde_games::tools::ToolGameContext::from_parts(game_id, plugin.display_name(), None, None)
    });
    let mut config = if let Some(row) = db.load_tool_config(game_id, tool_id)? {
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
    db.save_tool_config(game_id, tool_id, true, &settings_json)?;

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
pub fn handle_disable(tool_id: &str, game_id: &str) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| anyhow::anyhow!("unknown tool: '{tool_id}'"))?;

    let db = ModdeDb::open().context("failed to open database")?;

    // Load existing config to preserve settings
    let settings_json = db
        .load_tool_config(game_id, tool_id)?
        .map_or_else(|| "{}".into(), |r| r.settings_json);

    db.save_tool_config(game_id, tool_id, false, &settings_json)?;

    println!("Disabled {} for {game_id}", tool.display_name());

    Ok(())
}

/// Configure a tool's settings.
pub fn handle_configure(tool_id: &str, game_id: &str, settings: &[String]) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| anyhow::anyhow!("unknown tool: '{tool_id}'"))?;

    let db = ModdeDb::open().context("failed to open database")?;

    // Load existing or defaults
    let context = modde_games::resolve_game_plugin(game_id).map(|plugin| {
        modde_games::tools::ToolGameContext::from_parts(game_id, plugin.display_name(), None, None)
    });
    let mut config = match db.load_tool_config(game_id, tool_id)? {
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
    db.save_tool_config(game_id, tool_id, config.enabled, &settings_json)?;

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
pub fn handle_apply(tool_id: &str, game_id: &str) -> Result<()> {
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

    let db = ModdeDb::open().context("failed to open database")?;

    let context = modde_games::tools::ToolGameContext::from_parts(
        game_id,
        game_plugin.display_name(),
        Some(install_dir.clone()),
        None,
    );
    let mut config = match db.load_tool_config(game_id, tool_id)? {
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

    db.save_applied_files(game_id, tool_id, &rel_paths)?;
    if tool_id == "optiscaler" {
        let mut updated_config = config.clone();
        updated_config.set(
            "managed_manifest",
            modde_games::tools::optiscaler::managed_manifest_json(&install_dir, &applied),
        );
        let settings_json = serde_json::to_string(&updated_config.settings)?;
        db.save_tool_config(game_id, tool_id, true, &settings_json)?;
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

/// Revert tool patches from the game directory.
pub fn handle_revert(tool_id: &str, game_id: &str) -> Result<()> {
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

    let db = ModdeDb::open().context("failed to open database")?;

    let files = db.load_applied_files(game_id, tool_id)?;
    if files.is_empty() {
        println!("No applied files to revert for {}", tool.display_name());
        return Ok(());
    }

    let applied = modde_games::tools::AppliedFiles {
        files: files.iter().map(PathBuf::from).collect(),
    };

    tool.revert(&install_dir, &applied)?;
    db.clear_applied_files(game_id, tool_id)?;

    println!(
        "Reverted {} ({} files) from {}",
        tool.display_name(),
        files.len(),
        install_dir.display(),
    );

    Ok(())
}

/// List releases for a release-backed tool.
pub async fn handle_releases(tool_id: &str, game_id: &str) -> Result<()> {
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
    if !tool.supports_releases() {
        anyhow::bail!("{} does not support release selection", tool.display_name());
    }

    let db = ModdeDb::open().context("failed to open database")?;
    let current = load_tool_config_or_default(&db, game_id, tool_id, tool)?;
    let current_tag = current.get_str("release_tag").unwrap_or("latest");
    let current_asset = current.get_str("release_asset").unwrap_or("");
    let releases = tool.list_releases().await?;

    if releases.is_empty() {
        println!("No releases returned for {}", tool.display_name());
        return Ok(());
    }

    println!(
        "{} releases for {game_id} (selected: {} / {})",
        tool.display_name(),
        current_tag,
        if current_asset.is_empty() {
            "(no asset)"
        } else {
            current_asset
        }
    );
    for release in &releases {
        let assets = tool.installable_release_assets(release);
        if assets.is_empty() {
            println!("  {}: no installable assets", release.tag);
        } else {
            println!("  {}: {}", release.tag, assets.join(", "));
        }
    }
    Ok(())
}

/// Install a specific release asset for a release-backed tool.
pub async fn handle_install_release(
    tool_id: &str,
    game_id: &str,
    tag: &str,
    asset: &str,
) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| anyhow::anyhow!("unknown tool: '{tool_id}'"))?;
    if !tool.supports_releases() {
        anyhow::bail!("{} does not support release selection", tool.display_name());
    }

    let db = ModdeDb::open().context("failed to open database")?;
    let config = load_tool_config_or_default(&db, game_id, tool_id, tool)?;
    let config = tool.install_release(game_id, config, tag, asset).await?;
    let settings_json = serde_json::to_string(&config.settings)?;
    db.save_tool_config(game_id, tool_id, config.enabled, &settings_json)?;

    println!(
        "Installed {} {} ({}) for {}",
        tool.display_name(),
        tag,
        asset,
        game_id
    );
    Ok(())
}

/// Install a specific release asset for a release-backed tool from a local path.
pub async fn handle_install_release_from_path(
    tool_id: &str,
    game_id: &str,
    tag: &str,
    asset: &str,
    path: PathBuf,
) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| anyhow::anyhow!("unknown tool: '{tool_id}'"))?;
    if !tool.supports_releases() {
        anyhow::bail!("{} does not support release selection", tool.display_name());
    }

    let db = ModdeDb::open().context("failed to open database")?;
    let config = load_tool_config_or_default(&db, game_id, tool_id, tool)?;
    let config = tool
        .install_release_from_path(game_id, config, tag, asset, path)
        .await?;
    let settings_json = serde_json::to_string(&config.settings)?;
    db.save_tool_config(game_id, tool_id, config.enabled, &settings_json)?;

    println!(
        "Installed {} {} ({}) for {}",
        tool.display_name(),
        tag,
        asset,
        game_id
    );
    Ok(())
}

fn load_tool_config_or_default(
    db: &ModdeDb,
    game_id: &str,
    tool_id: &str,
    tool: &dyn modde_games::tools::GameTool,
) -> Result<modde_games::tools::ToolConfig> {
    let context = modde_games::resolve_game_plugin(game_id).map(|plugin| {
        modde_games::tools::ToolGameContext::from_parts(game_id, plugin.display_name(), None, None)
    });
    Ok(match db.load_tool_config(game_id, tool_id)? {
        Some(row) => modde_games::tools::ToolConfig {
            tool_id: row.tool_id,
            enabled: row.enabled,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        },
        None => tool.default_config_for(context.as_ref()),
    })
}

/// Snapshot a directory's file listing (relative paths).
fn snapshot_dir(dir: &Path) -> Result<HashSet<String>> {
    if !dir.exists() {
        return Ok(HashSet::new());
    }

    let files = walk_files_relative(dir)
        .with_context(|| format!("failed to walk directory: {}", dir.display()))?;

    Ok(files.into_iter().map(|(rel, _)| rel).collect())
}
