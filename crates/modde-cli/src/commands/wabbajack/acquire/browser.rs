//! Chromium browser-controller acquisition flow.

use super::*;

pub(super) async fn acquire_missing_with_browser_controller(
    manifest: &WabbajackManifest,
    store_dir: &Path,
    download_dir: &Path,
    data_dir: &Path,
    browser_profile: Option<&Path>,
    missing: Vec<AcquireMissingArchive>,
    timeout: Duration,
    json: bool,
) -> Result<Vec<AcquireResult>> {
    let mut results = Vec::with_capacity(missing.len());
    let mut pending = Vec::new();

    for archive in missing {
        match archive.source_kind {
            MissingArchiveSourceKind::Manual => {
                match try_acquire_manual_direct(manifest, store_dir, download_dir, &archive).await?
                {
                    DirectAcquireOutcome::Resolved(result)
                    | DirectAcquireOutcome::Final(result) => {
                        if !json {
                            print_acquire_result(&result);
                        }
                        results.push(result);
                    }
                    DirectAcquireOutcome::NeedsBrowser { archive, message } => {
                        if !json {
                            eprintln!(
                                "browser-required {:016x} {} message={}",
                                archive.hash, archive.name, message
                            );
                        }
                        pending.push(archive);
                    }
                    DirectAcquireOutcome::Unsupported => pending.push(archive),
                }
            }
            MissingArchiveSourceKind::Nexus => {
                let result =
                    acquire_nexus_archive(manifest, store_dir, download_dir, &archive, json)
                        .await?;
                if matches!(
                    result.status,
                    AcquireStatus::Imported | AcquireStatus::AlreadyPresent
                ) {
                    if !json {
                        print_acquire_result(&result);
                    }
                    results.push(result);
                } else if archive.url.is_some() {
                    if !json {
                        eprintln!(
                            "nexus-browser-fallback {:016x} {}: {}",
                            archive.hash,
                            archive.name,
                            result
                                .message
                                .as_deref()
                                .unwrap_or("Nexus API did not resolve")
                        );
                    }
                    pending.push(archive);
                } else {
                    if !json {
                        print_acquire_result(&result);
                    }
                    results.push(result);
                }
            }
        }
    }

    if pending.is_empty() {
        return Ok(results);
    }

    if !json {
        eprintln!("pending browser downloads:");
        for archive in &pending {
            eprintln!(
                "  {:016x} {} {}",
                archive.hash,
                archive.name,
                archive.url.as_deref().unwrap_or("<no url>")
            );
        }
    }

    let urls = pending
        .iter()
        .filter_map(|archive| archive.url.clone())
        .collect::<Vec<_>>();
    let default_browser_profile = data_dir.join("browser-profiles/wabbajack-acquire");
    let browser_profile = browser_profile.unwrap_or(&default_browser_profile);
    let mut browser = match launch_chromium_controller(&urls, download_dir, browser_profile) {
        Ok(child) => Some(child),
        Err(err) => {
            eprintln!("browser-controller-unavailable: {err:#}");
            eprintln!(
                "open the listed URLs manually, then save archives into the watched directory"
            );
            None
        }
    };

    let deadline = std::time::Instant::now() + timeout;
    while !pending.is_empty() {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        match wait_for_next_matching_download(download_dir, &pending, remaining).await {
            Ok(found) if found.matched => {
                let Some(hash) = found.matched_hash else {
                    continue;
                };
                let Some(pos) = pending.iter().position(|archive| archive.hash == hash) else {
                    continue;
                };
                let archive = pending.remove(pos);
                let result =
                    import_acquired_archive(manifest, store_dir, &archive, &found.path).await?;
                if !json {
                    print_acquire_result(&result);
                }
                results.push(result);
            }
            Ok(found) => {
                if !json {
                    eprintln!(
                        "mismatched-download path={} computed={:016x}",
                        found.path.display(),
                        found.computed_xxh64
                    );
                }
            }
            Err(err) => {
                if !json {
                    eprintln!("browser-download-wait-ended: {err:#}");
                }
                break;
            }
        }
    }

    if let Some(browser) = &mut browser {
        let _ = browser.kill();
    }
    for archive in pending {
        let result = AcquireResult {
            archive,
            status: AcquireStatus::TimedOut,
            path: None,
            computed_xxh64: None,
            message: Some(
                "browser-controlled acquisition did not observe a matching download".into(),
            ),
        };
        if !json {
            print_acquire_result(&result);
        }
        results.push(result);
    }

    Ok(results)
}

fn launch_chromium_controller(
    urls: &[String],
    download_dir: &Path,
    browser_profile: &Path,
) -> Result<std::process::Child> {
    if urls.is_empty() {
        anyhow::bail!("no URLs to open");
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        anyhow::bail!("no graphical display found ($DISPLAY or $WAYLAND_DISPLAY is required)");
    }
    std::fs::create_dir_all(download_dir)?;
    write_chromium_preferences(browser_profile, download_dir)?;
    let chromium = find_chromium().context("no Chromium-compatible browser found in PATH")?;
    let mut command = std::process::Command::new(chromium);
    command
        .arg(format!("--user-data-dir={}", browser_profile.display()))
        .arg("--no-first-run")
        .arg("--new-window");
    for url in urls {
        command.arg(url);
    }
    command
        .spawn()
        .context("failed to launch Chromium browser controller")
}

fn write_chromium_preferences(browser_profile: &Path, download_dir: &Path) -> Result<()> {
    let default_dir = browser_profile.join("Default");
    std::fs::create_dir_all(&default_dir)?;
    let prefs = serde_json::json!({
        "download": {
            "default_directory": download_dir,
            "directory_upgrade": true,
            "prompt_for_download": false
        },
        "profile": {
            "default_content_setting_values": {
                "automatic_downloads": 1
            }
        }
    });
    std::fs::write(
        default_dir.join("Preferences"),
        serde_json::to_vec_pretty(&prefs)?,
    )?;
    Ok(())
}

fn find_chromium() -> Option<String> {
    if let Ok(path) = std::env::var("MODDE_CHROMIUM")
        && std::process::Command::new(&path)
            .arg("--version")
            .output()
            .is_ok()
    {
        return Some(path);
    }
    [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
        "brave-browser",
        "brave",
    ]
    .into_iter()
    .find(|candidate| {
        std::process::Command::new(candidate)
            .arg("--version")
            .output()
            .is_ok()
    })
    .map(str::to_string)
}
