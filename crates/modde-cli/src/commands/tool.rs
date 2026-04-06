use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use tracing::{info, warn};

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
    let new_files: Vec<String> = after
        .difference(&before)
        .cloned()
        .collect();

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
            .with_context(|| format!("failed to move {} to overwrite", rel_path))?;

        println!("  {rel_path}");
    }

    println!(
        "\nOverwrite mod: {}",
        overwrite_dir.display()
    );
    println!("Add '__overwrite__' to your profile mod list to include these files in future deploys.");

    Ok(())
}

/// List known tools for a game.
pub fn handle_list(game_id: &str) -> Result<()> {
    let game_plugin = modde_games::resolve_game_plugin(game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{game_id}'"))?;

    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!("could not detect install directory for {}", game_plugin.display_name())
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

/// Snapshot a directory's file listing (relative paths).
fn snapshot_dir(dir: &Path) -> Result<HashSet<String>> {
    if !dir.exists() {
        return Ok(HashSet::new());
    }

    let files = walk_files_relative(dir)
        .with_context(|| format!("failed to walk directory: {}", dir.display()))?;

    Ok(files.into_iter().map(|(rel, _)| rel).collect())
}
