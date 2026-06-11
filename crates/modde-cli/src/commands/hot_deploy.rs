use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use modde_core::fs::walk_files_relative;
use modde_core::hot_deploy::{ensure_cosmetic_mod, ensure_cosmetic_patch, plan_patch};
use modde_core::profile::{Profile, ProfileManager, ProfileSource};
use modde_core::resolver::{self, ModId};
use modde_core::vfs::{Built, SymlinkFarm};

use super::load_profile_or_default;

pub async fn handle(
    profile_name: Option<String>,
    game_id: Option<String>,
    mod_id: String,
    enable: bool,
    disable: bool,
    dry_run: bool,
    force: bool,
) -> Result<()> {
    if enable == disable {
        bail!("choose exactly one of --enable or --disable");
    }
    let target_enabled = enable;

    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;

    if matches!(&profile.source, ProfileSource::Wabbajack { .. }) {
        bail!(
            "hot-deploy is not supported for Wabbajack profiles; run `modde deploy --profile {} --game {}`",
            profile.name,
            profile.game_id
        );
    }

    let plugin = modde_games::resolve_game_plugin(profile.game_id.as_str()).ok_or_else(|| {
        anyhow::anyhow!(
            "unsupported game: '{}'\nSupported games: {}",
            profile.game_id,
            modde_games::supported_game_ids().join(", ")
        )
    })?;
    let capability = plugin.hot_deploy_capability();
    if !capability.is_supported() {
        bail!(
            "hot-deploy is not supported for {} ({})",
            plugin.display_name(),
            profile.game_id
        );
    }

    let classifier = modde_games::resolve_collision_classifier(profile.game_id.as_str())
        .ok_or_else(|| anyhow::anyhow!("no collision classifier for game '{}'", profile.game_id))?;

    let Some(target_index) = profile.mods.iter().position(|entry| entry.mod_id == mod_id) else {
        bail!(
            "hot-deploy refused: mod '{}' is not part of profile '{}'",
            mod_id,
            profile.name
        );
    };

    let store = modde_core::paths::store_dir();
    let mod_dir = store.join(&mod_id);
    let inspected_files = ensure_cosmetic_mod(&mod_dir, classifier.as_ref())?;

    let mut next_profile = profile.clone();
    next_profile.mods[target_index].enabled = target_enabled;

    let before_farm = build_farm(&pm, &profile, &store).await?;
    let after_farm = build_farm(&pm, &next_profile, &store).await?;
    let patch = plan_patch(&before_farm.links, &after_farm.links);
    ensure_cosmetic_patch(&patch, classifier.as_ref())?;

    println!(
        "Hot-deploy plan: {} {} in profile '{}' for {}",
        if target_enabled { "enable" } else { "disable" },
        mod_id,
        profile.name,
        plugin.display_name()
    );
    println!("  Inspected files: {inspected_files}");
    println!("  Added paths: {}", patch.added_count());
    println!("  Removed paths: {}", patch.removed_count());
    println!("  Changed paths: {}", patch.changed_count());

    if patch.is_empty() {
        println!("  No live VFS changes required.");
        if !dry_run && profile.mods[target_index].enabled != target_enabled {
            pm.create_or_update(&next_profile)
                .await
                .context("failed to persist profile state")?;
            println!("  Profile state updated.");
        }
        return Ok(());
    }

    if dry_run {
        println!("  Dry run: no files or profile state changed.");
        return Ok(());
    }

    if !before_farm.staging_dir.is_dir() {
        bail!(
            "hot-deploy refused: profile staging directory is missing: {}\nRun `modde deploy --profile {} --game {}` first.",
            before_farm.staging_dir.display(),
            profile.name,
            profile.game_id
        );
    }

    let install_dir = plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install directory for {}",
            plugin.display_name()
        )
    })?;
    if !force && let Some(process) = running_game_process(profile.game_id.as_str()) {
        bail!(
            "hot-deploy refused: {} appears to be running as process '{}'. Close the game or re-run with --force.",
            plugin.display_name(),
            process
        );
    }

    if let Err(err) = plugin.apply_hot_deploy_patch(&patch, &before_farm.staging_dir, &install_dir)
    {
        eprintln!(
            "hot-deploy failed; recover with: modde deploy --profile {} --game {}",
            profile.name, profile.game_id
        );
        return Err(err).context("game plugin hot-deploy patch failed");
    }

    pm.create_or_update(&next_profile)
        .await
        .context("failed to persist profile state")?;

    println!("  Live VFS patched.");
    println!("  Profile state updated.");

    Ok(())
}

fn running_game_process(game_id: &str) -> Option<String> {
    let executable_names = hot_deploy_process_names(game_id)?;
    let entries = std::fs::read_dir("/proc").ok()?;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let pid = file_name.to_string_lossy();
        if !pid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let comm = std::fs::read_to_string(entry.path().join("comm"))
            .ok()
            .map(|value| value.trim().to_string());
        let exe = std::fs::read_link(entry.path().join("exe"))
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().to_string())
            });
        for candidate in [comm.as_deref(), exe.as_deref()].into_iter().flatten() {
            if executable_names
                .iter()
                .any(|name| candidate.eq_ignore_ascii_case(name))
            {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

fn hot_deploy_process_names(game_id: &str) -> Option<&'static [&'static str]> {
    match game_id {
        "cyberpunk2077" => Some(&["Cyberpunk2077.exe", "Cyberpunk2077"]),
        "baldurs-gate3" => Some(&["bg3.exe", "bg3_dx11.exe", "bg3", "bg3_dx11"]),
        _ => None,
    }
}

async fn build_farm(
    pm: &ProfileManager,
    profile: &Profile,
    store: &Path,
) -> Result<SymlinkFarm<Built>> {
    let resolved = resolver::resolve(profile).context("failed to resolve load order")?;
    let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();

    for mod_id in &resolved.order {
        let mod_dir = store.join(mod_id.as_str());
        if !mod_dir.exists() {
            continue;
        }
        let files = walk_files_relative(&mod_dir)
            .with_context(|| format!("failed to walk files for mod {mod_id}"))?;
        mod_files.insert(mod_id.clone(), files);
    }

    let overrides = if profile.overrides.exists() {
        Some(walk_files_relative(&profile.overrides).context("failed to walk override files")?)
    } else {
        None
    };

    let hidden_set: Option<HashSet<(String, String)>> = match profile.id {
        Some(pid) => match pm.db().list_hidden_files(pid).await.ok() {
            Some(hidden) if !hidden.is_empty() => Some(
                hidden
                    .into_iter()
                    .map(|hidden| (hidden.mod_id, hidden.rel_path))
                    .collect(),
            ),
            _ => None,
        },
        None => None,
    };

    SymlinkFarm::build(
        &profile.name,
        &resolved,
        &mod_files,
        overrides.as_deref(),
        hidden_set.as_ref(),
    )
    .context("failed to build symlink farm")
}
