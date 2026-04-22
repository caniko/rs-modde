use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use tracing::{info, warn};

use modde_core::db::ModdeDb;
use modde_core::fs::walk_files_relative;
use modde_core::paths;
use modde_core::profile::ProfileManager;

use super::load_profile_or_default;

/// Run an external tool and capture any new files it writes into the game directory
/// as an "Overwrite" mod.
pub async fn handle_run(
    executable: PathBuf,
    args: Vec<String>,
    profile_name: Option<String>,
    game_id: Option<String>,
) -> Result<()> {
    let pm = ProfileManager::open().context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref())?;

    let game_plugin = modde_games::resolve_game_plugin(&profile.game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{}'", profile.game_id))?;

    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install directory for {}",
            game_plugin.display_name()
        )
    })?;

    let mod_dir = game_plugin.mod_directory(&install_dir);

    // Step 1: Snapshot current state of mod directory
    info!(mod_dir = %mod_dir.display(), "snapshotting mod directory before tool run");
    let before = snapshot_dir(&mod_dir)?;

    // Step 2: Run the tool
    println!("Running: {} {}", executable.display(), args.join(" "));
    let status = Command::new(&executable)
        .args(&args)
        .current_dir(&install_dir)
        .status()
        .with_context(|| format!("failed to execute: {}", executable.display()))?;

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

    // Step 4: Move new files to the Overwrite mod
    let overwrite_dir = paths::store_dir().join("__overwrite__");
    std::fs::create_dir_all(&overwrite_dir)?;

    println!(
        "\nTool wrote {} new file(s). Moving to Overwrite mod:",
        new_files.len()
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

    println!("\nOverwrite mod: {}", overwrite_dir.display());
    println!(
        "Add '__overwrite__' to your profile mod list to include these files in future deploys."
    );

    Ok(())
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
    let mut config = if let Some(row) = db.load_tool_config(game_id, tool_id)? { modde_games::tools::ToolConfig {
        tool_id: row.tool_id,
        enabled: true,
        settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
    } } else {
        let mut cfg = tool.default_config();
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
        .load_tool_config(game_id, tool_id)?.map_or_else(|| "{}".into(), |r| r.settings_json);

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
    let mut config = match db.load_tool_config(game_id, tool_id)? {
        Some(row) => modde_games::tools::ToolConfig {
            tool_id: row.tool_id,
            enabled: row.enabled,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        },
        None => tool.default_config(),
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

    let config = match db.load_tool_config(game_id, tool_id)? {
        Some(row) => modde_games::tools::ToolConfig {
            tool_id: row.tool_id,
            enabled: row.enabled,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        },
        None => tool.default_config(),
    };

    let applied = tool.apply(&install_dir, &config)?;

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

/// Snapshot a directory's file listing (relative paths).
fn snapshot_dir(dir: &Path) -> Result<HashSet<String>> {
    if !dir.exists() {
        return Ok(HashSet::new());
    }

    let files = walk_files_relative(dir)
        .with_context(|| format!("failed to walk directory: {}", dir.display()))?;

    Ok(files.into_iter().map(|(rel, _)| rel).collect())
}
