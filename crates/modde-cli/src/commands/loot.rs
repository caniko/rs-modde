use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::info;

use modde_games::bethesda::loot::{self, LootMasterlist};
use modde_games::bethesda::plugin_header::{self, PluginWarning};

/// Sort plugins using LOOT masterlist rules and validate plugin headers.
pub fn handle_sort(game_id: &str, _data_dir: Option<PathBuf>) -> Result<()> {
    let cache_path = loot::masterlist_cache_path(game_id);

    if !cache_path.exists() {
        let url = loot::masterlist_url(game_id).ok_or_else(|| {
            anyhow::anyhow!(
                "no LOOT masterlist available for game '{game_id}'. \
                 LOOT sorting is only supported for Bethesda games."
            )
        })?;
        println!("Masterlist not cached. Download it with:");
        println!("  curl -L -o '{}' '{url}'", cache_path.display());
        println!("\nOr place a masterlist.yaml at: {}", cache_path.display());
        return Ok(());
    }

    let masterlist =
        LootMasterlist::from_file(&cache_path).context("failed to parse LOOT masterlist")?;

    println!(
        "Loaded LOOT masterlist: {} plugin entries",
        masterlist.plugins.len()
    );

    // Try to read the current plugins.txt
    let plugins = read_active_plugins(game_id)?;
    if plugins.is_empty() {
        println!("No active plugins found for game '{game_id}'.");
        return Ok(());
    }

    let plugin_names: Vec<&str> = plugins.iter().map(|s| s.as_str()).collect();
    let rules = masterlist.rules_for_plugins(&plugin_names);

    println!("Generated {} load order rules from masterlist", rules.len());

    for rule in &rules {
        match rule {
            modde_core::resolver::LoadOrderRule::LoadAfter { mod_id, after } => {
                println!("  {mod_id} loads after {after}");
            }
            modde_core::resolver::LoadOrderRule::LoadBefore { mod_id, before } => {
                println!("  {mod_id} loads before {before}");
            }
            modde_core::resolver::LoadOrderRule::Incompatible { mod_a, mod_b } => {
                println!("  INCOMPATIBLE: {mod_a} <-> {mod_b}");
            }
        }
    }

    Ok(())
}

/// Validate plugins in the game's Data directory.
pub fn handle_validate(game_id: &str) -> Result<()> {
    let game_plugin = modde_games::resolve_game_plugin(game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{game_id}'"))?;

    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install directory for {}",
            game_plugin.display_name()
        )
    })?;

    let data_dir = game_plugin.mod_directory(&install_dir);
    info!(data_dir = %data_dir.display(), "validating plugins");

    let plugins = read_active_plugins(game_id)?;
    if plugins.is_empty() {
        println!("No active plugins found for game '{game_id}'.");
        return Ok(());
    }

    let plugin_names: Vec<&str> = plugins.iter().map(|s| s.as_str()).collect();
    let check_form_43 = matches!(game_id, "skyrim-se" | "skyrim-ae");

    let warnings = plugin_header::validate_plugins(&data_dir, &plugin_names, check_form_43);

    if warnings.is_empty() {
        println!("All {} plugins are valid.", plugins.len());
    } else {
        println!("{} warning(s) found:\n", warnings.len());
        for w in &warnings {
            match w {
                PluginWarning::Form43 { plugin, version } => {
                    println!(
                        "  [FORM43] {plugin} (v{version:.2}) — Oldrim format, may cause CTDs in SSE"
                    );
                }
                PluginWarning::MissingMaster { plugin, master } => {
                    println!("  [MISSING] {plugin} requires '{master}' which is not loaded");
                }
            }
        }
    }

    Ok(())
}

/// Read active plugins from plugins.txt for a given game.
fn read_active_plugins(game_id: &str) -> Result<Vec<String>> {
    use modde_games::bethesda::plugins_txt;

    let (app_id, game_name) = match game_id {
        "skyrim-se" | "skyrim-ae" => (plugins_txt::SKYRIM_SE_APP_ID, "Skyrim Special Edition"),
        "fallout4" => (plugins_txt::FALLOUT4_APP_ID, "Fallout4"),
        "fallout76" => (plugins_txt::FALLOUT76_APP_ID, "Fallout76"),
        "starfield" => (plugins_txt::STARFIELD_APP_ID, "Starfield"),
        _ => {
            return Ok(Vec::new());
        }
    };

    match plugins_txt::read_plugins_txt(app_id, game_name) {
        Ok(entries) => Ok(entries
            .into_iter()
            .filter(|e| e.enabled)
            .map(|e| e.name)
            .collect()),
        Err(e) => {
            info!(error = %e, "could not read plugins.txt, may not exist yet");
            Ok(Vec::new())
        }
    }
}
