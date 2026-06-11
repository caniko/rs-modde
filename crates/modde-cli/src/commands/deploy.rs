use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::{info, warn};

use modde_core::collision;
use modde_core::fs::{symlink_async, walk_files_relative};
use modde_core::installer::InstallMethod;
use modde_core::paths;
use modde_core::profile::{ProfileManager, ProfileSource};
use modde_core::resolver::{self, ConflictMap, ModId};
use modde_core::vfs::SymlinkFarm;

use super::load_profile_or_default;

pub async fn handle(profile_name: Option<String>, game_id: Option<String>) -> Result<()> {
    handle_inner(profile_name, game_id, true).await
}

pub(crate) async fn handle_inner(
    profile_name: Option<String>,
    game_id: Option<String>,
    run_patchers: bool,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;

    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;

    let name = &profile.name;
    info!(profile = %name, game = %profile.game_id, "deploying profile");

    let game_plugin =
        modde_games::resolve_game_plugin(profile.game_id.as_str()).ok_or_else(|| {
            anyhow::anyhow!(
                "unsupported game: '{}'\nSupported games: {}",
                profile.game_id,
                modde_games::supported_game_ids().join(", ")
            )
        })?;

    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install directory for {}",
            game_plugin.display_name()
        )
    })?;

    let game_mod_dir = game_plugin
        .mod_root(&install_dir)
        .context("failed to resolve game mod root")?;
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

        super::install::configure_wine_overrides(profile.game_id.as_str(), &install_dir, &staging)
            .await
            .context("Wine DLL override configuration failed")?;

        let patcher_count = if run_patchers {
            super::patcher::run_enabled_for_deploy(&pm, &profile, game_plugin, &install_dir)
                .await
                .context("patcher pipeline failed")?
        } else {
            0
        };

        println!("Deployed Wabbajack profile: {name}");
        println!(
            "  Game: {} ({})",
            game_plugin.display_name(),
            profile.game_id
        );
        println!("  Install dir: {}", install_dir.display());
        if patcher_count > 0 {
            println!("  Patcher stages: {patcher_count}");
        }
        return Ok(());
    }

    let resolved = resolver::resolve(&profile).context("failed to resolve load order")?;

    println!("Load order: {} enabled mods", resolved.order.len());

    let store = paths::store_dir();

    // Build archive-aware conflict map using the collision system.
    let classifier = modde_games::resolve_collision_classifier(profile.game_id.as_str());

    let conflict_map = if let Some(ref cls) = classifier {
        collision::build_full_conflict_map(&store, &resolved.order, cls.as_ref())
            .context("failed to build conflict map")?
            .conflict_map
    } else {
        // Fallback: build a loose-files-only conflict map (no classifier available).
        let mut cm = ConflictMap::default();
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
        cm
    };

    // Walk store files for the symlink farm (still needs absolute paths).
    let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
    let mut conflict_count: usize = 0;
    let mut missing_mods = Vec::new();
    for mod_id in &resolved.order {
        let mod_dir_path = store.join(mod_id.as_str());
        if !mod_dir_path.exists() {
            missing_mods.push(mod_id.clone());
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
    if let Some(summary) = collision::summarize_missing_store_dirs(&missing_mods) {
        warn!(
            store = %store.display(),
            missing_mod_count = summary.missing_mod_count,
            missing_mod_sample = ?summary.missing_mod_sample,
            omitted_missing_mod_count = summary.omitted_missing_mod_count,
            "mod directories not found in store, skipping"
        );
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
    let hidden_set: Option<HashSet<(String, String)>> = match profile.id {
        Some(pid) => match pm.db().list_hidden_files(pid).await.ok() {
            Some(hidden) if !hidden.is_empty() => {
                let set: HashSet<(String, String)> =
                    hidden.into_iter().map(|h| (h.mod_id, h.rel_path)).collect();
                info!(count = set.len(), "applying hidden file exclusions");
                Some(set)
            }
            _ => None,
        },
        None => None,
    };

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
        .deploy_to_install(&farm.staging_dir, &install_dir)
        .context("game plugin deploy failed")?;

    game_plugin
        .post_deploy(&install_dir)
        .context("post-deploy hook failed")?;

    // Configure Wine DLL overrides after every deploy, not just Wabbajack installs.
    // This ensures Nexus/Manual profiles also get WINEDLLOVERRIDES set when mods
    // deploy proxy DLLs (e.g. version.dll for CET, winmm.dll for ASI loaders).
    let staging_dir = paths::staging_dir().join(name);
    super::install::configure_wine_overrides(profile.game_id.as_str(), &install_dir, &staging_dir)
        .await
        .context("Wine DLL override configuration failed")?;

    // Generate per-game tool configs and apply tool environment to launcher
    if let Ok(db) = modde_core::db::ModdeDb::open().await {
        // Generate config files (MangoHud.conf, vkBasalt.conf, etc.)
        if let Err(e) = modde_games::launcher::generate_tool_configs(&profile.game_id, &db).await {
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
                .await
                .unwrap_or_default();
            let wrappers = modde_games::launcher::collect_tool_wrappers(&profile.game_id, &db)
                .await
                .unwrap_or_default();
            match modde_games::launcher::apply_tool_environment_heroic(
                config_path,
                game_id,
                &env_vars,
                &wrappers,
            ) {
                Ok(report) => super::install::print_tool_environment_report(&report),
                Err(e) => {
                    warn!(error = %e, "failed to apply tool environment to Heroic");
                }
            }
        }
    }

    let alt_routed = deploy_alt_target_mods(&profile, game_plugin, &install_dir, &store).await?;
    let patcher_count = if run_patchers {
        super::patcher::run_enabled_for_deploy(&pm, &profile, game_plugin, &install_dir)
            .await
            .context("patcher pipeline failed")?
    } else {
        0
    };

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
    if alt_routed > 0 {
        println!("  Alt-target files: {alt_routed}");
    }
    if patcher_count > 0 {
        println!("  Patcher stages: {patcher_count}");
    }

    Ok(())
}

/// Deploy mods whose [`InstallMethod`] routes them to a plugin-supplied
/// alternate root (e.g. UE4's per-user `Saved/Config/Windows`).
///
/// Runs *after* the main symlink farm so an alt-target mod can never
/// shadow normal game-root mods. Each alt-target mod is symlinked
/// directly from its store dir into the resolved root: load-order
/// conflict resolution is intentionally bypassed for these mods —
/// their files are usually whole-file replacements (e.g. `Engine.ini`)
/// where last-mod-wins is the right semantics anyway, and trying to
/// run them through the farm would require teaching the farm about
/// multiple deploy roots.
async fn deploy_alt_target_mods(
    profile: &modde_core::profile::Profile,
    plugin: &dyn modde_games::GamePlugin,
    install_dir: &std::path::Path,
    store: &std::path::Path,
) -> Result<usize> {
    let mut linked = 0usize;

    for em in &profile.mods {
        if !em.enabled {
            continue;
        }
        let Some(method) = em.install_method.as_ref() else {
            continue;
        };
        let target_id = match method {
            InstallMethod::UserConfigOverlay { target_id } => target_id.as_str(),
            _ => continue,
        };

        let Some(target_root) = plugin.resolve_deploy_target(target_id, install_dir) else {
            warn!(
                mod_id = %em.mod_id,
                %target_id,
                "alt deploy target unresolved (Wine prefix may not exist yet); \
                 launch the game once via Steam/Proton, then re-deploy."
            );
            continue;
        };

        let mod_dir = store.join(&em.mod_id);
        if !mod_dir.exists() {
            warn!(mod_id = %em.mod_id, "store dir missing, skipping alt-target deploy");
            continue;
        }

        tokio::fs::create_dir_all(&target_root).await?;

        let files = walk_files_relative(&mod_dir)
            .with_context(|| format!("failed to walk store dir for mod {}", em.mod_id))?;
        for (rel_path, abs_src) in &files {
            let dst = target_root.join(rel_path);
            if let Some(parent) = dst.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            if dst.symlink_metadata().is_ok() {
                tokio::fs::remove_file(&dst).await?;
            }
            symlink_async(abs_src, &dst).await?;
            linked += 1;
        }
        info!(
            mod_id = %em.mod_id,
            target = %target_root.display(),
            files = files.len(),
            "deployed mod to alt target"
        );
    }

    Ok(linked)
}

// Re-export deploy_mo2_to_game for use in this module's Wabbajack path.
use super::install::deploy_mo2_to_game;
