use anyhow::{Context, Result};
use tracing::info;

use modde_core::profile::{ActivateResult, Profile, ProfileManager, ProfileSource};
use modde_core::save::SaveFingerprint;

use crate::ProfileAction;
use super::resolve_save_dir;

/// Compute a save fingerprint for a profile by classifying its mods via the game plugin.
fn compute_fingerprint(pm: &ProfileManager, name: &str, game_id: &str) -> Option<SaveFingerprint> {
    let profile = pm.load(name, Some(game_id)).ok()?;
    let game_plugin = modde_games::resolve_game_plugin(game_id)?;
    let staging_dir = ProfileManager::staging_dir(&profile.name);

    Some(SaveFingerprint::compute(&profile.mods, |mod_id| {
        let mod_path = staging_dir.join(mod_id);
        game_plugin.classify_mod(&mod_path).affects_saves()
    }))
}

pub fn handle(action: ProfileAction) -> Result<()> {
    let pm = ProfileManager::open().context("failed to open profile database")?;

    match action {
        ProfileAction::List { game } => {
            let profiles = match game {
                Some(ref g) => pm.list_for_game(g)?,
                None => pm.list()?,
            };
            if profiles.is_empty() {
                println!("No profiles found.");
            } else {
                for p in profiles {
                    println!("  {} (game: {}, {} mods, source: {})", p.name, p.game_id, p.mod_count, p.source_type);
                }
            }
        }
        ProfileAction::Switch { name, game } => {
            let save_dir = resolve_save_dir(&game);
            let fp = compute_fingerprint(&pm, &name, &game);
            match pm.activate_with_fingerprint(&name, &game, save_dir.as_deref(), fp.as_ref())? {
                ActivateResult::Activated => {
                    info!(profile = %name, "switched to profile");
                    if save_dir.is_some() {
                        println!("Switched to profile: {name} (saves swapped)");
                    } else {
                        println!("Switched to profile: {name} (no save directory detected)");
                    }
                }
                ActivateResult::AdoptionRequired { save_count } => {
                    println!(
                        "Found {save_count} existing save(s) in the game directory.\n\
                         Run `modde save adopt --game {game} --profile {name}` to adopt them first,\n\
                         or use a different profile."
                    );
                }
            }
        }
        ProfileAction::Create { name, game } => {
            let profile = Profile {
                id: None,
                name: name.clone(),
                game_id: modde_core::GameId::from(game.clone()),
                source: ProfileSource::Manual,
                mods: vec![],
                overrides: ProfileManager::default_overrides(&name),
                load_order_rules: smallvec::SmallVec::new(),
            };
            pm.create(&profile)?;
            println!("Created profile: {name} (game: {game})");
        }
        ProfileAction::Delete { name, game } => {
            pm.delete(&name, game.as_deref())?;
            println!("Deleted profile: {name}");
        }
        ProfileAction::Try { name, game } => {
            let save_dir = resolve_save_dir(&game);
            let fp = compute_fingerprint(&pm, &name, &game);
            pm.try_profile_with_fingerprint(&name, &game, save_dir.as_deref(), fp.as_ref())?;
            let depth = pm.active(&game)?
                .map(|a| a.experiment_depth)
                .unwrap_or(0);
            println!("Experimenting with profile: {name} (stack depth: {depth})");
            println!("Use `modde profile rollback --game {game}` to undo, or `modde profile commit --game {game}` to accept.");
        }
        ProfileAction::Rollback { game } => {
            let save_dir = resolve_save_dir(&game);

            // Compute fingerprint for the current (about-to-be-rolled-back) profile
            let fp = pm.active(&game)?
                .and_then(|info| {
                    let game_plugin = modde_games::resolve_game_plugin(&game)?;
                    let staging_dir = ProfileManager::staging_dir(&info.profile.name);
                    Some(SaveFingerprint::compute(&info.profile.mods, |mod_id| {
                        let mod_path = staging_dir.join(mod_id);
                        game_plugin.classify_mod(&mod_path).affects_saves()
                    }))
                });

            let restored = pm.rollback_with_fingerprint(&game, save_dir.as_deref(), fp.as_ref())?;
            println!("Rolled back to profile: {restored}");
        }
        ProfileAction::Commit { game } => {
            pm.commit(&game)?;
            println!("Experiment accepted. Rollback stack cleared for game: {game}");
        }
        ProfileAction::Active { game } => {
            match pm.active(&game)? {
                Some(info) => {
                    println!("Active profile: {} (game: {})", info.profile.name, info.profile.game_id);
                    println!("  Mods: {}", info.profile.mods.len());
                    if info.experiment_depth > 0 {
                        println!("  Experiment depth: {} (use `rollback` to undo or `commit` to accept)", info.experiment_depth);
                    }

                    // Show fingerprint info
                    if let Some(fp) = compute_fingerprint(&pm, &info.profile.name, info.profile.game_id.as_str()) {
                        if !fp.is_empty() {
                            println!("  Save fingerprint: {} ({} save-breaking mod(s))", fp.short_hash(), fp.mod_ids.len());
                        } else {
                            println!("  Save fingerprint: none (no save-breaking mods)");
                        }
                    }
                }
                None => {
                    println!("No active profile for game: {game}");
                }
            }
        }
        ProfileAction::Fork { source, name, game } => {
            let id = pm.fork(&source, &name, &game)?;
            println!("Forked profile '{source}' -> '{name}' (id: {id}, mods + saves cloned)");
        }
    }

    Ok(())
}
