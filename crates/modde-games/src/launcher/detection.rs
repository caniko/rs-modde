#![allow(clippy::wildcard_imports)]
use super::*;

use std::path::{Path, PathBuf};

use serde_json::Value;
use tracing::{debug, warn};

/// Detect which launcher manages a game at the given install path.
#[must_use]
pub fn detect_launcher(game_dir: &Path) -> Launcher {
    // Check for Heroic: game paths typically contain "heroic" or match a Heroic library
    if let Some(launcher) = detect_heroic(game_dir) {
        return launcher;
    }

    // Check for Steam: game is under steamapps/common/
    if let Some(app_id) = detect_steam(game_dir) {
        return Launcher::Steam { app_id };
    }

    Launcher::Unknown
}

/// Try to detect Heroic launcher by scanning its `GamesConfig` directory.
pub(super) fn detect_heroic(game_dir: &Path) -> Option<Launcher> {
    let config_dir = modde_core::paths::heroic_config_dir()?;
    let games_config = config_dir.join("GamesConfig");

    if !games_config.is_dir() {
        return None;
    }

    // Read each game config and check if its install path matches
    let entries = std::fs::read_dir(&games_config).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        // The filename is the game ID (e.g., "1423049311.json")
        let game_id = path.file_stem()?.to_string_lossy().to_string();

        // Also check the Heroic library for the install path
        if heroic_game_matches(&config_dir, &game_id, game_dir) {
            return Some(Launcher::Heroic {
                config_path: path,
                game_id,
            });
        }
    }

    None
}

/// Check if a Heroic game entry matches the given game directory.
pub(super) fn heroic_game_matches(config_dir: &Path, game_id: &str, game_dir: &Path) -> bool {
    // Check installed.json files for each store (GOG, Epic/Legendary, etc.)
    let installed_files = [
        config_dir.join("gog_store/installed.json"),
        config_dir.join("legendary_store/installed.json"),
        config_dir.join("sideload_apps/installed.json"),
    ];

    for installed_path in &installed_files {
        let data = match std::fs::read_to_string(installed_path) {
            Ok(d) => d,
            Err(e) => {
                debug!(error = %e, path = %installed_path.display(), "failed to read Heroic installed file");
                continue;
            }
        };
        let val: Value = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, path = %installed_path.display(), "failed to parse Heroic installed JSON");
                continue;
            }
        };
        if let Some(games) = val.get("installed").and_then(|v| v.as_array()) {
            for game in games {
                if game.get("appName").and_then(|v| v.as_str()) == Some(game_id)
                    && let Some(install_path) = game.get("install_path").and_then(|v| v.as_str())
                {
                    let canonical_game = game_dir
                        .canonicalize()
                        .unwrap_or_else(|_| game_dir.to_path_buf());
                    let canonical_install = PathBuf::from(install_path)
                        .canonicalize()
                        .unwrap_or_else(|_| PathBuf::from(install_path));
                    return canonical_game == canonical_install;
                }
            }
        }
    }

    false
}

/// Try to detect Steam by checking if the game is under steamapps/common/.
pub(super) fn detect_steam(game_dir: &Path) -> Option<String> {
    let path_str = game_dir.to_string_lossy().replace('\\', "/");
    if path_str.contains("steamapps/common/") {
        // Try to find the appmanifest to get the app ID
        if let Some(steamapps) = game_dir
            .ancestors()
            .find(|p| p.file_name().and_then(|f| f.to_str()) == Some("common"))
            .and_then(|p| p.parent())
        {
            let game_name = game_dir.file_name()?.to_string_lossy();
            let manifests = std::fs::read_dir(steamapps).ok()?;
            for entry in manifests.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with("appmanifest_")
                    && name_str.ends_with(".acf")
                    && let Ok(content) = std::fs::read_to_string(entry.path())
                    && content.contains(&*game_name)
                {
                    let app_id = name_str
                        .strip_prefix("appmanifest_")?
                        .strip_suffix(".acf")?
                        .to_string();
                    return Some(app_id);
                }
            }
        }
    }
    None
}
