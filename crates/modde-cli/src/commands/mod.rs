pub mod backup;
pub mod collisions;
pub mod deploy;
pub mod detect;
pub mod diagnostics;
pub mod export;
pub mod fomod;
pub mod import;
pub mod install;
pub mod instance;
pub mod loot;
pub mod nexus;
pub mod nxm;
pub mod play;
pub mod profile;
pub mod rollback;
pub mod save;
pub mod scan;
pub mod stock;
pub mod tool;
pub mod uninstall;
pub mod update;
pub mod verify;

use std::path::PathBuf;

use anyhow::Result;

use modde_core::PluginEntry;
use modde_core::profile::{Profile, ProfileManager};
use modde_core::save::SaveFingerprint;

/// Resolve the game's save directory via the `GamePlugin` trait.
///
/// Shared across profile and save commands to avoid duplication.
pub fn resolve_save_dir(game_id: &str) -> Option<PathBuf> {
    modde_games::resolve_game_plugin(game_id).and_then(modde_games::GamePlugin::save_directory)
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
    resolve_save_dir(game_id).ok_or_else(|| {
        anyhow::anyhow!(
            "save directory not found for game '{game_id}'. \
             The game may not be installed, or save tracking is not supported for this title."
        )
    })
}

/// Compute a save fingerprint for a profile by classifying its mods via the game plugin.
pub fn compute_fingerprint(
    pm: &ProfileManager,
    name: &str,
    game_id: &str,
) -> Option<SaveFingerprint> {
    let profile = pm.load(name, Some(game_id)).ok()?;
    let game_plugin = modde_games::resolve_game_plugin(game_id)?;
    let staging_dir = ProfileManager::staging_dir(&profile.name);

    Some(SaveFingerprint::compute(&profile.mods, |mod_id| {
        let mod_path = staging_dir.join(mod_id);
        game_plugin.classify_mod(&mod_path).affects_saves()
    }))
}

/// Load a profile by name (optional) and game (optional), falling back to
/// the first available profile when no name is given.
pub fn load_profile_or_default(
    pm: &ProfileManager,
    name: Option<&str>,
    game_id: Option<&str>,
) -> Result<Profile> {
    if let Some(name) = name { Ok(pm.load(name, game_id)?) } else {
        let profiles = pm.list()?;
        let first = profiles.first().ok_or_else(|| {
            anyhow::anyhow!(
                "no profiles found. Create one with: modde profile create <name> --game <id>"
            )
        })?;
        Ok(pm.load(&first.name, Some(&first.game_id))?)
    }
}

/// Load the real plugin order for a profile, preferring the DB and falling back
/// to the game's native `plugins.txt` when the DB has not been populated yet.
pub fn load_plugin_order(pm: &ProfileManager, profile: &Profile) -> Result<Vec<PluginEntry>> {
    let mut plugins = profile
        .id
        .map(|profile_id| pm.db().get_plugin_order(profile_id))
        .transpose()?
        .unwrap_or_default();

    if plugins.is_empty() {
        plugins =
            modde_games::read_native_plugin_order(profile.game_id.as_str()).unwrap_or_default();
        if !plugins.is_empty()
            && let Some(profile_id) = profile.id {
                pm.db().set_plugin_order(profile_id, &plugins)?;
            }
    }

    Ok(plugins)
}

/// Persist plugin order to both the DB and the game's native `plugins.txt`
/// when the current game supports a writable plugin list.
pub fn persist_plugin_order(
    pm: &ProfileManager,
    profile: &Profile,
    plugins: &[PluginEntry],
) -> Result<()> {
    if let Some(profile_id) = profile.id {
        pm.db().set_plugin_order(profile_id, plugins)?;
    }

    if modde_games::resolve_game_plugin(profile.game_id.as_str())
        .is_some_and(modde_games::GamePlugin::has_plugin_system)
    {
        modde_games::write_native_plugin_order(profile.game_id.as_str(), plugins)?;
    }

    Ok(())
}
