use anyhow::{Context, Result};
use modde_core::diagnostics::{DiagContext, Severity};
use modde_core::paths;
use modde_core::profile::ProfileManager;
use modde_core::resolver::ConflictMap;

pub fn handle(game_id: &str, profile_name: Option<String>) -> Result<()> {
    let pm = ProfileManager::open().context("failed to open profile database")?;

    // Load profile (use active or specified)
    let profile = match profile_name {
        Some(name) => pm.load(&name, Some(game_id))?,
        None => {
            let (_, name) = pm
                .db()
                .get_active_profile(game_id)?
                .ok_or_else(|| anyhow::anyhow!("no active profile for game '{game_id}'"))?;
            pm.load(&name, Some(game_id))?
        }
    };

    // Build conflict map (simplified for now)
    let conflict_map = ConflictMap::default();

    // Build context
    let store = paths::store_dir();
    let staging = paths::staging_dir().join(&profile.name);
    let ctx = DiagContext {
        game_id,
        profile: &profile,
        conflict_map: &conflict_map,
        collision_report: None,
        store_dir: &store,
        staging_dir: &staging,
    };

    // Get appropriate engine for the game
    let engine = match game_id {
        "skyrim-se" | "skyrim-ae" | "fallout4" | "fallout76" => {
            modde_games::bethesda::diagnostics::bethesda_diagnostics()
        }
        _ => modde_core::diagnostics::DiagnosticEngine::new(),
    };

    let diagnostics = engine.run_all(&ctx);

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
