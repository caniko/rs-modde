#![allow(clippy::wildcard_imports)]
use super::*;

/// Scan Heroic's installed game databases (GOG, Epic/Legendary, Sideload).
pub(super) fn scan_heroic_stores(detected: &mut Vec<DetectedGame>) {
    let Some(heroic_dir) = paths::heroic_config_dir() else {
        return;
    };

    // GOG store
    scan_heroic_store_file(
        &heroic_dir.join("gog_store/installed.json"),
        |app_id| {
            launcher_games()
                .find(|g| g.launcher.heroic_gog_app_id == Some(app_id))
                .map(|g| (g, HeroicStoreKind::Gog))
        },
        detected,
    );

    // Epic/Legendary store
    scan_heroic_store_file(
        &heroic_dir.join("legendary_store/installed.json"),
        |app_id| {
            launcher_games()
                .find(|g| g.launcher.heroic_epic_app_id == Some(app_id))
                .map(|g| (g, HeroicStoreKind::Epic))
        },
        detected,
    );

    // Sideloaded apps — match by directory name heuristic
    scan_heroic_sideload(&heroic_dir.join("sideload_apps/installed.json"), detected);
}

#[derive(Clone, Copy)]
pub(super) enum HeroicStoreKind {
    Gog,
    Epic,
}

/// Parse a Heroic `installed.json` and match entries against known games.
pub(super) fn scan_heroic_store_file(
    path: &Path,
    matcher: impl Fn(&str) -> Option<(&'static GameRegistration, HeroicStoreKind)>,
    detected: &mut Vec<DetectedGame>,
) {
    let data = match std::fs::read_to_string(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return;
        }
        Err(e) => {
            debug!(error = %e, path = %path.display(), "failed to read Heroic store file");
            return;
        }
    };

    let parsed: Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, path = %path.display(), "failed to parse Heroic store JSON");
            return;
        }
    };

    let Some(installed) = parsed.get("installed").and_then(|v| v.as_array()) else {
        debug!(path = %path.display(), "Heroic store file missing 'installed' array");
        return;
    };

    for entry in installed {
        let Some(app_name) = entry.get("appName").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(install_path) = entry.get("install_path").and_then(|v| v.as_str()) else {
            continue;
        };

        let install_path = PathBuf::from(install_path);
        if !install_path.is_dir() {
            continue;
        }

        if let Some((game, kind)) = matcher(app_name) {
            debug!(
                game_id = game.game_id,
                app_name,
                path = %install_path.display(),
                "detected Heroic game"
            );
            let source = match kind {
                HeroicStoreKind::Gog => LauncherSource::HeroicGog {
                    app_id: game
                        .launcher
                        .heroic_gog_app_id
                        .unwrap_or(app_name)
                        .to_string(),
                },
                HeroicStoreKind::Epic => LauncherSource::HeroicEpic {
                    app_id: game
                        .launcher
                        .heroic_epic_app_id
                        .unwrap_or(app_name)
                        .to_string(),
                },
            };
            detected.push(DetectedGame {
                game_id: game.game_id,
                display_name: game.display_name,
                install_path,
                source,
            });
        }
    }
}

/// Scan Heroic sideloaded apps — match by directory name against known `steam_dir` names.
pub(super) fn scan_heroic_sideload(path: &Path, detected: &mut Vec<DetectedGame>) {
    let data = match std::fs::read_to_string(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return;
        }
        Err(e) => {
            debug!(error = %e, path = %path.display(), "failed to read Heroic sideload file");
            return;
        }
    };

    let parsed: Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, path = %path.display(), "failed to parse Heroic sideload JSON");
            return;
        }
    };

    let Some(installed) = parsed.get("installed").and_then(|v| v.as_array()) else {
        debug!(path = %path.display(), "Heroic sideload file missing 'installed' array");
        return;
    };

    for entry in installed {
        let Some(app_name) = entry.get("appName").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(install_path_str) = entry.get("install_path").and_then(|v| v.as_str()) else {
            continue;
        };

        let install_path = PathBuf::from(install_path_str);
        if !install_path.is_dir() {
            continue;
        }

        // Try to match by directory name
        let dir_name = install_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        for game in launcher_games() {
            let matches = game
                .launcher
                .steam_dir
                .is_some_and(|sd| sd.eq_ignore_ascii_case(dir_name));

            if matches {
                debug!(
                    game_id = game.game_id,
                    app_name,
                    path = %install_path.display(),
                    "detected Heroic sideloaded game"
                );
                detected.push(DetectedGame {
                    game_id: game.game_id,
                    display_name: game.display_name,
                    install_path,
                    source: LauncherSource::HeroicSideload {
                        app_id: app_name.to_string(),
                    },
                });
                break;
            }
        }
    }
}
