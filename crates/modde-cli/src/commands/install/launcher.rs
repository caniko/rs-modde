//! Launcher integration and MO2 staging deployment helpers.

use super::*;

pub fn print_launcher_configuration_report(
    report: &modde_games::launcher::LauncherConfigurationReport,
) {
    if let Some(wine_overrides) = &report.wine_overrides {
        print_wine_override_report(wine_overrides);
    }
    if let Some(wrapper) = &report.launch_wrapper {
        print_launch_wrapper_report(wrapper);
    }
    if let Some(registration) = &report.wrapper_registration {
        print_wrapper_registration_report(registration);
    }
}

fn print_wine_override_report(report: &modde_games::launcher::WineOverrideReport) {
    match report {
        modde_games::launcher::WineOverrideReport::HeroicUpdated { value } => {
            println!("  Updated Heroic config with WINEDLLOVERRIDES: {value}");
        }
        modde_games::launcher::WineOverrideReport::SteamInstruction { override_value } => {
            println!(
                "\nSteam: Add this to your launch options:\n  \
                 WINEDLLOVERRIDES=\"{override_value}\" %command%"
            );
        }
        modde_games::launcher::WineOverrideReport::UnknownInstruction { override_value } => {
            println!(
                "\nSet this environment variable before launching:\n  \
                 WINEDLLOVERRIDES=\"{override_value}\""
            );
        }
    }
}

fn print_launch_wrapper_report(report: &modde_games::launcher::LaunchWrapperReport) {
    println!("  Launch wrapper: {}", report.path.display());
    if report.restore_count > 0 {
        println!("  Launch wrapper: restores {} DLL(s)", report.restore_count);
    }
    if report.tool_env_var_count > 0 {
        println!(
            "  Launch wrapper: exports {} tool env var(s)",
            report.tool_env_var_count
        );
    }
}

fn print_wrapper_registration_report(report: &modde_games::launcher::WrapperRegistrationReport) {
    match report {
        modde_games::launcher::WrapperRegistrationReport::HeroicRegistered => {
            println!("  Registered modde launch wrapper in Heroic (position: after fgmod)");
        }
        modde_games::launcher::WrapperRegistrationReport::ManualInstruction { wrapper_path } => {
            let wrapper_str = wrapper_path.display();
            println!("\nAdd this wrapper before your game launcher:\n  {wrapper_str} --");
        }
    }
}

pub fn print_tool_environment_report(report: &modde_games::launcher::ToolEnvironmentReport) {
    if report.env_var_count > 0 {
        println!(
            "  Applied {} tool env var(s) to Heroic config",
            report.env_var_count
        );
    }
    if report.wrapper_count > 0 {
        println!(
            "  Registered {} tool wrapper(s) in Heroic config",
            report.wrapper_count
        );
    }
}

/// Fetch a Nexus Collection manifest via the two-step API.
///
/// Step 1: `GET /v1/collections/{slug}.json` → discover `game_domain` + latest revision.
/// Step 2: `GET /v1/collections/{slug}/revisions/{rev}.json?game_domain_name={game_domain}`.
///
/// If `version` is already known (e.g. from CLI arg), the revision number is
pub async fn configure_wine_overrides(
    game_id: &str,
    game_dir: &Path,
    staging: &Path,
) -> Result<modde_games::launcher::LauncherConfigurationReport> {
    let mut report = modde_games::launcher::LauncherConfigurationReport::default();
    let Some(plugin) = modde_games::resolve_game_plugin(game_id) else {
        info!(%game_id, "no game plugin found, skipping Wine DLL override detection");
        return Ok(report);
    };

    // Merge overrides from deployed game dir and staging (staging catches DLLs
    // that fgmod may have deleted from the game dir)
    let mut overrides = plugin.wine_dll_overrides(game_dir);
    for dll in plugin.wine_dll_overrides_from_staging(staging) {
        if !overrides.contains(&dll) {
            overrides.push(dll);
        }
    }
    if overrides.is_empty() {
        info!("no proxy DLLs detected, no Wine overrides needed");
        return Ok(report);
    }

    println!(
        "  Detected proxy DLLs needing Wine overrides: {}",
        overrides
            .iter()
            .map(|d| format!("{d}.dll"))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let launcher = modde_games::launcher::detect_launcher(game_dir);
    info!(?launcher, "detected game launcher");

    // Set WINEDLLOVERRIDES in the launcher config (Linux only — Wine/Proton concept)
    #[cfg(all(target_os = "linux", feature = "linux-integrations"))]
    {
        report.wine_overrides = modde_games::launcher::apply_wine_overrides(&launcher, &overrides)?;
    }

    // Collect tool env vars for the launch wrapper
    let tool_env_vars = match modde_core::db::ModdeDb::open().await {
        Ok(db) => {
            modde_games::launcher::collect_tool_env_vars(&modde_core::GameId::from(game_id), &db)
                .await
                .unwrap_or_default()
        }
        Err(_) => Vec::new(),
    };

    // Generate a launch wrapper that restores mod DLLs deleted by fgmod + exports tool env vars
    if let Some(wrapper) = modde_games::launcher::generate_launch_wrapper(
        game_dir,
        staging,
        &modde_core::GameId::from(game_id),
        &tool_env_vars,
    )? {
        report.wrapper_registration =
            modde_games::launcher::register_heroic_wrapper(&launcher, &wrapper.path)?;
        report.launch_wrapper = Some(wrapper);
    }

    print_launcher_configuration_report(&report);

    Ok(report)
}

/// Deploy MO2 mods/ staging layout to the game directory.
///
/// Walks `staging/mods/<ModName>/` and hardlinks files into `game_dir`,
/// preserving the internal directory structure (which is game-relative).
#[cfg(unix)]
fn is_same_file(src_meta: &std::fs::Metadata, dst_meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    src_meta.ino() == dst_meta.ino() && src_meta.dev() == dst_meta.dev()
}

#[cfg(not(unix))]
fn is_same_file(_src_meta: &std::fs::Metadata, _dst_meta: &std::fs::Metadata) -> bool {
    false
}

pub async fn deploy_mo2_to_game(staging: &Path, game_dir: &Path, force: bool) -> Result<()> {
    let mods_dir = staging.join("mods");
    if !mods_dir.exists() {
        info!("no mods/ directory in staging, skipping deployment");
        return Ok(());
    }

    let mut deployed = 0usize;
    let mut skipped = 0usize;

    let mut entries = tokio::fs::read_dir(&mods_dir).await?;
    while let Some(mod_entry) = entries.next_entry().await? {
        if !mod_entry.file_type().await?.is_dir() {
            continue;
        }

        let mod_name = mod_entry.file_name();
        let mod_path = mod_entry.path();

        // Walk all files in this mod directory
        let mut stack = vec![mod_path.clone()];
        while let Some(dir) = stack.pop() {
            let mut dir_entries = tokio::fs::read_dir(&dir).await?;
            while let Some(entry) = dir_entries.next_entry().await? {
                let file_type = entry.file_type().await?;
                let entry_path = entry.path();

                if file_type.is_dir() {
                    stack.push(entry_path);
                    continue;
                }

                // Get the relative path within the mod (game-relative)
                let rel_path = entry_path.strip_prefix(&mod_path).unwrap_or(&entry_path);

                // Skip MO2 metadata files
                let filename = rel_path.file_name().unwrap_or_default().to_string_lossy();
                if filename == "meta.ini" || filename == "meta.json" {
                    skipped += 1;
                    continue;
                }

                let dest = game_dir.join(rel_path);

                // Skip files already hardlinked to the source (same inode).
                if !force
                    && let (Ok(src_meta), Ok(dst_meta)) = (
                        tokio::fs::metadata(&entry_path).await,
                        tokio::fs::metadata(&dest).await,
                    )
                    && is_same_file(&src_meta, &dst_meta)
                {
                    skipped += 1;
                    continue;
                }

                let kind = modde_core::link::link_or_copy(&entry_path, &dest).await?;
                tracing::debug!(src = %entry_path.display(), dst = %dest.display(), ?kind, "deployed file");
                deployed += 1;
            }
        }

        info!(
            mod_name = ?mod_name,
            "deployed mod to game directory"
        );
    }

    println!("  Deployed {deployed} files to game directory ({skipped} metadata files skipped)");
    Ok(())
}
