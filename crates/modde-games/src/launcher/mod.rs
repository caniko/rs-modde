//! Launcher detection and configuration for Wine/Proton DLL overrides.
//!
//! After deploying mods that include proxy DLLs (e.g. `version.dll` for CET),
//! Wine/Proton needs `WINEDLLOVERRIDES` set so it loads the native (mod) version
//! instead of its built-in stub. This module detects the game launcher and
//! updates its configuration automatically.

use std::path::PathBuf;

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

mod detection;
mod environment;
mod wine;
mod wrapper;

pub use detection::detect_launcher;
pub use environment::{
    ToolEnvironmentReport, apply_tool_environment_heroic, collect_tool_dll_overrides,
    collect_tool_env_vars, collect_tool_wrappers, generate_tool_configs,
};
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
pub use wine::apply_wine_overrides;
pub use wrapper::{generate_launch_wrapper, register_heroic_wrapper};
