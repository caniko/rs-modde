use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::{info, warn};

use modde_core::collision;
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

    let game_plugin = modde_games::resolve_game_plugin(&profile.game_id).ok_or_else(|| {
        anyhow::anyhow!(
            "unsupported game: '{}'\nSupported games: {}",
            profile.game_id,
            modde_games::SUPPORTED_GAME_IDS.join(", ")
        )
    })?;

    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install directory for {}",
            game_plugin.display_name()
        )
    })?;

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

        super::install::configure_wine_overrides(&profile.game_id, &install_dir, &staging)
            .context("Wine DLL override configuration failed")?;

        println!("Deployed Wabbajack profile: {name}");
        println!(
            "  Game: {} ({})",
            game_plugin.display_name(),
            profile.game_id
        );
        println!("  Install dir: {}", install_dir.display());
        return Ok(());
    }

    let resolved = resolver::resolve(&profile).context("failed to resolve load order")?;

    println!("Load order: {} enabled mods", resolved.order.len());

    let store = paths::store_dir();

    // Build archive-aware conflict map using the collision system.
    let classifier = modde_games::resolve_collision_classifier(&profile.game_id);

    let (conflict_map, _origins) = if let Some(ref cls) = classifier {
        collision::build_full_conflict_map(&store, &resolved.order, cls.as_ref())
            .context("failed to build conflict map")?
    } else {
        // Fallback: build a loose-files-only conflict map (no classifier available).
        let mut cm = ConflictMap::default();
        let origins = collision::OriginMap::new();
        for mod_id in &resolved.order {
            let mod_dir = store.join(mod_id.as_str());
            if !mod_dir.exists() {
                continue;
            }
            if let Ok(files) = walk_files_relative(&mod_dir) {
                for (rel_path, _) in &files {
                    cm.register(rel_path.clone(), mod_id.clone());
                }
            }
        }
        (cm, origins)
    };

    // Walk store files for the symlink farm (still needs absolute paths).
    let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
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
            if let Some(providers) = conflict_map.files.get(rel_path)
                && providers.len() > 1
            {
                conflict_count += 1;
            }
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
        let files =
            walk_files_relative(&profile.overrides).context("failed to walk override files")?;
        for (rel_path, _) in &files {
            info!(file = %rel_path, "override wins over mod file");
        }
        Some(files)
    } else {
        None
    };

    // Load hidden files for this profile
    let hidden_set: Option<HashSet<(String, String)>> = profile.id.and_then(|pid| {
        let hidden = pm.db().list_hidden_files(pid).ok()?;
        if hidden.is_empty() {
            None
        } else {
            let set: HashSet<(String, String)> =
                hidden.into_iter().map(|h| (h.mod_id, h.rel_path)).collect();
            info!(count = set.len(), "applying hidden file exclusions");
            Some(set)
        }
    });

    let farm = SymlinkFarm::build(
        name,
        &resolved,
        &mod_files,
        overrides.as_deref(),
        hidden_set.as_ref(),
    )
    .context("failed to build symlink farm")?;

    let total_files = farm.links.len();

    let farm = farm
        .materialize()
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
    super::install::configure_wine_overrides(&profile.game_id, &install_dir, &staging_dir)
        .context("Wine DLL override configuration failed")?;

    // Generate per-game tool configs and apply tool environment to launcher
    if let Ok(db) = modde_core::db::ModdeDb::open() {
        // Generate config files (MangoHud.conf, vkBasalt.conf, etc.)
        if let Err(e) = modde_games::launcher::generate_tool_configs(&profile.game_id, &db) {
            warn!(error = %e, "failed to generate tool configs");
        }

        // Apply tool env vars + wrappers to Heroic launcher config
        let launcher = modde_games::launcher::detect_launcher(&install_dir);
        if let modde_games::launcher::Launcher::Heroic {
            ref config_path,
            ref game_id,
        } = launcher
        {
            let env_vars = modde_games::launcher::collect_tool_env_vars(&profile.game_id, &db)
                .unwrap_or_default();
            let wrappers = modde_games::launcher::collect_tool_wrappers(&profile.game_id, &db)
                .unwrap_or_default();
            if let Err(e) = modde_games::launcher::apply_tool_environment_heroic(
                config_path,
                game_id,
                &env_vars,
                &wrappers,
            ) {
                warn!(error = %e, "failed to apply tool environment to Heroic");
            }
        }
    }

    println!("Deployed profile: {name}");
    println!(
        "  Game: {} ({})",
        game_plugin.display_name(),
        profile.game_id
    );
    println!("  Install dir: {}", install_dir.display());
    println!("  Mod dir: {}", game_mod_dir.display());
    println!("  Total files: {total_files}");
    println!("  Conflicts resolved: {conflict_count}");

    Ok(())
}

// Re-export deploy_mo2_to_game for use in this module's Wabbajack path.
use super::install::deploy_mo2_to_game;
