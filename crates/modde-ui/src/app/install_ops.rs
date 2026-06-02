use std::path::PathBuf;

/// Run the full install pipeline for a single Nexus mod, invoked from
/// the Browse Nexus **Install** button. Owned as a free function so
/// the `update()` arm can hand it to `Task::perform` without borrowing
/// `self`.
pub(super) async fn run_browse_install(
    db: modde_core::db::ModdeDb,
    game_domain: String,
    mod_id: modde_core::NexusModId,
) -> Result<String, String> {
    let api_key = modde_sources::nexus::auth::load_api_key().map_err(|e| e.to_string())?;
    let client = reqwest::Client::new();
    let api = modde_sources::nexus::api::NexusApi::new(client.clone(), api_key.clone());

    // Look up the latest MAIN file id.
    let files = api
        .get_mod_files(&game_domain, mod_id)
        .await
        .map_err(|e| e.to_string())?;
    let mut candidates: Vec<_> = files
        .files
        .into_iter()
        .filter(|f| f.category_name.as_deref() == Some("MAIN"))
        .collect();
    candidates.sort_by_key(|f| std::cmp::Reverse(f.uploaded_timestamp));
    let file_id = candidates
        .first()
        .map(|f| f.file_id)
        .ok_or_else(|| format!("no MAIN file found for mod {mod_id}"))?;

    let mod_info = api
        .get_mod(&game_domain, mod_id)
        .await
        .map_err(|e| e.to_string())?;

    // Build a probe from whichever game plugin is registered. Try the
    // modde `game_id` first, then the Nexus domain — Nexus URLs carry
    // the domain (e.g. "stellarblade") which doesn't match `game_id`
    // (e.g. "stellar-blade").
    let probe = modde_games::resolve_game_plugin(&game_domain)
        .or_else(|| modde_games::resolve_game_plugin_by_nexus_domain(&game_domain))
        .map_or_else(
            modde_core::installer::InstallProbe::noop,
            modde_games::game_probe,
        );

    let outcome = modde_sources::nexus::install::install_single_mod(
        &client,
        &api_key,
        &game_domain,
        mod_id,
        file_id,
        &mod_info,
        &probe,
    )
    .await
    .map_err(|e| e.to_string())?;

    use modde_core::installer::InstallStatus;
    use modde_sources::nexus::install::InstallOutcome;

    let mod_id_str = format!("{game_domain}_{mod_id}_{file_id}");
    let pm = modde_core::profile::ProfileManager::with_db(db.clone());

    // Prefer an existing profile for the game; fall back to creating a
    // Manual profile named after the game domain if none exist.
    let profile_name = crate::app::block_on(pm.list())
        .ok()
        .and_then(|profiles| {
            profiles
                .into_iter()
                .find(|p| p.game_id.as_str() == game_domain)
                .map(|p| p.name)
        })
        .unwrap_or_else(|| game_domain.clone());
    let mut profile = match crate::app::block_on(pm.load(&profile_name, None)) {
        Ok(p) => p,
        Err(_) => modde_core::profile::Profile {
            id: None,
            name: profile_name.clone(),
            game_id: modde_core::resolver::GameId::from(game_domain.clone()),
            source: modde_core::profile::ProfileSource::Manual,
            mods: Vec::new(),
            overrides: modde_core::profile::ProfileManager::default_overrides(&profile_name),
            load_order_rules: smallvec::SmallVec::new(),
            load_order_lock: None,
        },
    };
    let status = match &outcome {
        InstallOutcome::Installed(_) | InstallOutcome::AlreadyStaged => InstallStatus::Installed,
        InstallOutcome::PendingUserInput { .. } => InstallStatus::PendingUserInput,
        InstallOutcome::Unknown { .. } => InstallStatus::Unknown,
    };
    if !profile.mods.iter().any(|m| m.mod_id == mod_id_str) {
        profile.mods.push(modde_core::profile::EnabledMod {
            mod_id: mod_id_str.clone(),
            display_name: Some(mod_info.name.clone()),
            enabled: true,
            version: Some(mod_info.version.clone()),
            nexus_mod_id: Some(mod_id),
            nexus_file_id: Some(file_id),
            nexus_game_domain: Some(game_domain.clone()),
            install_status: Some(status),
            ..Default::default()
        });
    }
    crate::app::block_on(pm.create_or_update(&profile)).map_err(|e| e.to_string())?;

    if let InstallOutcome::Installed(plan) = &outcome {
        let profile_id = crate::app::block_on(pm.load(&profile_name, None))
            .map_err(|e| e.to_string())?
            .id
            .ok_or_else(|| "saved profile has no id".to_string())?;
        crate::app::block_on(db.record_install(
            profile_id,
            &modde_core::ModId::from(mod_id_str.as_str()),
            plan,
            InstallStatus::Installed,
        ))
        .map_err(|e| e.to_string())?;
    }

    Ok(match outcome {
        InstallOutcome::Installed(_) | InstallOutcome::AlreadyStaged => {
            format!("Installed '{}'", mod_info.name)
        }
        InstallOutcome::PendingUserInput { method } => {
            format!("'{}' needs {method} wizard", mod_info.name)
        }
        InstallOutcome::Unknown { dossier_path, .. } => {
            format!(
                "Unknown install layout — dossier: {}",
                dossier_path.display()
            )
        }
    })
}

pub(super) fn format_anyhow_error(error: anyhow::Error) -> String {
    error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

pub(super) async fn download_wabbajack_source(source: String) -> Result<PathBuf, String> {
    let client = reqwest::Client::new();
    let url = if modde_sources::wabbajack::catalog::is_remote_url(&source) {
        source
    } else if std::path::Path::new(&source).exists() {
        return Ok(PathBuf::from(source));
    } else {
        modde_sources::wabbajack::catalog::resolve_download_target(
            &client,
            &source,
            modde_sources::wabbajack::catalog::CatalogSource::Both,
        )
        .await
        .map_err(|e| e.to_string())?
    };
    let output = modde_core::paths::downloads_dir().join("wabbajack");
    modde_sources::wabbajack::catalog::download_wabbajack_file(&client, &url, &output)
        .await
        .map_err(|e| e.to_string())
}

pub(super) async fn run_wabbajack_install_for_ui(
    db: modde_core::db::ModdeDb,
    path: PathBuf,
    profile_name: Option<String>,
    game_dir: Option<PathBuf>,
) -> Result<(String, Vec<String>), String> {
    tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        runtime.block_on(async move {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let collector = tokio::spawn(async move {
                let mut lines = Vec::new();
                while let Some(progress) = rx.recv().await {
                    lines.push(format_install_progress(&progress));
                }
                lines
            });
            let summary = modde_sources::wabbajack::runner::install_wabbajack(
                modde_sources::wabbajack::runner::WabbajackInstallOptions {
                    db: Some(db),
                    path,
                    profile_name,
                    game_dir,
                    force: false,
                    no_deploy: false,
                    safety: modde_sources::wabbajack::runner::WabbajackInstallSafety::default(),
                    diagnostics: None,
                    archive_retention:
                        modde_sources::wabbajack::installer::ArchiveRetentionPolicy::Keep,
                    missing_archive_policy:
                        modde_sources::wabbajack::impact::MissingArchivePolicy::Fail,
                },
                Some(tx),
            )
            .await
            .map_err(|e| e.to_string())?;
            let lines = collector.await.map_err(|e| e.to_string())?;
            Ok((
                format!(
                    "Installed '{}' to profile '{}' ({} mods)",
                    summary.modlist_name, summary.profile_name, summary.mod_count
                ),
                lines,
            ))
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(super) fn format_install_progress(
    progress: &modde_sources::wabbajack::installer::InstallProgress,
) -> String {
    use modde_sources::wabbajack::installer::InstallProgress;
    match progress {
        InstallProgress::Starting { total_downloads } => {
            format!("Starting install: {total_downloads} downloads")
        }
        InstallProgress::Downloading { name, bytes, total } => {
            format!("Downloading {name}: {bytes}/{total} bytes")
        }
        InstallProgress::DownloadComplete { name } => format!("Downloaded: {name}"),
        InstallProgress::Verifying { name } => format!("Verifying: {name}"),
        InstallProgress::Applying {
            directive_index,
            total,
        } => format!("Applying directives: {}/{total}", directive_index + 1),
        InstallProgress::Patching { name } => format!("Patching: {name}"),
        InstallProgress::CreatingBSA { name } => format!("Creating BSA: {name}"),
        InstallProgress::LauncherConfigured { report } => {
            format_launcher_configuration_progress(report)
        }
        InstallProgress::InlineFile { name } => format!("Writing inline file: {name}"),
        InstallProgress::StagingAdopted {
            archive_batches,
            create_bsa,
        } => format!(
            "Adopted existing staging: {archive_batches} archive batches, {create_bsa} BSA outputs"
        ),
        InstallProgress::Complete => "Install pipeline complete".to_string(),
        InstallProgress::Failed { error } => format!("Install failed: {error}"),
    }
}

pub(super) fn format_launcher_configuration_progress(
    report: &modde_games::launcher::LauncherConfigurationReport,
) -> String {
    let mut parts = Vec::new();
    if let Some(wine_overrides) = &report.wine_overrides {
        parts.push(match wine_overrides {
            modde_games::launcher::WineOverrideReport::HeroicUpdated { .. } => {
                "Updated Heroic Wine DLL overrides".to_string()
            }
            modde_games::launcher::WineOverrideReport::SteamInstruction { .. } => {
                "Steam launch options need Wine DLL overrides".to_string()
            }
            modde_games::launcher::WineOverrideReport::UnknownInstruction { .. } => {
                "Game launch environment needs Wine DLL overrides".to_string()
            }
        });
    }
    if let Some(wrapper) = &report.launch_wrapper {
        parts.push(format!(
            "Generated launch wrapper ({} DLL restores, {} tool env vars)",
            wrapper.restore_count, wrapper.tool_env_var_count
        ));
    }
    if let Some(registration) = &report.wrapper_registration {
        parts.push(match registration {
            modde_games::launcher::WrapperRegistrationReport::HeroicRegistered => {
                "Registered launch wrapper in Heroic".to_string()
            }
            modde_games::launcher::WrapperRegistrationReport::ManualInstruction { .. } => {
                "Launch wrapper needs manual launcher setup".to_string()
            }
        });
    }

    if parts.is_empty() {
        "Launcher configuration unchanged".to_string()
    } else {
        parts.join("; ")
    }
}

pub(super) fn slugify_profile_name(name: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}
