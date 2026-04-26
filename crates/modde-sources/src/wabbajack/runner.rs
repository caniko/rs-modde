use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tracing::{info, warn};

use modde_core::manifest::wabbajack::{
    WabbajackManifest, cache_wabbajack_file, compute_manifest_hash,
};
use modde_core::paths;
use modde_core::profile::{
    EnabledMod, LoadOrderLock, LockReason, Profile, ProfileManager, ProfileSource,
};

use crate::direct::DirectSource;
use crate::nexus::NexusSource;
use crate::wabbajack::installer::{InstallProgress, WabbajackInstaller};

#[derive(Debug, Clone)]
pub struct WabbajackInstallOptions {
    pub path: PathBuf,
    pub profile_name: Option<String>,
    pub game_dir: Option<PathBuf>,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct WabbajackInstallSummary {
    pub profile_name: String,
    pub modlist_name: String,
    pub game_id: String,
    pub mod_count: usize,
    pub manifest_hash: String,
}

pub fn build_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_mins(5))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client")
}

pub fn parse_wabbajack_manifest(path: &Path) -> Result<WabbajackManifest> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to open wabbajack file: {}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read wabbajack archive: {}", path.display()))?;

    let entry_name = if archive.by_name("modlist").is_ok() {
        "modlist"
    } else {
        "modlist.json"
    };
    let mut entry = archive
        .by_name(entry_name)
        .context("wabbajack archive missing modlist entry")?;
    let mut json = String::new();
    entry
        .read_to_string(&mut json)
        .context("failed to read modlist entry")?;
    serde_json::from_str(&json).context("failed to parse modlist JSON")
}

pub async fn install_wabbajack(
    options: WabbajackInstallOptions,
    progress_tx: Option<mpsc::UnboundedSender<InstallProgress>>,
) -> Result<WabbajackInstallSummary> {
    info!(path = %options.path.display(), ?options.profile_name, "installing Wabbajack modlist");

    let manifest = parse_wabbajack_manifest(&options.path)?;
    let modlist_name = manifest.name.clone();
    let game_id = modde_games::normalize_wabbajack_game(&manifest.game)
        .map_or_else(|| manifest.game.to_lowercase(), String::from);
    let profile_name = options
        .profile_name
        .clone()
        .unwrap_or_else(|| modlist_name.clone());

    let store = paths::store_dir();
    let staging = paths::staging_dir().join(&profile_name);
    std::fs::create_dir_all(&store)?;
    std::fs::create_dir_all(&staging)?;

    let manifest_hash = compute_manifest_hash(&manifest);
    let client = build_http_client()?;
    let mut installer = WabbajackInstaller::new(
        manifest.clone(),
        options.path.clone(),
        store.clone(),
        staging.clone(),
    );
    if let Some(game_dir) = options.game_dir.clone() {
        installer.set_game_dir(game_dir);
    }

    match NexusSource::new(client.clone()) {
        Ok(nexus) => installer.add_source(crate::AnySource::Nexus(nexus)),
        Err(e) => warn!("failed to create Nexus source (no API key?): {e:#}"),
    }
    installer.add_source(crate::AnySource::Direct(DirectSource::new(client)));

    let skip_install =
        !options.force && crate::wabbajack::validator::preflight_staging(&manifest, &staging).await;

    if skip_install {
        if let Some(tx) = &progress_tx {
            tx.send(InstallProgress::Complete).ok();
        }
    } else {
        let (fallback_tx, mut fallback_rx) = mpsc::unbounded_channel();
        let tx = progress_tx.unwrap_or(fallback_tx);
        let drain = tokio::spawn(async move { while fallback_rx.recv().await.is_some() {} });
        installer
            .install(tx)
            .await
            .context("wabbajack install pipeline failed")?;
        drain.abort();
    }

    if let Some(ref game_dir) = options.game_dir {
        deploy_mo2_to_game(&staging, game_dir, options.force)
            .await
            .context("failed to deploy mods to game directory")?;
        configure_wine_overrides(&game_id, game_dir, &staging)?;
    }

    let mut enabled_mods = Vec::new();
    for archive in &manifest.archives {
        enabled_mods.push(EnabledMod {
            mod_id: modde_core::scanner::archive_mod_id(archive),
            enabled: true,
            version: None,
            fomod_config: None,
            ..Default::default()
        });
    }

    let pm = ProfileManager::open().context("failed to open profile database")?;
    let profile = Profile {
        id: None,
        name: profile_name.clone(),
        game_id: modde_core::GameId::from(game_id.clone()),
        source: ProfileSource::Wabbajack {
            manifest_hash: manifest_hash.clone(),
        },
        mods: enabled_mods,
        overrides: ProfileManager::default_overrides(&profile_name),
        load_order_rules: smallvec::SmallVec::new(),
        load_order_lock: Some(LoadOrderLock::now(LockReason::Wabbajack {
            manifest_hash: manifest_hash.clone(),
        })),
    };

    save_profile_and_settings(&pm, &profile, options.game_dir.as_deref())?;

    if let Err(e) = cache_wabbajack_file(&options.path, &manifest_hash) {
        warn!("failed to cache wabbajack source file: {e:#}");
    }

    Ok(WabbajackInstallSummary {
        profile_name,
        modlist_name,
        game_id,
        mod_count: profile.mods.len(),
        manifest_hash,
    })
}

fn save_profile_and_settings(
    pm: &ProfileManager,
    profile: &Profile,
    game_dir: Option<&Path>,
) -> Result<()> {
    pm.create_or_update(profile)?;
    let mut settings = modde_core::settings::AppSettings::load();
    if let Some(gd) = game_dir {
        settings.set_game_path(&profile.game_id, gd.to_path_buf());
    }
    settings.selected_game = Some(profile.game_id.to_string());
    settings.save();
    Ok(())
}

pub fn configure_wine_overrides(game_id: &str, game_dir: &Path, staging: &Path) -> Result<()> {
    let Some(plugin) = modde_games::resolve_game_plugin(game_id) else {
        info!(%game_id, "no game plugin found, skipping Wine DLL override detection");
        return Ok(());
    };

    let mut overrides = plugin.wine_dll_overrides(game_dir);
    for dll in plugin.wine_dll_overrides_from_staging(staging) {
        if !overrides.contains(&dll) {
            overrides.push(dll);
        }
    }
    if overrides.is_empty() {
        return Ok(());
    }

    let launcher = modde_games::launcher::detect_launcher(game_dir);
    #[cfg(target_os = "linux")]
    modde_games::launcher::apply_wine_overrides(&launcher, &overrides)?;

    let tool_env_vars = match modde_core::db::ModdeDb::open() {
        Ok(db) => modde_games::launcher::collect_tool_env_vars(game_id, &db).unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    if let Some(wrapper_path) =
        modde_games::launcher::generate_launch_wrapper(game_dir, staging, game_id, &tool_env_vars)?
    {
        modde_games::launcher::register_heroic_wrapper(&launcher, &wrapper_path)?;
    }

    Ok(())
}

pub async fn deploy_mo2_to_game(staging: &Path, game_dir: &Path, force: bool) -> Result<()> {
    let mods_dir = staging.join("mods");
    if !mods_dir.exists() {
        info!("no mods/ directory in staging, skipping deployment");
        return Ok(());
    }

    let mut entries = tokio::fs::read_dir(&mods_dir).await?;
    while let Some(mod_entry) = entries.next_entry().await? {
        if !mod_entry.file_type().await?.is_dir() {
            continue;
        }
        let mod_path = mod_entry.path();
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

                let rel_path = entry_path.strip_prefix(&mod_path).unwrap_or(&entry_path);
                let filename = rel_path.file_name().unwrap_or_default().to_string_lossy();
                if filename == "meta.ini" || filename == "meta.json" {
                    continue;
                }

                let dest = game_dir.join(rel_path);
                #[cfg(unix)]
                if !force
                    && let (Ok(src_meta), Ok(dst_meta)) = (
                        tokio::fs::metadata(&entry_path).await,
                        tokio::fs::metadata(&dest).await,
                    )
                {
                    use std::os::unix::fs::MetadataExt;
                    if src_meta.ino() == dst_meta.ino() && src_meta.dev() == dst_meta.dev() {
                        continue;
                    }
                }

                if let Some(parent) = dest.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                if dest.exists() || dest.symlink_metadata().is_ok() {
                    tokio::fs::remove_file(&dest).await.ok();
                }
                match tokio::fs::hard_link(&entry_path, &dest).await {
                    Ok(()) => {}
                    Err(e) if modde_core::fs::is_cross_device_error(&e) => {
                        tokio::fs::copy(&entry_path, &dest).await.with_context(|| {
                            format!(
                                "cross-filesystem copy fallback failed: {} -> {}",
                                entry_path.display(),
                                dest.display()
                            )
                        })?;
                    }
                    Err(e) => {
                        return Err(e).with_context(|| {
                            format!(
                                "failed to hardlink {} -> {}",
                                entry_path.display(),
                                dest.display()
                            )
                        });
                    }
                }
            }
        }
    }
    Ok(())
}
