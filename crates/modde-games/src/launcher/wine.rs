#![allow(clippy::wildcard_imports)]
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
use super::*;

#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
use std::path::Path;

#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
use anyhow::{Context, Result};
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
use serde_json::Value;
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
use tracing::{info, warn};

/// Format a list of DLL names as a `WINEDLLOVERRIDES` value string.
///
/// Wine DLL overrides are only relevant on Linux (where games run via Wine/Proton).
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
pub(super) fn format_wine_overrides(overrides: &[String]) -> String {
    overrides
        .iter()
        .map(|dll| format!("{dll}=n,b"))
        .collect::<Vec<_>>()
        .join(";")
}

/// Apply Wine DLL overrides to the detected launcher's configuration.
///
/// Returns `true` if the config was updated, `false` if no changes were needed.
///
/// Only relevant on Linux where games run via Wine/Proton.
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
pub fn apply_wine_overrides(
    launcher: &Launcher,
    overrides: &[String],
) -> Result<Option<WineOverrideReport>> {
    if overrides.is_empty() {
        return Ok(None);
    }

    match launcher {
        Launcher::Heroic {
            config_path,
            game_id,
        } => apply_heroic_overrides(config_path, game_id, overrides),
        Launcher::Steam { app_id } => {
            let override_str = format_wine_overrides(overrides);
            warn!(
                "Steam game (app {app_id}): add to launch options:\n  \
                 WINEDLLOVERRIDES=\"{override_str}\" %command%"
            );
            Ok(Some(WineOverrideReport::SteamInstruction {
                override_value: override_str,
            }))
        }
        Launcher::Unknown => {
            let override_str = format_wine_overrides(overrides);
            warn!("Unknown launcher: set WINEDLLOVERRIDES=\"{override_str}\" before launching");
            Ok(Some(WineOverrideReport::UnknownInstruction {
                override_value: override_str,
            }))
        }
    }
}

/// Update Heroic's `GamesConfig` JSON to include WINEDLLOVERRIDES.
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
pub(super) fn apply_heroic_overrides(
    config_path: &Path,
    game_id: &str,
    overrides: &[String],
) -> Result<Option<WineOverrideReport>> {
    let data = std::fs::read_to_string(config_path)
        .with_context(|| format!("failed to read Heroic config: {}", config_path.display()))?;

    let mut config: Value = serde_json::from_str(&data).with_context(|| {
        format!(
            "failed to parse Heroic config JSON: {}",
            config_path.display()
        )
    })?;

    let game_config = config
        .get_mut(game_id)
        .context("game entry not found in Heroic config")?;

    // Build the override string: "version=n,b;winmm=n,b" etc.
    // We don't include dxgi here since fgmod handles it at launch time.
    let new_overrides: Vec<String> = overrides
        .iter()
        .filter(|dll| *dll != "dxgi") // fgmod handles dxgi
        .map(|dll| format!("{dll}=n,b"))
        .collect();

    if new_overrides.is_empty() {
        return Ok(None);
    }

    let override_value = new_overrides.join(";");

    // Get or create enviromentOptions (Heroic uses this spelling)
    let env_options = game_config
        .get_mut("enviromentOptions")
        .context("enviromentOptions not found in game config")?;

    let env_array = env_options
        .as_array_mut()
        .context("enviromentOptions is not an array")?;

    // Check if WINEDLLOVERRIDES is already set
    let existing_idx = env_array
        .iter()
        .position(|entry| entry.get("key").and_then(|k| k.as_str()) == Some("WINEDLLOVERRIDES"));

    if let Some(idx) = existing_idx {
        // Update existing entry — merge with existing overrides
        let existing_value = env_array[idx]
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Parse existing overrides and merge (split on ';' only — commas are part of values like "n,b")
        let mut all_overrides: Vec<String> = existing_value
            .split(';')
            .filter(|s| !s.is_empty())
            .map(std::string::ToString::to_string)
            .collect();

        for new_ov in &new_overrides {
            let dll_name = new_ov.split('=').next().unwrap_or("");
            // Remove any existing entry for this DLL
            all_overrides.retain(|ov| {
                let existing_name = ov.split('=').next().unwrap_or("");
                existing_name != dll_name
            });
            all_overrides.push(new_ov.clone());
        }

        let merged = all_overrides.join(";");
        env_array[idx] = serde_json::json!({
            "key": "WINEDLLOVERRIDES",
            "value": merged
        });

        info!(overrides = %merged, "updated existing WINEDLLOVERRIDES in Heroic config");
    } else {
        // Add new entry
        env_array.push(serde_json::json!({
            "key": "WINEDLLOVERRIDES",
            "value": override_value
        }));

        info!(overrides = %override_value, "added WINEDLLOVERRIDES to Heroic config");
    }

    // Write back
    let output =
        serde_json::to_string_pretty(&config).context("failed to serialize Heroic config")?;
    std::fs::write(config_path, output)
        .with_context(|| format!("failed to write Heroic config: {}", config_path.display()))?;

    Ok(Some(WineOverrideReport::HeroicUpdated {
        value: if existing_idx.is_some() {
            "merged with existing".to_string()
        } else {
            override_value
        },
    }))
}
