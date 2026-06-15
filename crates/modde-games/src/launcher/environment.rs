use std::path::Path;

use anyhow::{Context, Result};
use modde_core::resolver::GameId;
use serde_json::Value;
use tracing::info;

// ── Tool environment integration ────────────────────────────────────────

/// Collect all environment variables from enabled tools for a game.
///
/// Reads tool configs from the database and calls each tool's `env_vars()`.
/// Returns a flat list of `(KEY, VALUE)` pairs.
pub async fn collect_tool_env_vars(
    game_id: &GameId,
    db: &modde_core::db::ModdeDb,
) -> Result<Vec<(String, String)>> {
    let rows = db.load_tool_configs(game_id).await?;
    let mut all_vars = Vec::new();

    for row in &rows {
        if !row.enabled {
            continue;
        }

        let Some(tool) = crate::tools::resolve_tool(&row.tool_id) else {
            continue;
        };

        let mut config = crate::tools::ToolConfig {
            tool_id: row.tool_id.clone(),
            enabled: true,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        };
        // Inject game_id so tools can build per-game config paths
        config.set("_game_id", serde_json::json!(game_id.as_str()));

        all_vars.extend(tool.env_vars(&config));
    }

    Ok(all_vars)
}

/// Collect all Wine DLL overrides from enabled tools for a game.
pub async fn collect_tool_dll_overrides(
    game_id: &GameId,
    db: &modde_core::db::ModdeDb,
) -> Result<Vec<String>> {
    let rows = db.load_tool_configs(game_id).await?;
    let mut overrides = Vec::new();

    for row in &rows {
        if !row.enabled {
            continue;
        }

        let Some(tool) = crate::tools::resolve_tool(&row.tool_id) else {
            continue;
        };

        let config = crate::tools::ToolConfig {
            tool_id: row.tool_id.clone(),
            enabled: true,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        };

        overrides.extend(tool.wine_dll_overrides(&config));
    }

    Ok(overrides)
}

/// Collect wrapper commands from enabled tools.
pub async fn collect_tool_wrappers(
    game_id: &GameId,
    db: &modde_core::db::ModdeDb,
) -> Result<Vec<crate::tools::WrapperEntry>> {
    let rows = db.load_tool_configs(game_id).await?;
    let mut wrappers = Vec::new();

    for row in &rows {
        if !row.enabled {
            continue;
        }

        let Some(tool) = crate::tools::resolve_tool(&row.tool_id) else {
            continue;
        };

        let config = crate::tools::ToolConfig {
            tool_id: row.tool_id.clone(),
            enabled: true,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        };

        if let Some(wrapper) = tool.wrapper_command(&config) {
            wrappers.push(wrapper);
        }
    }

    Ok(wrappers)
}

/// Generate per-game config files for all enabled tools.
///
/// Writes configs to `~/.local/share/modde/tools/{game_id}/`.
pub async fn generate_tool_configs(game_id: &GameId, db: &modde_core::db::ModdeDb) -> Result<()> {
    let rows = db.load_tool_configs(game_id).await?;

    for row in &rows {
        if !row.enabled {
            continue;
        }

        let Some(tool) = crate::tools::resolve_tool(&row.tool_id) else {
            continue;
        };

        let mut config = crate::tools::ToolConfig {
            tool_id: row.tool_id.clone(),
            enabled: true,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        };
        config.set("_game_id", serde_json::json!(game_id.as_str()));

        if let Some(generated) = tool.generate_config(&config) {
            if let Some(parent) = generated.path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            std::fs::write(&generated.path, &generated.content).with_context(|| {
                format!("failed to write tool config: {}", generated.path.display())
            })?;
            info!(tool = tool.tool_id(), path = %generated.path.display(), "wrote tool config");
        }
    }

    Ok(())
}

/// Apply tool environment to a Heroic launcher config.
///
/// Merges env vars from tools into Heroic's `enviromentOptions` and
/// registers wrapper commands in `wrapperOptions`.
pub fn apply_tool_environment_heroic(
    config_path: &Path,
    game_id_heroic: &str,
    env_vars: &[(String, String)],
    wrappers: &[crate::tools::WrapperEntry],
) -> Result<ToolEnvironmentReport> {
    if env_vars.is_empty() && wrappers.is_empty() {
        return Ok(ToolEnvironmentReport::default());
    }

    let data = std::fs::read_to_string(config_path)
        .with_context(|| format!("failed to read Heroic config: {}", config_path.display()))?;

    let mut config: Value = serde_json::from_str(&data).with_context(|| {
        format!(
            "failed to parse Heroic config JSON: {}",
            config_path.display()
        )
    })?;

    let game_config = config
        .get_mut(game_id_heroic)
        .context("game entry not found in Heroic config")?;

    // Merge env vars
    if !env_vars.is_empty() {
        let env_options = game_config
            .get_mut("enviromentOptions")
            .context("enviromentOptions not found in game config")?;
        let env_array = env_options
            .as_array_mut()
            .context("enviromentOptions is not an array")?;

        for (key, value) in env_vars {
            // Remove existing entry for this key
            env_array.retain(|entry| entry.get("key").and_then(|k| k.as_str()) != Some(key));
            env_array.push(serde_json::json!({ "key": key, "value": value }));
        }
    }

    // Register wrapper commands
    if !wrappers.is_empty() {
        let wrapper_options = game_config
            .get_mut("wrapperOptions")
            .context("wrapperOptions not found in game config")?;
        let wrapper_array = wrapper_options
            .as_array_mut()
            .context("wrapperOptions is not an array")?;

        for wrapper in wrappers {
            let already = wrapper_array
                .iter()
                .any(|w| w.get("exe").and_then(|e| e.as_str()) == Some(&wrapper.exe));
            if !already {
                wrapper_array.push(serde_json::json!({
                    "exe": wrapper.exe,
                    "args": wrapper.args,
                }));
            }
        }
    }

    let output =
        serde_json::to_string_pretty(&config).context("failed to serialize Heroic config")?;
    std::fs::write(config_path, output)
        .with_context(|| format!("failed to write Heroic config: {}", config_path.display()))?;

    Ok(ToolEnvironmentReport {
        env_var_count: env_vars.len(),
        wrapper_count: wrappers.len(),
    })
}

/// Result of applying tool environment settings to a launcher config.
#[derive(Debug, Clone, Copy, Default)]
pub struct ToolEnvironmentReport {
    pub env_var_count: usize,
    pub wrapper_count: usize,
}
