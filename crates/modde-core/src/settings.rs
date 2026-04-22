use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::resolver::GameId;

/// Persistent application settings shared between CLI and UI.
///
/// Stored as TOML at `<config_dir>/modde/settings.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub nexus_api_key: String,
    /// Configured game install paths — typically 1–4 games.
    /// `SmallVec<[_; 4]>` keeps ≤4 entries inline (no heap allocation).
    #[serde(default)]
    pub game_paths: SmallVec<[GamePath; 4]>,
    #[serde(default)]
    pub download_dir: Option<PathBuf>,
    #[serde(default)]
    pub theme: String,
    #[serde(default)]
    pub selected_game: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamePath {
    pub game_id: GameId,
    pub path: PathBuf,
}

impl AppSettings {
    fn config_path() -> PathBuf {
        crate::paths::modde_config_dir().join("settings.toml")
    }

    #[must_use]
    pub fn load() -> Self {
        let path = Self::config_path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!(error = %e, "failed to create config directory");
            }
        if let Ok(s) = toml::to_string_pretty(self)
            && let Err(e) = std::fs::write(&path, s) {
                tracing::warn!(error = %e, "failed to write settings file");
            }
    }

    /// Get the install path for a game, if configured.
    #[must_use]
    pub fn game_path(&self, game_id: &str) -> Option<&PathBuf> {
        self.game_paths
            .iter()
            .find(|gp| gp.game_id == game_id)
            .map(|gp| &gp.path)
    }

    /// Set the install path for a game (add or update).
    pub fn set_game_path(&mut self, game_id: &str, path: PathBuf) {
        if let Some(entry) = self.game_paths.iter_mut().find(|gp| gp.game_id == game_id) {
            entry.path = path;
        } else {
            self.game_paths.push(GamePath {
                game_id: GameId::from(game_id),
                path,
            });
        }
    }

    /// Load settings from a specific file path.
    #[must_use]
    pub fn load_from(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Save settings to a specific file path.
    pub fn save_to(&self, path: &std::path::Path) {
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!(error = %e, "failed to create config directory");
            }
        if let Ok(s) = toml::to_string_pretty(self)
            && let Err(e) = std::fs::write(path, s) {
                tracing::warn!(error = %e, "failed to write settings file");
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn default_settings_are_empty() {
        let s = AppSettings::default();
        assert!(s.nexus_api_key.is_empty());
        assert!(s.game_paths.is_empty());
        assert!(s.download_dir.is_none());
        assert!(s.theme.is_empty());
        assert!(s.selected_game.is_none());
    }

    #[test]
    fn game_path_lookup_and_set() {
        let mut s = AppSettings::default();
        assert!(s.game_path("cyberpunk2077").is_none());

        s.set_game_path("cyberpunk2077", PathBuf::from("/games/cp2077"));
        assert_eq!(
            s.game_path("cyberpunk2077"),
            Some(&PathBuf::from("/games/cp2077"))
        );

        // Update existing
        s.set_game_path("cyberpunk2077", PathBuf::from("/new/path"));
        assert_eq!(
            s.game_path("cyberpunk2077"),
            Some(&PathBuf::from("/new/path"))
        );
        assert_eq!(s.game_paths.len(), 1, "should update, not duplicate");
    }

    #[test]
    fn multiple_game_paths() {
        let mut s = AppSettings::default();
        s.set_game_path("skyrim-se", PathBuf::from("/games/skyrim"));
        s.set_game_path("cyberpunk2077", PathBuf::from("/games/cp2077"));

        assert_eq!(s.game_paths.len(), 2);
        assert_eq!(
            s.game_path("skyrim-se"),
            Some(&PathBuf::from("/games/skyrim"))
        );
        assert_eq!(
            s.game_path("cyberpunk2077"),
            Some(&PathBuf::from("/games/cp2077"))
        );
        assert!(s.game_path("fallout4").is_none());
    }

    #[test]
    fn save_and_load_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");

        let mut original = AppSettings::default();
        original.nexus_api_key = "test-key-123".into();
        original.set_game_path("cyberpunk2077", PathBuf::from("/games/cp2077"));
        original.set_game_path("skyrim-se", PathBuf::from("/games/skyrim"));
        original.selected_game = Some("cyberpunk2077".into());
        original.theme = "Nord".into();
        original.download_dir = Some(PathBuf::from("/downloads"));

        original.save_to(&path);

        let loaded = AppSettings::load_from(&path);
        assert_eq!(loaded.nexus_api_key, "test-key-123");
        assert_eq!(loaded.selected_game.as_deref(), Some("cyberpunk2077"));
        assert_eq!(loaded.theme, "Nord");
        assert_eq!(loaded.download_dir, Some(PathBuf::from("/downloads")));
        assert_eq!(
            loaded.game_path("cyberpunk2077"),
            Some(&PathBuf::from("/games/cp2077"))
        );
        assert_eq!(
            loaded.game_path("skyrim-se"),
            Some(&PathBuf::from("/games/skyrim"))
        );
    }

    #[test]
    fn load_missing_file_returns_default() {
        let s = AppSettings::load_from(Path::new("/nonexistent/settings.toml"));
        assert!(s.nexus_api_key.is_empty());
        assert!(s.game_paths.is_empty());
    }

    #[test]
    fn load_partial_toml_fills_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        std::fs::write(&path, "nexus_api_key = \"mykey\"\n").unwrap();

        let s = AppSettings::load_from(&path);
        assert_eq!(s.nexus_api_key, "mykey");
        assert!(s.game_paths.is_empty());
        assert!(s.selected_game.is_none());
    }

    #[test]
    fn load_with_unknown_fields_does_not_fail() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");
        std::fs::write(
            &path,
            "nexus_api_key = \"key\"\nunknown_field = \"value\"\n",
        )
        .unwrap();

        let s = AppSettings::load_from(&path);
        assert_eq!(s.nexus_api_key, "key");
    }

    #[test]
    fn toml_format_is_human_readable() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.toml");

        let mut s = AppSettings::default();
        s.set_game_path("cyberpunk2077", PathBuf::from("/games/cp2077"));
        s.selected_game = Some("cyberpunk2077".into());
        s.save_to(&path);

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("selected_game"));
        assert!(content.contains("cyberpunk2077"));
        assert!(content.contains("game_id"));
    }
}
