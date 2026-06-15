use std::path::PathBuf;

use crate::detection::{DetectedGame, LauncherSource};

/// Game metadata available to per-game tools.
#[derive(Debug, Clone)]
pub struct ToolGameContext {
    pub game_id: String,
    pub display_name: String,
    pub install_path: Option<PathBuf>,
    pub launcher_source: Option<LauncherSource>,
    pub steam_app_id: Option<String>,
    pub executable_dir: Option<PathBuf>,
}

impl ToolGameContext {
    #[must_use]
    pub fn from_parts(
        game_id: &str,
        display_name: impl Into<String>,
        install_path: Option<PathBuf>,
        detected: Option<&DetectedGame>,
    ) -> Self {
        let plugin = crate::resolve_game_plugin(game_id);
        let executable_dir = install_path
            .as_deref()
            .and_then(|path| plugin.map(|plugin| plugin.executable_dir(path)));
        let launcher_source = detected.map(|game| game.source.clone());
        let steam_app_id = launcher_source.as_ref().and_then(|source| match source {
            LauncherSource::Steam { app_id, .. } => Some(app_id.clone()),
            LauncherSource::HeroicGog { .. }
            | LauncherSource::HeroicEpic { .. }
            | LauncherSource::HeroicSideload { .. } => None,
        });

        Self {
            game_id: game_id.to_string(),
            display_name: display_name.into(),
            install_path,
            launcher_source,
            steam_app_id,
            executable_dir,
        }
    }

    #[must_use]
    pub fn launcher_label(&self) -> String {
        self.launcher_source
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "Not detected".to_string())
    }
}
