use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::{info, warn};

use modde_core::fs::walk_files_relative;
use modde_core::paths;
use modde_core::profile::{ProfileManager, ProfileSource};
use modde_core::resolver::{self, ConflictMap, ModId};
use modde_core::vfs::SymlinkFarm;

use super::load_profile_or_default;

pub async fn handle(profile_name: Option<String>, game_id: Option<String>) -> Result<()> {
    let pm = ProfileManager::open().context("failed to open profile database")?;

    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref())?;

    let name = &profile.name;
    info!(profile = %name, game = %profile.game_id, "deploying profile");

    let game_plugin = modde_games::resolve_game_plugin(&profile.game_id)
        .ok_or_else(|| anyhow::anyhow!(
            "unsupported game: '{}'\nSupported games: {}",
            profile.game_id,
            modde_games::SUPPORTED_GAME_IDS.join(", ")
        ))?;

    let install_dir = game_plugin
        .detect_install()
        .ok_or_else(|| anyhow::anyhow!(
            "could not detect install directory for {}",
            game_plugin.display_name()
        ))?;

    let game_mod_dir = game_plugin.mod_directory(&install_dir);
    info!(
        game = game_plugin.display_name(),
        install_dir = %install_dir.display(),
        mod_dir = %game_mod_dir.display(),
        "resolved game paths"
    );

    // ── Wabbajack profiles: staging is authoritative ─────────────────────────
    //
    // Wabbajack installs write mod files into the staging directory under
    // `staging/{profile_name}/mods/`. The store-based VFS build in the code
    // below only works for Nexus/Manual profiles (store keys = mod_ids).
    // For Wabbajack profiles we skip the VFS and deploy directly from staging.
    if let ProfileSource::Wabbajack { .. } = &profile.source {
        let staging = paths::staging_dir().join(&profile.name);
        info!(staging = %staging.display(), "Wabbajack profile: deploying from staging");

        deploy_mo2_to_game(&staging, &install_dir, false)
            .await
            .context("Wabbajack deploy from staging failed")?;

        game_plugin
            .post_deploy(&install_dir)
            .context("post-deploy hook failed")?;

        super::install::configure_wine_overrides(
            &profile.game_id, &install_dir, &staging,
        )
        .context("Wine DLL override configuration failed")?;

        println!("Deployed Wabbajack profile: {name}");
        println!("  Game: {} ({})", game_plugin.display_name(), profile.game_id);
        println!("  Install dir: {}", install_dir.display());
        return Ok(());
    }

    let resolved = resolver::resolve(&profile)
        .context("failed to resolve load order")?;

    println!("Load order: {} enabled mods", resolved.order.len());

    let store = paths::store_dir();
    let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
    let mut conflict_map = ConflictMap::default();
    let mut conflict_count: usize = 0;

    for mod_id in &resolved.order {
        let mod_dir_path = store.join(mod_id.as_str());

        if !mod_dir_path.exists() {
            warn!(%mod_id, "mod directory not found in store, skipping");
            continue;
        }

        let files = walk_files_relative(&mod_dir_path)
            .with_context(|| format!("failed to walk files for mod {mod_id}"))?;

        for (rel_path, _) in &files {
            if conflict_map.files.contains_key(rel_path) {
                conflict_count += 1;
                info!(file = %rel_path, mod_id = %mod_id, "file override: later mod wins");
            }
            conflict_map.register(rel_path.clone(), mod_id.clone());
        }

        mod_files.insert(mod_id.clone(), files);
    }

    let conflicts = conflict_map.conflicts();
    if !conflicts.is_empty() {
        println!("Conflicts detected ({} files):", conflicts.len());
        for (path, providers) in &conflicts {
            let provider_list: Vec<&str> = providers.iter().map(ModId::as_str).collect();
            info!(file = %path, providers = ?provider_list, "file conflict");
        }
    }

    // Collect profile-level overrides
    let overrides = if profile.overrides.exists() {
        let files = walk_files_relative(&profile.overrides)
            .context("failed to walk override files")?;
        for (rel_path, _) in &files {
            info!(file = %rel_path, "override wins over mod file");
        }
        Some(files)
    } else {
        None
    };

    let farm = SymlinkFarm::build(name, &resolved, &mod_files, overrides.as_deref())
        .context("failed to build symlink farm")?;

    let total_files = farm.links.len();

    let farm = farm.materialize()
        .await
        .context("failed to materialize symlink farm")?;

    // Delegate final deployment to the game plugin (handles game-specific deploy strategy)
    game_plugin
        .deploy(&farm.staging_dir, &game_mod_dir)
        .context("game plugin deploy failed")?;

    game_plugin
        .post_deploy(&install_dir)
        .context("post-deploy hook failed")?;

    // Configure Wine DLL overrides after every deploy, not just Wabbajack installs.
    // This ensures Nexus/Manual profiles also get WINEDLLOVERRIDES set when mods
    // deploy proxy DLLs (e.g. version.dll for CET, winmm.dll for ASI loaders).
    let staging_dir = paths::staging_dir().join(name);
    super::install::configure_wine_overrides(
        &profile.game_id, &install_dir, &staging_dir,
    )
    .context("Wine DLL override configuration failed")?;

    println!("Deployed profile: {name}");
    println!("  Game: {} ({})", game_plugin.display_name(), profile.game_id);
    println!("  Install dir: {}", install_dir.display());
    println!("  Mod dir: {}", game_mod_dir.display());
    println!("  Total files: {total_files}");
    println!("  Conflicts resolved: {conflict_count}");

    Ok(())
}

// Re-export deploy_mo2_to_game for use in this module's Wabbajack path.
use super::install::deploy_mo2_to_game;
