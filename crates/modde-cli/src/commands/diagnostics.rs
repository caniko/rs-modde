use anyhow::{Context, Result};
use std::collections::HashSet;

use modde_core::diagnostics::Severity;
use modde_core::paths;
use modde_core::profile::ProfileManager;
use modde_core::resolver::GameId;

pub fn handle(game_id: &str, profile_name: Option<String>) -> Result<()> {
    let pm = ProfileManager::open().context("failed to open profile database")?;

    let typed_game_id = GameId::from(game_id);

    // Load profile (use active or specified)
    let profile = if let Some(name) = profile_name {
        pm.load(&name, Some(&typed_game_id))?
    } else {
        let (_, name) = pm
            .db()
            .get_active_profile(&typed_game_id)?
            .ok_or_else(|| anyhow::anyhow!("no active profile for game '{game_id}'"))?;
        pm.load(&name, Some(&typed_game_id))?
    };

    let store = paths::store_dir();
    let staging = modde_core::profile::ProfileManager::staging_dir(&profile.name);
    let hidden: HashSet<(String, String)> = profile
        .id
        .map(|profile_id| {
            pm.db().list_hidden_files(profile_id).map(|rows| {
                rows.into_iter()
                    .map(|row| (row.mod_id, row.rel_path))
                    .collect()
            })
        })
        .transpose()?
        .unwrap_or_default();
    let active_plugins = super::load_plugin_order(&pm, &profile)?
        .into_iter()
        .filter(|plugin| plugin.enabled)
        .map(|plugin| plugin.plugin_name)
        .collect::<Vec<_>>();

    // Get appropriate engine for the game
    let engine = match game_id {
        "skyrim-se" | "skyrim-ae" | "fallout4" | "fallout76" => {
            modde_games::bethesda::diagnostics::bethesda_diagnostics()
        }
        _ => modde_core::diagnostics::base_diagnostics(),
    };

    let classifier = modde_games::resolve_collision_classifier(game_id);
    let (diagnostics, _) = modde_core::diagnostics::run_profile_diagnostics(
        game_id,
        &profile,
        &active_plugins,
        &store,
        &staging,
        &hidden,
        classifier.as_deref(),
        &engine,
    )?;

    if diagnostics.is_empty() {
        println!(
            "No issues found for profile '{}' ({game_id}).",
            profile.name
        );
        return Ok(());
    }

    println!("{} issue(s) found:\n", diagnostics.len());
    for d in &diagnostics {
        let icon = match d.severity {
            Severity::Error => "[ERROR]",
            Severity::Warning => "[WARN] ",
            Severity::Info => "[INFO] ",
        };
        println!("  {icon} {}", d.title);
        if !d.detail.is_empty() {
            println!("         {}", d.detail);
        }
        if let Some(mod_id) = &d.affected_mod {
            println!("         Mod: {mod_id}");
        }
        if let Some(fix) = &d.fix {
            println!("         Fix: {}", fix.description);
        }
    }

    Ok(())
}
