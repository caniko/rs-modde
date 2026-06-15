#![allow(clippy::wildcard_imports)]
use super::*;

#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct WabbajackInstallerState {
    pub tab: WabbajackTab,
    pub entries: Vec<modde_sources::wabbajack::catalog::WabbajackCatalogEntry>,
    pub loading: bool,
    pub error: Option<String>,
    pub search: String,
    pub game_filter: Option<String>,
    pub game_filter_user_edited: bool,
    pub official_only: bool,
    pub include_nsfw: bool,
    pub include_down: bool,
    pub selected_index: Option<usize>,
    pub manual_source: String,
    pub hm_profile: String,
    pub hm_game: String,
    pub hm_game_dir: String,
    pub hm_game_dir_user_edited: bool,
    pub hm_snippet: String,
    pub downloaded_path: Option<PathBuf>,
    pub file_path: Option<PathBuf>,
    pub readiness: Option<modde_sources::wabbajack::readiness::WabbajackReadinessReport>,
    pub readiness_loading: bool,
    pub readiness_error: Option<String>,
    pub installing: bool,
    pub install_phase: String,
    pub install_current_item: String,
    pub archive_import_results: Vec<modde_sources::wabbajack::import::ArchiveImportResult>,
    pub archive_import_status: Option<String>,
    pub progress: f32,
    pub status: String,
    pub log_lines: Vec<String>,
}

impl WabbajackInstallerState {
    #[must_use]
    pub fn install_blocker(&self) -> Option<String> {
        if self.installing {
            return Some("A Wabbajack install is already running.".to_string());
        }
        if self.file_path.is_none() {
            return Some(
                "Select or download a local .wabbajack file before installing.".to_string(),
            );
        }
        if self.readiness_loading {
            return Some("Wait for the readiness check to finish before installing.".to_string());
        }
        if let Some(error) = &self.readiness_error {
            return Some(format!("Fix the readiness check error first: {error}"));
        }
        let Some(readiness) = &self.readiness else {
            return Some("Run a readiness check before installing.".to_string());
        };
        if !readiness.hard_blockers.is_empty() {
            return Some("Resolve the hard blockers before installing.".to_string());
        }
        if !readiness.manual_downloads.is_empty() {
            return Some("Import the required manual archives before installing.".to_string());
        }
        if !readiness.install_ready {
            return Some("Resolve the readiness blockers before installing.".to_string());
        }
        None
    }

    #[must_use]
    pub fn can_install(&self) -> bool {
        self.install_blocker().is_none()
    }
}

pub(crate) fn prefill_wabbajack_game_dir(
    settings: &AppSettings,
    state: &mut WabbajackInstallerState,
) {
    if state.hm_game_dir_user_edited && !state.hm_game_dir.is_empty() {
        return;
    }
    let Some(path) = settings.game_path(&GameId::from(state.hm_game.as_str())) else {
        return;
    };
    state.hm_game_dir = path.display().to_string();
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WabbajackTab {
    #[default]
    Catalog,
    AuthoredFiles,
    Manual,
}
