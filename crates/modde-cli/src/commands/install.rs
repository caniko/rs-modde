use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tokio::sync::mpsc;
use tracing::{info, warn};

use modde_core::installer::{
    self as installer, DossierContext, InstallMethod, InstallStatus, InstallerError,
};
use modde_core::manifest::collection::CollectionManifest;
use modde_core::paths;
use modde_core::profile::{
    EnabledMod, LoadOrderLock, LockReason, Profile, ProfileManager, ProfileSource,
};
use modde_core::{ModdeDb, NexusFileId, NexusModId};
use modde_sources::nexus::api::NexusApi;
use modde_sources::nexus::auth::load_api_key;
use modde_sources::nexus::cdn::generate_download_link;
use modde_sources::wabbajack::installer::InstallProgress;

use crate::InstallSource;
use crate::commands::wabbajack::{acquire_missing, acquire_status_label};

/// Build a shared HTTP client with sensible timeouts for mod downloads.
fn build_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_mins(5))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client")
}

/// Persist a profile and update the selected game in shared settings.
async fn save_profile_and_settings(
    pm: &ProfileManager,
    profile: &Profile,
    game_dir: Option<&Path>,
) -> Result<()> {
    pm.create_or_update(profile).await?;
    let mut settings = modde_core::settings::AppSettings::load();
    if let Some(gd) = game_dir {
        settings.set_game_path(&profile.game_id, gd.to_path_buf());
    }
    settings.selected_game = Some(profile.game_id.to_string());
    settings.save();
    Ok(())
}

/// One-line rendering of a lock reason for CLI warnings. Intentionally
/// shorter than the `format_lock_reason` in `commands/profile.rs` (which
/// is used by `lock-info` output). Kept local so install.rs doesn't
/// depend on profile.rs internals.
fn format_lock_reason_short(reason: &LockReason) -> &'static str {
    match reason {
        LockReason::Wabbajack { .. } => "Wabbajack",
        LockReason::NexusCollection { .. } => "Nexus Collection",
        LockReason::TomlImport { .. } => "TOML import",
        LockReason::Manual { .. } => "manual",
    }
}

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
/// parsed and used directly — only the game domain still requires step 1.
async fn fetch_collection(
    client: &reqwest::Client,
    api_key: &str,
    slug: &str,
    version: Option<&str>,
) -> Result<CollectionManifest> {
    let api = NexusApi::new(client.clone(), api_key.to_string());

    let parsed_version = version.and_then(|v| v.parse::<u64>().ok());

    api.get_collection_by_slug(slug, parsed_version)
        .await
        .with_context(|| format!("failed to fetch collection '{slug}' from Nexus API"))
}

/// Download a file from a URL to a destination path.
async fn download_file(client: &reqwest::Client, url: &str, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let resp = modde_sources::error::status_error(
        client
            .get(url)
            .send()
            .await
            .context("download request failed")?,
    )
    .context("download returned error status")?;

    let bytes = resp.bytes().await.context("failed to read download body")?;
    tokio::fs::write(dest, &bytes)
        .await
        .context("failed to write downloaded file")?;

    Ok(())
}

/// Extract a zip archive to a destination directory.
fn extract_archive(archive_path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("failed to open archive: {}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read zip archive: {}", archive_path.display()))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(name) = entry.enclosed_name() else {
            warn!("skipping archive entry with unsafe path");
            continue;
        };
        let out_path = dest.join(name);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out_file = std::fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut out_file)?;
        }
    }

    Ok(())
}

/// Find the FOMOD ModuleConfig.xml path in a mod directory (case-insensitive).
pub fn find_fomod_config(mod_dir: &Path) -> Option<std::path::PathBuf> {
    let config_path = mod_dir.join("fomod").join("ModuleConfig.xml");
    if config_path.exists() {
        return Some(config_path);
    }
    // Case-insensitive fallback
    let Ok(entries) = std::fs::read_dir(mod_dir) else {
        return None;
    };
    for entry in entries.flatten() {
        if entry.file_name().eq_ignore_ascii_case("fomod") && entry.path().is_dir() {
            let Ok(inner) = std::fs::read_dir(entry.path()) else {
                continue;
            };
            for inner_entry in inner.flatten() {
                if inner_entry
                    .file_name()
                    .eq_ignore_ascii_case("moduleconfig.xml")
                {
                    return Some(inner_entry.path());
                }
            }
        }
    }
    None
}

/// Parse a Nexus mod URL to extract `game_domain`, `mod_id`, and optional `file_id`.
///
/// Supports URLs like:
/// - `https://www.nexusmods.com/skyrimspecialedition/mods/12345`
/// - `https://www.nexusmods.com/skyrimspecialedition/mods/12345?tab=files&file_id=67890`
fn parse_nexus_url(url: &str) -> Result<(String, NexusModId, Option<NexusFileId>)> {
    // Extract path segments: /GAME/mods/MOD_ID
    let url_parsed = url::Url::parse(url).context("invalid URL")?;

    let segments: Vec<&str> = url_parsed
        .path_segments()
        .map(std::iter::Iterator::collect)
        .unwrap_or_default();

    if segments.len() < 3 || segments[1] != "mods" {
        bail!("URL does not look like a Nexus mod URL: {url}");
    }

    let game_domain = segments[0].to_string();
    let mod_id = segments[2]
        .parse::<u64>()
        .map(NexusModId::from)
        .with_context(|| format!("invalid mod ID in URL: {}", segments[2]))?;

    // Check for file_id in query params
    let file_id = url_parsed
        .query_pairs()
        .find(|(k, _)| k == "file_id")
        .and_then(|(_, v)| v.parse::<u64>().ok().map(NexusFileId::from));

    Ok((game_domain, mod_id, file_id))
}

pub async fn handle(source: InstallSource) -> Result<()> {
    match source {
        InstallSource::NexusCollection {
            slug,
            version,
            profile,
        } => {
            handle_nexus_collection(slug, version, profile).await?;
        }
        InstallSource::Wabbajack {
            path,
            profile,
            game_dir,
            force,
            no_deploy,
            continue_on_error,
            reset_staging,
            skip_validate,
            diagnostics_dir,
            diagnostics_interval,
            stall_warn_seconds,
            stall_abort_seconds,
            archive_retention,
            missing_archive_policy,
            acquire_missing: _acquire_missing,
            acquire_download_dir,
            acquire_timeout,
            acquire_include_nexus,
            acquire_browser_controller,
            no_acquire_missing,
        } => {
            if !no_acquire_missing {
                let results = acquire_missing(
                    path.clone(),
                    acquire_download_dir,
                    None,
                    None,
                    acquire_include_nexus,
                    acquire_browser_controller,
                    acquire_timeout,
                    false,
                )
                .await?;
                let failed: Vec<_> = results
                    .iter()
                    .filter(|result| {
                        !matches!(
                            result.status,
                            modde_sources::wabbajack::acquire::AcquireStatus::Imported
                                | modde_sources::wabbajack::acquire::AcquireStatus::AlreadyPresent
                                | modde_sources::wabbajack::acquire::AcquireStatus::DirectResolved
                        )
                    })
                    .collect();
                if !failed.is_empty()
                    && matches!(
                        missing_archive_policy,
                        crate::WabbajackMissingArchivePolicyArg::Fail
                    )
                {
                    let details = failed
                        .iter()
                        .map(|result| {
                            format!(
                                "{} {:016x} {}",
                                acquire_status_label(&result.status),
                                result.archive.hash,
                                result.archive.name
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    anyhow::bail!(
                        "frontloaded Wabbajack archive acquisition did not complete:\n{details}"
                    );
                }
            }
            handle_wabbajack(modde_sources::wabbajack::runner::WabbajackInstallOptions {
                db: None,
                path,
                profile_name: profile,
                game_dir,
                force,
                no_deploy,
                safety: modde_sources::wabbajack::runner::WabbajackInstallSafety {
                    continue_on_error,
                    reset_staging,
                    skip_validate,
                },
                diagnostics: diagnostics_dir.map(|dir| {
                    modde_sources::wabbajack::diagnostics::WabbajackDiagnosticsOptions {
                        dir,
                        interval: std::time::Duration::from_secs(diagnostics_interval.max(1)),
                        stall_warn: std::time::Duration::from_secs(stall_warn_seconds.max(1)),
                        stall_abort: std::time::Duration::from_secs(stall_abort_seconds.max(1)),
                    }
                }),
                archive_retention: archive_retention.into(),
                missing_archive_policy: missing_archive_policy.into(),
            })
            .await?;
        }
        InstallSource::Mod { url, profile, .. } => {
            handle_single_mod(url, profile).await?;
        }
    }
    Ok(())
}

async fn handle_nexus_collection(
    slug: String,
    version: Option<String>,
    profile_name: Option<String>,
) -> Result<()> {
    info!(%slug, ?version, ?profile_name, "installing Nexus Collection");

    let api_key = load_api_key().context("failed to load Nexus API key")?;
    let client = build_http_client()?;

    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile_name = profile_name.unwrap_or_else(|| slug.clone());

    // Two-step fetch: discover game_domain from slug, then fetch the full manifest.
    let manifest = fetch_collection(&client, &api_key, &slug, version.as_deref())
        .await
        .context("failed to fetch collection manifest")?;
    let transaction = modde_sources::resolution::preflight_collection_transaction(&manifest)
        .context("collection dependency transaction is not solvable")?;

    let game_domain = manifest.game.domain_name.clone();
    let collection_version = manifest.version.version.clone();

    println!(
        "Collection: {} by {} ({} mods, {} solved artifacts)",
        manifest.name,
        manifest.author.name,
        manifest.mods.len(),
        transaction.artifacts.len()
    );

    let store = paths::store_dir();
    let mut enabled_mods = Vec::new();

    // Sort mods by install_order
    let mut mods = manifest.mods.clone();
    mods.sort_by_key(|m| m.install_order);

    for collection_mod in &mods {
        let mod_name = &collection_mod.name;
        let mod_id = collection_mod.mod_id;
        let file_id = collection_mod.file_id;

        println!("  Installing: {mod_name} (mod {mod_id}, file {file_id})");

        let mod_store_dir = store.join(format!("{game_domain}_{mod_id}_{file_id}"));

        if mod_store_dir.exists() {
            println!("    (already downloaded, skipping)");
        } else {
            // Generate download link and download
            let download_url =
                generate_download_link(&client, &api_key, &game_domain, mod_id, file_id)
                    .await
                    .with_context(|| format!("failed to get download link for {mod_name}"))?;

            let archive_path = store.join(format!("{mod_id}_{file_id}.zip"));
            download_file(&client, &download_url, &archive_path)
                .await
                .with_context(|| format!("failed to download {mod_name}"))?;

            // Extract archive
            std::fs::create_dir_all(&mod_store_dir)?;
            extract_archive(&archive_path, &mod_store_dir)
                .with_context(|| format!("failed to extract {mod_name}"))?;

            // Clean up archive after extraction
            let _ = std::fs::remove_file(&archive_path);
        }

        // Detect FOMOD - for automated installs we apply default selections
        if find_fomod_config(&mod_store_dir).is_some() {
            info!(%mod_name, "FOMOD installer detected, applying defaults");
            // FOMOD will be applied during deploy phase with full context
        }

        let mod_id_str = format!("{game_domain}_{mod_id}_{file_id}");
        enabled_mods.push(EnabledMod {
            mod_id: mod_id_str,
            display_name: Some(collection_mod.name.clone()),
            enabled: !collection_mod.optional,
            version: Some(collection_mod.version.clone()),
            fomod_config: None,
            ..Default::default()
        });
    }

    // Create and save profile. Collections define a canonical install
    // order, so we stamp a matching `LockReason::NexusCollection` lock —
    // preventing accidental reorder until the user explicitly unlocks.
    let profile = Profile {
        id: None,
        name: profile_name.clone(),
        game_id: modde_core::GameId::from(game_domain.clone()),
        source: ProfileSource::NexusCollection {
            slug: slug.clone(),
            version: collection_version.clone(),
        },
        mods: enabled_mods,
        overrides: paths::modde_data_dir()
            .join("profiles")
            .join(&profile_name)
            .join("overrides"),
        load_order_rules: smallvec::SmallVec::new(),
        load_order_lock: Some(LoadOrderLock::now(LockReason::NexusCollection {
            slug: slug.clone(),
            version: collection_version,
        })),
    };

    save_profile_and_settings(&pm, &profile, None).await?;

    println!(
        "Collection '{slug}' installed to profile '{profile_name}' ({} mods)",
        profile.mods.len()
    );

    Ok(())
}

async fn handle_wabbajack(
    options: modde_sources::wabbajack::runner::WabbajackInstallOptions,
) -> Result<()> {
    let manifest = modde_sources::wabbajack::runner::parse_wabbajack_manifest(&options.path)?;
    println!(
        "Wabbajack modlist: {} by {} (game: {})",
        manifest.name, manifest.author, manifest.game
    );
    println!(
        "  {} archives, {} directives",
        manifest.archives.len(),
        manifest.directives.len()
    );

    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
    let progress_handle = tokio::spawn(async move {
        while let Some(progress) = progress_rx.recv().await {
            match progress {
                InstallProgress::Starting { total_downloads } => {
                    println!("  Starting install: {total_downloads} downloads");
                }
                InstallProgress::DownloadComplete { name } => {
                    println!("  Downloaded: {name}");
                }
                InstallProgress::Applying {
                    directive_index,
                    total,
                } if (directive_index % 100 == 0 || directive_index == total - 1) => {
                    println!("  Applying directives: {}/{total}", directive_index + 1);
                }
                InstallProgress::Patching { name } => {
                    println!("  Patching: {name}");
                }
                InstallProgress::CreatingBSA { name } => {
                    println!("  Creating BSA: {name}");
                }
                InstallProgress::LauncherConfigured { report } => {
                    print_launcher_configuration_report(&report);
                }
                InstallProgress::StagingAdopted {
                    archive_batches,
                    create_bsa,
                } => {
                    println!(
                        "  Adopted existing staging: {archive_batches} archive batches, {create_bsa} BSA outputs"
                    );
                }
                InstallProgress::Complete => {
                    println!("  Install pipeline complete");
                }
                InstallProgress::Failed { error } => {
                    eprintln!("  Install failed: {error}");
                }
                _ => {}
            }
        }
    });

    let summary =
        modde_sources::wabbajack::runner::install_wabbajack(options, Some(progress_tx)).await?;
    progress_handle.await?;

    println!(
        "Wabbajack modlist '{}' installed to profile '{profile_name}' ({} mods)",
        summary.modlist_name,
        summary.mod_count,
        profile_name = summary.profile_name
    );

    Ok(())
}

/// Detect proxy DLLs in the game directory, configure Wine DLL overrides,
/// and generate a launch wrapper to restore DLLs that fgmod deletes.
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

async fn handle_single_mod(url: String, profile_name: Option<String>) -> Result<()> {
    info!(%url, ?profile_name, "installing mod from Nexus");

    // Parse the Nexus URL
    let (game_domain, mod_id, file_id_opt) =
        parse_nexus_url(&url).context("failed to parse mod URL")?;

    let api_key = load_api_key().context("failed to load Nexus API key")?;
    let client = build_http_client()?;

    // If no file_id provided, fetch mod files and select the most-recent MAIN file.
    let file_id = if let Some(id) = file_id_opt {
        id
    } else {
        let api = NexusApi::new(client.clone(), api_key.clone());
        let files = api
            .get_mod_files(&game_domain, mod_id)
            .await
            .context("failed to fetch mod files")?;

        // Prefer MAIN files sorted by upload timestamp (newest first).
        // Fall back to the first file if no MAIN category exists.
        let mut candidates: Vec<_> = files
            .files
            .into_iter()
            .filter(|f| f.category_name.as_deref() == Some("MAIN"))
            .collect();
        candidates.sort_by_key(|f| std::cmp::Reverse(f.uploaded_timestamp));

        let file = candidates
            .into_iter()
            .next()
            .or_else(|| {
                warn!(%mod_id, "no MAIN category file found, using first available file");
                None
            })
            .ok_or_else(|| anyhow::anyhow!("no files found for mod {mod_id}"))?;

        file.file_id
    };

    // Fetch mod metadata from Nexus for the display name.
    let api = NexusApi::new(client.clone(), api_key.clone());
    let mod_info = api
        .get_mod(&game_domain, mod_id)
        .await
        .context("failed to fetch mod info from Nexus")?;

    println!(
        "Installing mod: {} ({game_domain}/mods/{mod_id}, file {file_id})",
        mod_info.name
    );

    let store = paths::store_dir();
    let mod_store_dir = store.join(format!("{game_domain}_{mod_id}_{file_id}"));

    // Extract into a temp staging dir rather than straight into the
    // store, so `installer::analyze` runs on the raw archive tree and
    // `installer::execute` can decide the final staging layout. The
    // archive is kept until execution is committed so we can compute
    // its source hash and also dump the dossier from it if needed.
    let archive_path = store.join(format!("{mod_id}_{file_id}.zip"));
    let staging_root =
        paths::staging_dir().join(format!("install_{game_domain}_{mod_id}_{file_id}"));

    let mut install_outcome = InstallOutcome::AlreadyStaged;
    if mod_store_dir.exists() {
        println!("  Already in store, skipping download");
    } else {
        // Fresh install: download + extract + analyze + execute.
        let download_url = generate_download_link(&client, &api_key, &game_domain, mod_id, file_id)
            .await
            .context("failed to get download link")?;
        download_file(&client, &download_url, &archive_path)
            .await
            .context("failed to download mod")?;

        // Extract into staging (not the final store dir).
        if staging_root.exists() {
            let _ = std::fs::remove_dir_all(&staging_root);
        }
        std::fs::create_dir_all(&staging_root)?;
        installer::extract_archive(&archive_path, &staging_root)
            .context("failed to extract mod archive")?;

        let source_hash = installer::xxh64_file_hex(&archive_path)
            .context("failed to hash downloaded archive")?;
        // Archive is no longer needed once we have the hash.
        let _ = std::fs::remove_file(&archive_path);

        // Resolve the game plugin to build a probe. Games we don't
        // recognize yet still install through the generic pipeline,
        // just without game-specific hints.
        // Probe lookup tries the modde `game_id` first (in case the
        // caller passed one through), then falls back to the Nexus
        // domain — Nexus URLs carry the domain (e.g. "stellarblade"),
        // which doesn't match `game_id` (e.g. "stellar-blade").
        let probe = modde_games::resolve_game_plugin(&game_domain)
            .or_else(|| modde_games::resolve_game_plugin_by_nexus_domain(&game_domain))
            .map_or_else(installer::InstallProbe::noop, modde_games::game_probe);

        let mut plan = installer::analyze(&staging_root, &probe, source_hash)
            .context("installer analyze failed")?;
        info!(method = plan.method.label(), "install plan decided");

        install_outcome = match &plan.method {
            InstallMethod::Unknown { .. } => {
                let dossier = write_unknown_dossier(
                    &staging_root,
                    &game_domain,
                    mod_id,
                    file_id,
                    &mod_info,
                    &plan.method,
                    &plan.source_archive_hash,
                )?;
                InstallOutcome::Unknown {
                    dossier_path: dossier,
                }
            }
            _ if !plan.method.is_ready() => {
                // FOMOD / BAIN with no config yet — copy the raw
                // extracted tree into the store so the UI wizard can
                // walk it later without re-downloading. The archive
                // itself is already gone (removed above once hashed).
                std::fs::create_dir_all(&mod_store_dir)?;
                copy_dir_tree(&staging_root, &mod_store_dir)
                    .context("failed to copy staging → store for pending install")?;
                InstallOutcome::PendingUserInput {
                    method: plan.method.label().to_string(),
                }
            }
            _ => {
                std::fs::create_dir_all(&mod_store_dir)?;
                match installer::execute(&mut plan, &staging_root, &mod_store_dir) {
                    Ok(files) => {
                        println!(
                            "  Staged {} files into {}",
                            files.len(),
                            mod_store_dir.display()
                        );
                        InstallOutcome::Installed { plan }
                    }
                    Err(InstallerError::UnknownMethod { reason: _ }) => {
                        let dossier = write_unknown_dossier(
                            &staging_root,
                            &game_domain,
                            mod_id,
                            file_id,
                            &mod_info,
                            &plan.method,
                            &plan.source_archive_hash,
                        )?;
                        InstallOutcome::Unknown {
                            dossier_path: dossier,
                        }
                    }
                    Err(InstallerError::RequiresUserInput { method }) => {
                        InstallOutcome::PendingUserInput {
                            method: method.to_string(),
                        }
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        };

        // Clean up the staging dir regardless of outcome — anything
        // important has already been copied into the store or the
        // dossier directory.
        let _ = std::fs::remove_dir_all(&staging_root);
    }

    // Load or create profile.
    //
    // Lock policy for `modde install mod` (single-Nexus-mod flow):
    //   - **New profile** (not yet in DB) → `load_order_lock: None`.
    //     Fresh profiles start Manual; if the user wants to later mark
    //     them authoritative they run `modde profile lock`.
    //   - **Existing profile** → preserve whatever lock is already there
    //     (Wabbajack / Collection / TomlImport / Manual). If a lock is
    //     present, emit a warning because adding a mod drifts the load
    //     order away from the authoritative source — the lock exists
    //     precisely to prevent that kind of drift.
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile_name = profile_name.unwrap_or_else(|| game_domain.clone());
    let mod_id_str = format!("{game_domain}_{mod_id}_{file_id}");

    let mut profile = match pm.load(&profile_name, None).await {
        Ok(p) => p,
        Err(_) => Profile {
            id: None,
            name: profile_name.clone(),
            game_id: modde_core::GameId::from(game_domain.clone()),
            source: ProfileSource::Manual,
            mods: Vec::new(),
            overrides: ProfileManager::default_overrides(&profile_name),
            load_order_rules: smallvec::SmallVec::new(),
            load_order_lock: None,
        },
    };

    if let Some(lock) = profile.load_order_lock.as_ref() {
        eprintln!(
            "  warning: profile '{profile_name}' is locked ({}). Adding a mod will \
             drift from the authoritative source. Run `modde profile unlock \
             {profile_name}` first if this is intentional.",
            format_lock_reason_short(&lock.reason),
        );
    }

    // Add mod to profile if not already present
    let status = install_outcome.status();
    if !profile.mods.iter().any(|m| m.mod_id == mod_id_str) {
        profile.mods.push(EnabledMod {
            mod_id: mod_id_str.clone(),
            display_name: Some(mod_info.name.clone()),
            enabled: true,
            version: Some(mod_info.version.clone()),
            nexus_mod_id: Some(mod_id),
            nexus_file_id: Some(file_id),
            nexus_game_domain: Some(game_domain.clone()),
            install_status: Some(status),
            fomod_config: None,
            ..Default::default()
        });
    }

    save_profile_and_settings(&pm, &profile, None).await?;

    // If analyze+execute succeeded, wire the plan into the DB so
    // uninstall can remove the exact file list later.
    if let InstallOutcome::Installed { plan } = &install_outcome {
        let db = ModdeDb::open()
            .await
            .context("failed to open mod db for record_install")?;
        let profile_id = pm
            .load(&profile_name, None)
            .await
            .context("failed to reload profile to get id")?
            .id
            .ok_or_else(|| anyhow::anyhow!("saved profile has no database id"))?;
        db.record_install(
            profile_id,
            &modde_core::ModId::from(mod_id_str.as_str()),
            plan,
            InstallStatus::Installed,
        )
        .await
        .context("failed to persist install plan")?;
    }

    match install_outcome {
        InstallOutcome::Installed { .. } | InstallOutcome::AlreadyStaged => {
            println!("Mod '{mod_id_str}' added to profile '{profile_name}'");
        }
        InstallOutcome::PendingUserInput { method } => {
            println!(
                "Mod '{mod_id_str}' staged. Install method '{method}' needs user input — \
                 open the UI to complete the wizard."
            );
        }
        InstallOutcome::Unknown { dossier_path } => {
            println!("Mod '{mod_id_str}' has an unknown install layout. Dossier written to:");
            println!("  {}", dossier_path.display());
            println!(
                "Run `/modde-installer {mod_id_str}` inside Claude Code to extend modde \
                 with a handler for this layout."
            );
        }
    }

    Ok(())
}

/// High-level result of the Phase 4 pipeline, consumed by the profile-
/// write step below.
enum InstallOutcome {
    Installed {
        plan: modde_core::installer::InstallPlan,
    },
    PendingUserInput {
        method: String,
    },
    Unknown {
        dossier_path: PathBuf,
    },
    /// The mod was already in the store before we ran — no fresh plan
    /// was produced, so we only refresh the profile row.
    AlreadyStaged,
}

impl InstallOutcome {
    fn status(&self) -> InstallStatus {
        match self {
            InstallOutcome::Installed { .. } | InstallOutcome::AlreadyStaged => {
                InstallStatus::Installed
            }
            InstallOutcome::PendingUserInput { .. } => InstallStatus::PendingUserInput,
            InstallOutcome::Unknown { .. } => InstallStatus::Unknown,
        }
    }
}

/// Write a dossier for an unknown-layout mod and return the dossier
/// directory path so the CLI can tell the user where to look.
fn write_unknown_dossier(
    extracted_dir: &Path,
    game_domain: &str,
    mod_id: NexusModId,
    file_id: NexusFileId,
    mod_info: &modde_sources::nexus::api::NexusMod,
    method: &InstallMethod,
    source_hash: &str,
) -> Result<PathBuf> {
    let ctx = DossierContext {
        game_id: game_domain.to_string(),
        game_domain: Some(game_domain.to_string()),
        nexus_mod_id: Some(mod_id),
        nexus_file_id: Some(file_id),
        mod_name: mod_info.name.clone(),
        mod_author: Some(mod_info.author.clone()),
        mod_version: Some(mod_info.version.clone()),
        mod_summary: mod_info.summary.clone(),
        nexus_url: Some(format!(
            "https://www.nexusmods.com/{game_domain}/mods/{mod_id}"
        )),
        source_archive_hash: source_hash.to_string(),
    };
    let trace = vec![installer::ProbeTrace {
        probe: "generic+game".to_string(),
        matched: false,
        note: format!("verdict: {}", method.label()),
    }];
    installer::dump_dossier(extracted_dir, &ctx, method, trace)
        .context("failed to write unknown-installer dossier")
}

/// Recursively copy `src` into `dst`, preserving directory structure.
/// Used for the `PendingUserInput` path where we want to keep the raw
/// archive around so the UI wizard can walk it, without touching the
/// files we would otherwise move via `installer::execute`.
fn copy_dir_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !src.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_tree(&src_path, &dst_path)?;
        } else if src_path.is_file() {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    // ── parse_nexus_url ──────────────────────────────────────────────

    #[test]
    fn parse_nexus_url_basic_mod_url() {
        let (game, mod_id, file_id) =
            parse_nexus_url("https://www.nexusmods.com/skyrimspecialedition/mods/12345").unwrap();
        assert_eq!(game, "skyrimspecialedition");
        assert_eq!(mod_id, NexusModId::from(12345));
        assert_eq!(file_id, None);
    }

    #[test]
    fn parse_nexus_url_with_file_id() {
        let (game, mod_id, file_id) = parse_nexus_url(
            "https://www.nexusmods.com/skyrimspecialedition/mods/12345?tab=files&file_id=67890",
        )
        .unwrap();
        assert_eq!(game, "skyrimspecialedition");
        assert_eq!(mod_id, NexusModId::from(12345));
        assert_eq!(file_id, Some(NexusFileId::from(67890)));
    }

    #[test]
    fn parse_nexus_url_file_id_only_query_param() {
        let (game, mod_id, file_id) =
            parse_nexus_url("https://www.nexusmods.com/fallout4/mods/999?file_id=42").unwrap();
        assert_eq!(game, "fallout4");
        assert_eq!(mod_id, NexusModId::from(999));
        assert_eq!(file_id, Some(NexusFileId::from(42)));
    }

    #[test]
    fn parse_nexus_url_trailing_slash() {
        // Trailing slash means the 4th segment is empty, but segments[0..3] still work.
        let result = parse_nexus_url("https://www.nexusmods.com/skyrimspecialedition/mods/12345/");
        assert!(result.is_ok());
        let (game, mod_id, _) = result.unwrap();
        assert_eq!(game, "skyrimspecialedition");
        assert_eq!(mod_id, NexusModId::from(12345));
    }

    #[test]
    fn parse_nexus_url_invalid_not_a_url() {
        let result = parse_nexus_url("not-a-url");
        assert!(result.is_err());
    }

    #[test]
    fn parse_nexus_url_invalid_wrong_path_structure() {
        let result = parse_nexus_url("https://www.nexusmods.com/skyrimspecialedition");
        assert!(result.is_err());
    }

    #[test]
    fn parse_nexus_url_invalid_missing_mods_segment() {
        let result = parse_nexus_url("https://www.nexusmods.com/skyrimspecialedition/files/12345");
        assert!(result.is_err());
    }

    #[test]
    fn parse_nexus_url_invalid_non_numeric_mod_id() {
        let result = parse_nexus_url("https://www.nexusmods.com/skyrimspecialedition/mods/abc");
        assert!(result.is_err());
    }

    #[test]
    fn parse_nexus_url_different_game_domains() {
        for domain in &["fallout4", "cyberpunk2077", "morrowind", "oblivion"] {
            let url = format!("https://www.nexusmods.com/{domain}/mods/1");
            let (game, mod_id, _) = parse_nexus_url(&url).unwrap();
            assert_eq!(game, *domain);
            assert_eq!(mod_id, NexusModId::from(1));
        }
    }

    // ── find_fomod_config / has_fomod ────────────────────────────────

    #[test]
    fn find_fomod_config_exact_case() {
        let tmp = tempfile::tempdir().unwrap();
        let fomod = tmp.path().join("fomod");
        std::fs::create_dir_all(&fomod).unwrap();
        std::fs::write(fomod.join("ModuleConfig.xml"), "<config/>").unwrap();

        let result = find_fomod_config(tmp.path());
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("ModuleConfig.xml"));
    }

    #[test]
    fn find_fomod_config_uppercase_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let fomod = tmp.path().join("FOMOD");
        std::fs::create_dir_all(&fomod).unwrap();
        std::fs::write(fomod.join("moduleconfig.xml"), "<config/>").unwrap();

        let result = find_fomod_config(tmp.path());
        assert!(result.is_some());
    }

    #[test]
    fn find_fomod_config_mixed_case() {
        let tmp = tempfile::tempdir().unwrap();
        let fomod = tmp.path().join("FoMod");
        std::fs::create_dir_all(&fomod).unwrap();
        std::fs::write(fomod.join("MODULECONFIG.XML"), "<config/>").unwrap();

        let result = find_fomod_config(tmp.path());
        assert!(result.is_some());
    }

    #[test]
    fn find_fomod_config_not_present() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("data")).unwrap();

        let result = find_fomod_config(tmp.path());
        assert!(result.is_none());
    }

    #[test]
    fn find_fomod_config_empty_fomod_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("fomod")).unwrap();
        // No ModuleConfig.xml inside

        let result = find_fomod_config(tmp.path());
        assert!(result.is_none());
    }

    #[test]
    fn has_fomod_returns_true_when_present() {
        let tmp = tempfile::tempdir().unwrap();
        let fomod = tmp.path().join("fomod");
        std::fs::create_dir_all(&fomod).unwrap();
        std::fs::write(fomod.join("ModuleConfig.xml"), "<config/>").unwrap();

        assert!(find_fomod_config(tmp.path()).is_some());
    }

    #[test]
    fn has_fomod_returns_false_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(find_fomod_config(tmp.path()).is_none());
    }

    // ── extract_archive ──────────────────────────────────────────────

    fn create_test_zip(dir: &Path, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
        let zip_path = dir.join(name);
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        for (entry_name, content) in entries {
            writer.start_file(entry_name.to_string(), options).unwrap();
            writer.write_all(content).unwrap();
        }
        writer.finish().unwrap();
        zip_path
    }

    #[test]
    fn extract_archive_simple_zip() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = create_test_zip(
            tmp.path(),
            "test.zip",
            &[
                ("hello.txt", b"Hello, world!"),
                ("data/readme.md", b"# Readme"),
            ],
        );

        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();
        extract_archive(&zip_path, &dest).unwrap();

        assert_eq!(
            std::fs::read_to_string(dest.join("hello.txt")).unwrap(),
            "Hello, world!"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("data/readme.md")).unwrap(),
            "# Readme"
        );
    }

    #[test]
    fn extract_archive_nested_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = create_test_zip(
            tmp.path(),
            "nested.zip",
            &[
                ("a/b/c/deep.txt", b"deep content"),
                ("top.txt", b"top content"),
            ],
        );

        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();
        extract_archive(&zip_path, &dest).unwrap();

        assert!(dest.join("a/b/c/deep.txt").exists());
        assert_eq!(
            std::fs::read_to_string(dest.join("a/b/c/deep.txt")).unwrap(),
            "deep content"
        );
    }

    #[test]
    fn extract_archive_empty_zip() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = create_test_zip(tmp.path(), "empty.zip", &[]);

        let dest = tmp.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();
        extract_archive(&zip_path, &dest).unwrap();

        // dest exists but is empty
        let count = std::fs::read_dir(&dest).unwrap().count();
        assert_eq!(count, 0);
    }

    #[test]
    fn extract_archive_nonexistent_file() {
        let tmp = tempfile::tempdir().unwrap();
        let result = extract_archive(&tmp.path().join("noexist.zip"), &tmp.path().join("out"));
        assert!(result.is_err());
    }
}
