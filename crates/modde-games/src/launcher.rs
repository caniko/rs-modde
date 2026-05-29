//! Launcher detection and configuration for Wine/Proton DLL overrides.
//!
//! After deploying mods that include proxy DLLs (e.g. `version.dll` for CET),
//! Wine/Proton needs `WINEDLLOVERRIDES` set so it loads the native (mod) version
//! instead of its built-in stub. This module detects the game launcher and
//! updates its configuration automatically.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use modde_core::resolver::GameId;
use serde_json::Value;
use tracing::{debug, info, warn};

/// Detected game launcher type.
#[derive(Debug)]
pub enum Launcher {
    /// Heroic Games Launcher — config at `~/.config/heroic/GamesConfig/<id>.json`
    Heroic {
        config_path: PathBuf,
        game_id: String,
    },
    /// Steam — uses launch options in Steam client
    Steam { app_id: String },
    /// Unknown launcher — print instructions for manual setup
    Unknown,
}

/// Structured result of launcher configuration work.
#[derive(Debug, Clone, Default)]
pub struct LauncherConfigurationReport {
    pub wine_overrides: Option<WineOverrideReport>,
    pub launch_wrapper: Option<LaunchWrapperReport>,
    pub wrapper_registration: Option<WrapperRegistrationReport>,
}

impl LauncherConfigurationReport {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.wine_overrides.is_none()
            && self.launch_wrapper.is_none()
            && self.wrapper_registration.is_none()
    }
}

/// Result of applying or instructing Wine DLL override configuration.
#[derive(Debug, Clone)]
pub enum WineOverrideReport {
    HeroicUpdated { value: String },
    SteamInstruction { override_value: String },
    UnknownInstruction { override_value: String },
}

/// Result of generating the modde launch wrapper.
#[derive(Debug, Clone)]
pub struct LaunchWrapperReport {
    pub path: PathBuf,
    pub restore_count: usize,
    pub tool_env_var_count: usize,
}

/// Result of registering, or instructing the user to register, a wrapper.
#[derive(Debug, Clone)]
pub enum WrapperRegistrationReport {
    HeroicRegistered,
    ManualInstruction { wrapper_path: PathBuf },
}

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
fn detect_heroic(game_dir: &Path) -> Option<Launcher> {
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
fn heroic_game_matches(config_dir: &Path, game_id: &str, game_dir: &Path) -> bool {
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
fn detect_steam(game_dir: &Path) -> Option<String> {
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

/// Generate a Unix (Linux/macOS) bash wrapper script.
#[cfg(unix)]
fn generate_wrapper_unix(
    wrapper_dir: &Path,
    restore_commands: &[(String, String)],
    tool_env_vars: &[(String, String)],
    game_id: &GameId,
    modde_bin: &str,
) -> (PathBuf, String) {
    let wrapper_path = wrapper_dir.join("modde-launch-wrapper.sh");

    let mut script = String::from("#!/usr/bin/env bash\n");
    script.push_str("# Auto-generated by modde — tool env vars + fgmod DLL restoration\n");
    script.push_str("# Do not edit manually; re-run `modde deploy` to regenerate.\n\n");

    if !tool_env_vars.is_empty() {
        script.push_str("# Tool environment variables\n");
        for (key, value) in tool_env_vars {
            script.push_str(&format!("export {key}=\"{value}\"\n"));
        }
        script.push('\n');
    }

    if !restore_commands.is_empty() {
        script.push_str("# Restore mod DLLs that fgmod deletes\n");
        for (src, dest) in restore_commands {
            script.push_str(&format!(
                "cp -f \"{src}\" \"{dest}\" 2>/dev/null && echo \"modde: restored $(basename \"{dest}\")\"\n"
            ));
        }
        script.push('\n');
    }

    script.push_str("\"$@\"\n");
    script.push_str("status=$?\n\n");
    script.push_str(&format!(
        "# Auto-capture saves after game exit\n\
         \"{modde_bin}\" save auto-capture --game {game_id} 2>/dev/null &\n\n\
         exit $status\n"
    ));

    (wrapper_path, script)
}

/// Generate a Windows `.cmd` wrapper script.
#[cfg(windows)]
fn generate_wrapper_windows(
    wrapper_dir: &Path,
    restore_commands: &[(String, String)],
    tool_env_vars: &[(String, String)],
    game_id: &GameId,
    modde_bin: &str,
) -> (PathBuf, String) {
    let wrapper_path = wrapper_dir.join("modde-launch-wrapper.cmd");

    let mut script = String::from("@echo off\r\n");
    script.push_str("REM Auto-generated by modde — tool env vars + fgmod DLL restoration\r\n");
    script.push_str("REM Do not edit manually; re-run `modde deploy` to regenerate.\r\n\r\n");

    if !tool_env_vars.is_empty() {
        script.push_str("REM Tool environment variables\r\n");
        for (key, value) in tool_env_vars {
            script.push_str(&format!("set \"{key}={value}\"\r\n"));
        }
        script.push_str("\r\n");
    }

    if !restore_commands.is_empty() {
        script.push_str("REM Restore mod DLLs that fgmod deletes\r\n");
        for (src, dest) in restore_commands {
            script.push_str(&format!(
                "copy /Y \"{src}\" \"{dest}\" >nul 2>nul && echo modde: restored {dest}\r\n"
            ));
        }
        script.push_str("\r\n");
    }

    script.push_str("%*\r\n");
    script.push_str("set status=%ERRORLEVEL%\r\n\r\n");
    script.push_str(&format!(
        "REM Auto-capture saves after game exit\r\n\
         start \"\" /B \"{modde_bin}\" save auto-capture --game {game_id} 2>nul\r\n\r\n\
         exit /b %status%\r\n"
    ));

    (wrapper_path, script)
}

/// Generate a modde launch wrapper script that restores mod DLLs deleted by fgmod
/// and exports tool environment variables.
///
/// fgmod cleans up certain DLLs (winmm.dll, dxgi.dll, etc.) before installing `OptiScaler`.
/// If mods deploy those same DLLs (e.g. `RED4ext` uses winmm.dll), we need to restore them
/// after fgmod runs but before the game starts.
///
/// Additionally, the wrapper exports env vars for any enabled tools (`MangoHud`, vkBasalt, etc.)
/// so they take effect even when launched via Steam (where we can't modify the launcher config).
pub fn generate_launch_wrapper(
    game_dir: &Path,
    staging_dir: &Path,
    game_id: &GameId,
    tool_env_vars: &[(String, String)],
) -> Result<Option<LaunchWrapperReport>> {
    // Delegate fgmod restore scanning to the optiscaler module, using the
    // selected game's metadata to derive the executable directory.
    let executable_dir = crate::resolve_game_plugin(game_id.as_str())
        .map(|plugin| plugin.executable_dir(game_dir))
        .unwrap_or_else(|| game_dir.to_path_buf());
    let restore_commands = crate::tools::optiscaler::fgmod_restore_commands_for_executable_dir(
        game_dir,
        staging_dir,
        &executable_dir,
    );

    if restore_commands.is_empty() && tool_env_vars.is_empty() {
        return Ok(None);
    }

    // Generate the wrapper script
    let wrapper_dir = modde_core::paths::modde_data_dir().join("bin");
    std::fs::create_dir_all(&wrapper_dir).context("failed to create modde bin directory")?;

    let modde_bin = std::env::current_exe()
        .map_or_else(|_| "modde".to_string(), |p| p.to_string_lossy().to_string());

    #[cfg(unix)]
    let (wrapper_path, script) = generate_wrapper_unix(
        &wrapper_dir,
        &restore_commands,
        tool_env_vars,
        game_id,
        &modde_bin,
    );

    #[cfg(windows)]
    let (wrapper_path, script) = generate_wrapper_windows(
        &wrapper_dir,
        &restore_commands,
        tool_env_vars,
        game_id,
        &modde_bin,
    );

    std::fs::write(&wrapper_path, &script)
        .with_context(|| format!("failed to write launch wrapper: {}", wrapper_path.display()))?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper_path, std::fs::Permissions::from_mode(0o755))
            .context("failed to set wrapper script permissions")?;
    }

    info!(
        wrapper = %wrapper_path.display(),
        restores = restore_commands.len(),
        tool_vars = tool_env_vars.len(),
        "generated modde launch wrapper"
    );

    Ok(Some(LaunchWrapperReport {
        path: wrapper_path,
        restore_count: restore_commands.len(),
        tool_env_var_count: tool_env_vars.len(),
    }))
}

/// Format a list of DLL names as a `WINEDLLOVERRIDES` value string.
///
/// Wine DLL overrides are only relevant on Linux (where games run via Wine/Proton).
#[cfg(target_os = "linux")]
fn format_wine_overrides(overrides: &[String]) -> String {
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
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
fn apply_heroic_overrides(
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

/// Register the modde launch wrapper in Heroic's wrapper chain.
///
/// The wrapper is inserted **after** fgmod (if present) so it can restore DLLs
/// that fgmod deletes before the game launches.
pub fn register_heroic_wrapper(
    launcher: &Launcher,
    wrapper_path: &Path,
) -> Result<Option<WrapperRegistrationReport>> {
    let Launcher::Heroic {
        config_path,
        game_id,
    } = launcher
    else {
        return Ok(Some(WrapperRegistrationReport::ManualInstruction {
            wrapper_path: wrapper_path.to_path_buf(),
        }));
    };

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

    let wrapper_options = game_config
        .get_mut("wrapperOptions")
        .context("wrapperOptions not found in game config")?;

    let wrappers = wrapper_options
        .as_array_mut()
        .context("wrapperOptions is not an array")?;

    let wrapper_exe = wrapper_path.to_string_lossy().to_string();

    // Check if modde wrapper is already registered
    let already_registered = wrappers
        .iter()
        .any(|w| w.get("exe").and_then(|e| e.as_str()) == Some(&wrapper_exe));

    if already_registered {
        info!("modde launch wrapper already registered in Heroic config");
        return Ok(None);
    }

    // Insert the modde wrapper. It should go after fgmod (which modifies files)
    // but before gamemoderun/umu-run. In Heroic, wrappers are chained in order,
    // so we insert at position 1 (after fgmod at position 0) or at the end.
    let fgmod_idx = wrappers.iter().position(|w| {
        w.get("exe")
            .and_then(|e| e.as_str())
            .is_some_and(|e| e.contains("fgmod"))
    });

    let insert_idx = match fgmod_idx {
        Some(idx) => idx + 1,   // After fgmod
        None => wrappers.len(), // At the end
    };

    let wrapper_entry = serde_json::json!({
        "exe": wrapper_exe,
        "args": "--"
    });

    wrappers.insert(insert_idx, wrapper_entry);

    // Write back
    let output =
        serde_json::to_string_pretty(&config).context("failed to serialize Heroic config")?;
    std::fs::write(config_path, output)
        .with_context(|| format!("failed to write Heroic config: {}", config_path.display()))?;

    info!(wrapper = %wrapper_exe, "registered modde wrapper in Heroic config");

    Ok(Some(WrapperRegistrationReport::HeroicRegistered))
}

// ── Tool environment integration ────────────────────────────────────────

/// Collect all environment variables from enabled tools for a game.
///
/// Reads tool configs from the database and calls each tool's `env_vars()`.
/// Returns a flat list of `(KEY, VALUE)` pairs.
pub fn collect_tool_env_vars(
    game_id: &GameId,
    db: &modde_core::db::ModdeDb,
) -> Result<Vec<(String, String)>> {
    let rows = db.load_tool_configs(game_id)?;
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
pub fn collect_tool_dll_overrides(
    game_id: &GameId,
    db: &modde_core::db::ModdeDb,
) -> Result<Vec<String>> {
    let rows = db.load_tool_configs(game_id)?;
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
pub fn collect_tool_wrappers(
    game_id: &GameId,
    db: &modde_core::db::ModdeDb,
) -> Result<Vec<crate::tools::WrapperEntry>> {
    let rows = db.load_tool_configs(game_id)?;
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
pub fn generate_tool_configs(game_id: &GameId, db: &modde_core::db::ModdeDb) -> Result<()> {
    let rows = db.load_tool_configs(game_id)?;

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
