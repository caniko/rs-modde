pub mod deploy;
pub mod detect;
pub mod fomod;
pub mod import;
pub mod install;
pub mod nexus;
pub mod profile;
pub mod rollback;
pub mod save;
pub mod stock;
pub mod verify;

use std::path::PathBuf;

use anyhow::Result;

use modde_core::profile::{Profile, ProfileManager};

/// Resolve the game's save directory via the GamePlugin trait.
///
/// Shared across profile and save commands to avoid duplication.
pub fn resolve_save_dir(game_id: &str) -> Option<PathBuf> {
    modde_games::resolve_game_plugin(game_id).and_then(|p| p.save_directory())
}

/// Resolve the game's save directory, returning an error if not found.
pub fn require_save_dir(game_id: &str) -> Result<PathBuf> {
    // First check the game ID is valid at all
    if modde_games::resolve_game_plugin(game_id).is_none() {
        anyhow::bail!(
            "unknown game '{}'. Supported games: {}",
            game_id,
            modde_games::SUPPORTED_GAME_IDS.join(", ")
        );
    }
    resolve_save_dir(game_id)
        .ok_or_else(|| anyhow::anyhow!(
            "save directory not found for game '{game_id}'. \
             The game may not be installed, or save tracking is not supported for this title."
        ))
}

/// Load a profile by name (optional) and game (optional), falling back to
/// the first available profile when no name is given.
pub fn load_profile_or_default(
    pm: &ProfileManager,
    name: Option<&str>,
    game_id: Option<&str>,
) -> Result<Profile> {
    match name {
        Some(name) => Ok(pm.load(name, game_id)?),
        None => {
            let profiles = pm.list()?;
            let first = profiles
                .first()
                .ok_or_else(|| anyhow::anyhow!(
                    "no profiles found. Create one with: modde profile create <name> --game <id>"
                ))?;
            Ok(pm.load(&first.name, Some(&first.game_id))?)
        }
    }
}
