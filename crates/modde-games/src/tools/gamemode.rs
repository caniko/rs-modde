//! `GameMode` — Feral Interactive's performance optimization daemon.
//!
//! Uses `gamemoderun` as a wrapper command before the game executable.
//! No config files or env vars needed.

use smallvec::SmallVec;

use super::{GameTool, ToolAvailability, ToolCategory, ToolConfig, WrapperEntry, which};

pub static GAMEMODE: GameMode = GameMode;

pub struct GameMode;

impl GameTool for GameMode {
    fn tool_id(&self) -> &'static str {
        "gamemode"
    }

    fn display_name(&self) -> &'static str {
        "GameMode"
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Performance
    }

    fn description(&self) -> &'static str {
        "Feral GameMode wrapper that applies system performance tuning while the game runs."
    }

    fn settings_schema(&self) -> Vec<super::ToolSettingSpec> {
        vec![super::ToolSettingSpec::read_only(
            "wrapper",
            "Wrapper",
            "Uses gamemoderun before the game executable when enabled.",
        )]
    }

    fn detect_available(&self) -> ToolAvailability {
        #[cfg(not(target_os = "linux"))]
        {
            return ToolAvailability::NotInstalled {
                install_hint: "GameMode is only available on Linux".into(),
            };
        }

        #[cfg(target_os = "linux")]
        if which("gamemoderun").is_some() {
            ToolAvailability::Available { version: None }
        } else {
            ToolAvailability::NotInstalled {
                install_hint: "Install gamemode from your package manager".into(),
            }
        }
    }

    fn env_vars(&self, _config: &ToolConfig) -> SmallVec<[(String, String); 4]> {
        SmallVec::new()
    }

    fn wrapper_command(&self, _config: &ToolConfig) -> Option<WrapperEntry> {
        Some(WrapperEntry {
            exe: "gamemoderun".into(),
            args: String::new(),
        })
    }

    fn default_config(&self) -> ToolConfig {
        ToolConfig::new("gamemode")
    }
}
