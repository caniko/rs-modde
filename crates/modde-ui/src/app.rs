use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use crate::views::selectable_text::text;
use iced::widget::{button, column, container, mouse_area, opaque, row, stack, text_input};
use iced::{Element, Length, Task, Theme, window};
use smallvec::SmallVec;

use modde_core::filter::{FilterCriterion, FilterKind, FilterMode};
use modde_core::manifest::collection::CollectionManifest;
use modde_core::profile::ProfileManager;
use modde_core::resolver::GameId;
use modde_core::save::SaveSnapshot;
use modde_core::settings::AppSettings;

use crate::action_button::{ButtonAction, DescribedButtonExt};

mod fomod_wizard_state;
mod install_ops;
mod state;
mod tool_ops;
mod tool_settings;

pub use self::fomod_wizard_state::FOMODWizardState;
use self::install_ops::{
    download_wabbajack_source, format_anyhow_error, run_browse_install,
    run_wabbajack_install_for_ui, slugify_profile_name,
};
pub(crate) use self::state::format_lock_reason;
pub use self::state::{
    AddCustomGameDraft, AddCustomGameDraftField, AddCustomGameState, ExecutableDraft,
    ExecutableDraftField, ExecutableUiEntry, ReorderDirection, SidebarGroup, ToolApplyResult,
    ToolHistoryUiEntry, ToolLoadSnapshot, ToolReleaseSupport, ToolRevertResult, ToolState,
    ToolUiEntry, View, WabbajackInstallerState, WabbajackTab,
};
use self::state::{ToolLoadRequest, empty_to_none, prefill_wabbajack_game_dir};
pub use self::tool_ops::parse_executable_environment;
#[cfg(test)]
use self::tool_ops::validate_optiscaler_apply;
use self::tool_ops::{
    apply_tool_for_game, deactivate_optiscaler_for_game, executable_draft_to_row,
    install_selected_proton_version, install_selected_tool_release, load_executables_for_game,
    load_proton_versions, load_tool_releases, load_tools_state, remove_executable_for_game,
    restore_tool_settings_for_game, revert_tool_for_game, run_saved_executable_for_game,
    save_executable_for_game,
};
use self::tool_settings::{
    apply_derived_tool_settings, build_tool_derived_facts, current_tool_config,
    format_tool_availability, normalize_tool_setting_for_kind, normalize_tool_setting_value,
    normalize_tool_settings_for_specs, patch_tool_setting_options, save_tool_settings,
    set_nested_tool_setting, set_tool_options, sync_optiscaler_release_options,
    tool_apply_is_pending, tool_options,
};
#[cfg(test)]
use self::tool_settings::{get_tool_setting_value, tool_apply_signature};

pub type ToolOptionCatalog = HashMap<String, Vec<String>>;

const BUTTON_HOVER_TOAST_DELAY: Duration = Duration::from_secs(2);

/// Settings view state — consumed by the settings view.
#[derive(Debug, Clone, Default)]
pub struct SettingsState {
    pub nexus_api_key_draft: String,
    pub nexus_api_key_visible: bool,
    pub nexus_api_key_source: Option<modde_sources::nexus::auth::ApiKeySource>,
    pub nexus_config_key_exists: bool,
    pub game_install_paths: Vec<SettingsGameInstall>,
    pub download_dir: Option<PathBuf>,
    pub effective_download_dir: PathBuf,
    pub has_stock_snapshot: bool,
    pub theme_name: String,
    pub nexus_status: Option<NexusAuthStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsGameInstall {
    pub game_id: String,
    pub display_name: String,
    pub source: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub enum NexusAuthStatus {
    Checking,
    Valid { username: String, is_premium: bool },
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonHoverToast {
    pub id: u64,
    pub description: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ButtonHoverToastState {
    pub pending: Option<ButtonHoverToast>,
    pub visible: Option<ButtonHoverToast>,
}

/// Top-level application state.
#[allow(clippy::struct_excessive_bools)]
pub struct Modde {
    pub active_view: View,
    pub active_profile: Option<String>,
    pub profiles: Vec<modde_core::profile::ProfileSummary>,
    pub status_message: String,
    pub button_hover_toast: ButtonHoverToastState,
    pub pending_tools_load_status_message: Option<String>,
    pub settings: AppSettings,
    pub collection_search: String,
    pub collections: Vec<CollectionManifest>,
    pub fomod_installer: Option<FOMODWizardState>,
    pub fomod_visible_step_indices: SmallVec<[usize; 16]>,
    pub fomod_wizard_pos: usize,
    pub fomod_source_dir: Option<PathBuf>,
    pub fomod_dest_dir: Option<PathBuf>,
    pub fomod_conflicts: SmallVec<[String; 4]>,
    pub fomod_can_undo: bool,
    pub fomod_selections: HashMap<(usize, usize), Vec<usize>>,
    pub selected_mod_index: Option<usize>,
    /// Loaded Nexus metadata for the currently selected mod — populates the
    /// detail panel at the bottom of the left nav sidebar. `None` means no
    /// Nexus-tracked mod is selected (either nothing is selected or the
    /// selected mod has no `nexus_mod_id`).
    pub selected_mod_details: Option<crate::views::mod_details::ModDetailsState>,
    pub mod_filter: String,
    pub mod_id_filter_keys: Vec<String>,
    pub theme_name: String,
    pub wabbajack_manifest: Option<modde_core::WabbajackManifest>,
    pub active_downloads: Vec<crate::views::collections::CollectionDownload>,
    pub download_queue: modde_sources::queue::DownloadQueue,
    pub download_lookup: HashMap<String, usize>,
    // ── New state fields ──
    pub loaded_profile: Option<modde_core::Profile>,
    pub save_snapshots: Vec<SaveSnapshot>,
    pub current_fingerprint: Option<modde_core::save::SaveFingerprint>,
    pub selected_save_details: Option<crate::views::save_details::SaveDetailsState>,
    pub experiment_depth: usize,
    pub nexus_status: Option<NexusAuthStatus>,
    pub nexus_api_key_draft: String,
    pub nexus_api_key_visible: bool,
    pub nexus_api_key_source: Option<modde_sources::nexus::auth::ApiKeySource>,
    pub nexus_config_key_exists: bool,
    pub new_profile_name: String,
    pub new_profile_dialog_open: bool,
    pub game_path_dialog_open: bool,
    pub add_custom_game_dialog_open: bool,
    pub manage_custom_games_dialog_open: bool,
    pub pending_game_path_game_id: Option<String>,
    pub previous_game_before_path_dialog: Option<String>,
    pub game_path_dialog_error: Option<String>,
    pub add_custom_game: AddCustomGameState,
    pub available_games: SmallVec<[(String, String); 8]>,
    pub detected_games: HashSet<String>,
    pub selected_game: Option<String>,
    pub stock_snapshot_exists: bool,
    pub window_id: window::Id,
    /// Which category groups are collapsed in the mod list view.
    /// `None` key = the "Uncategorized" group.
    pub collapsed_categories: HashSet<Option<i64>>,
    /// Category id-to-name mapping for the mod list view.
    pub mod_categories: Vec<(Option<i64>, String)>,
    pub data_tab_state: crate::views::data_tab::DataTabState,
    pub data_tab_conflicts: Vec<(String, Vec<String>)>,
    /// State for the Browse Nexus view (Phase 6 of the installer pipeline).
    pub browse_nexus: crate::views::browse_nexus::NexusBrowseState,
    pub diagnostics_state: crate::views::diagnostics::DiagnosticsState,
    pub tool_state: ToolState,
    /// Filter mode (AND/OR) for the mod list filter toolbar.
    pub filter_mode: FilterMode,
    /// Active tri-state filter criteria for the mod list.
    pub filter_criteria: Vec<FilterCriterion>,
    /// Whether the mod list uses compact row rendering.
    pub compact_mod_list: bool,
    /// Sidebar groups the user has collapsed for this session.
    pub collapsed_sidebar_groups: HashSet<SidebarGroup>,
    pub update_available: Option<modde_core::update_check::UpdateInfo>,
}

fn load_hidden_files(
    pm: &ProfileManager,
    profile: &modde_core::Profile,
) -> HashSet<(String, String)> {
    profile
        .id
        .and_then(|profile_id| pm.db().list_hidden_files(profile_id).ok())
        .map(|rows| {
            rows.into_iter()
                .map(|row| (row.mod_id, row.rel_path))
                .collect()
        })
        .unwrap_or_default()
}

fn load_active_plugins(pm: &ProfileManager, profile: &modde_core::Profile) -> Vec<String> {
    let mut plugins = profile
        .id
        .and_then(|profile_id| pm.db().get_plugin_order(profile_id).ok())
        .unwrap_or_default();

    if plugins.is_empty() {
        plugins =
            modde_games::read_native_plugin_order(profile.game_id.as_str()).unwrap_or_default();
        if let Some(profile_id) = profile.id {
            let _ = pm.db().set_plugin_order(profile_id, &plugins);
        }
    }

    plugins
        .into_iter()
        .filter(|plugin| plugin.enabled)
        .map(|plugin| plugin.plugin_name)
        .collect()
}

fn detected_game_ids(
    settings: &AppSettings,
    available_games: &[(String, String)],
) -> HashSet<String> {
    let mut detected: HashSet<String> = settings
        .game_paths
        .iter()
        .filter(|game_path| game_path.path.is_dir())
        .map(|game_path| game_path.game_id.to_string())
        .collect();

    detected.extend(
        modde_games::scan_installed_games()
            .into_iter()
            .map(|game| game.game_id.to_string()),
    );

    for (game_id, _) in available_games {
        if !detected.contains(game_id)
            && modde_games::resolve_game_plugin(game_id)
                .and_then(modde_games::GamePlugin::detect_install)
                .is_some()
        {
            detected.insert(game_id.clone());
        }
    }

    detected
}

fn settings_game_install_paths(
    settings: &AppSettings,
    detected_games: Vec<modde_games::detection::DetectedGame>,
) -> Vec<SettingsGameInstall> {
    let mut seen = HashSet::new();
    let mut installs = Vec::new();

    for detected in detected_games {
        let game_id = detected.game_id.to_string();
        let path = detected.install_path;
        if !seen.insert((game_id.clone(), path.clone())) {
            continue;
        }
        installs.push(SettingsGameInstall {
            game_id,
            display_name: detected.display_name.to_string(),
            source: detected.source.to_string(),
            path,
        });
    }

    for game_path in &settings.game_paths {
        if !game_path.path.is_dir() {
            continue;
        }
        let game_id = game_path.game_id.to_string();
        let path = game_path.path.clone();
        if !seen.insert((game_id.clone(), path.clone())) {
            continue;
        }
        let display_name = modde_games::resolve_game_plugin(&game_id)
            .map(|plugin| plugin.display_name().to_string())
            .unwrap_or_else(|| game_id.clone());
        installs.push(SettingsGameInstall {
            game_id,
            display_name,
            source: "Configured".to_string(),
            path,
        });
    }

    installs.sort_by(|a, b| {
        a.display_name
            .cmp(&b.display_name)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.source.cmp(&b.source))
    });
    installs
}

fn build_conflict_rows(
    analysis: &modde_core::diagnostics::ProfileAnalysis,
    hidden: &HashSet<(String, String)>,
) -> Vec<(String, Vec<String>)> {
    let mut rows: Vec<(String, Vec<String>)> = analysis
        .conflict_map
        .resolved_conflicts(&analysis.resolved_order, hidden)
        .into_iter()
        .filter(|(_, providers, _)| providers.len() > 1)
        .map(|(path, providers, winner)| {
            let mut provider_list: Vec<String> = providers
                .iter()
                .map(|provider| {
                    if winner.as_ref() == Some(provider) {
                        format!("{provider} (winner)")
                    } else {
                        provider.to_string()
                    }
                })
                .collect();
            provider_list.sort();
            (path.to_string(), provider_list)
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

fn format_diagnostic_entry(
    diagnostic: &modde_core::diagnostics::Diagnostic,
) -> crate::views::diagnostics::DiagnosticEntry {
    let severity = match diagnostic.severity {
        modde_core::diagnostics::Severity::Info => {
            crate::views::diagnostics::DiagnosticSeverity::Info
        }
        modde_core::diagnostics::Severity::Warning => {
            crate::views::diagnostics::DiagnosticSeverity::Warning
        }
        modde_core::diagnostics::Severity::Error => {
            crate::views::diagnostics::DiagnosticSeverity::Error
        }
    };

    let mut message = diagnostic.title.clone();
    if !diagnostic.detail.is_empty() {
        message.push_str(": ");
        message.push_str(&diagnostic.detail);
    }
    if let Some(mod_id) = &diagnostic.affected_mod {
        message.push_str(&format!(" [mod: {mod_id}]"));
    }

    crate::views::diagnostics::DiagnosticEntry { severity, message }
}

fn build_default_download_meta(id: &str, name: &str) -> modde_sources::meta::DownloadMeta {
    modde_sources::meta::DownloadMeta {
        url: id.to_string(),
        expected_hash: None,
        bytes_downloaded: 0,
        total_bytes: None,
        nexus_mod_id: None,
        nexus_file_id: None,
        game_domain: None,
        mod_name: Some(name.to_string()),
        version: None,
        status: "queued".to_string(),
    }
}

impl Modde {
    pub fn settings_state(&self) -> SettingsState {
        SettingsState {
            nexus_api_key_draft: self.nexus_api_key_draft.clone(),
            nexus_api_key_visible: self.nexus_api_key_visible,
            nexus_api_key_source: self.nexus_api_key_source.clone(),
            nexus_config_key_exists: self.nexus_config_key_exists,
            game_install_paths: settings_game_install_paths(
                &self.settings,
                modde_games::scan_installed_games(),
            ),
            download_dir: self.settings.download_dir.clone(),
            effective_download_dir: self
                .settings
                .download_dir
                .clone()
                .unwrap_or_else(modde_core::paths::downloads_dir),
            has_stock_snapshot: self.stock_snapshot_exists,
            theme_name: self.theme_name.clone(),
            nexus_status: self.nexus_status.clone(),
        }
    }

    fn refresh_nexus_api_key_state(&mut self) {
        self.nexus_config_key_exists = modde_sources::nexus::auth::config_api_key_exists();
        if let Ok(loaded) = modde_sources::nexus::auth::load_api_key_with_source() {
            self.nexus_api_key_draft = loaded.key;
            self.nexus_api_key_source = Some(loaded.source);
        } else {
            self.nexus_api_key_draft.clear();
            self.nexus_api_key_source = None;
        }
    }

    pub fn fomod_is_last_step(&self) -> bool {
        if self.fomod_visible_step_indices.is_empty() {
            return true;
        }
        self.fomod_wizard_pos >= self.fomod_visible_step_indices.len().saturating_sub(1)
    }

    pub fn reset_fomod(&mut self) {
        self.fomod_installer = None;
        self.fomod_source_dir = None;
        self.fomod_dest_dir = None;
        self.fomod_visible_step_indices.clear();
        self.fomod_wizard_pos = 0;
        self.fomod_selections.clear();
        self.fomod_conflicts.clear();
        self.fomod_can_undo = false;
    }

    pub fn refresh_fomod_visible_steps(&mut self) {
        if let Some(ref installer) = self.fomod_installer {
            self.fomod_visible_step_indices = installer
                .visible_steps()
                .iter()
                .map(|&(idx, _)| idx)
                .collect();
        }
    }

    fn refresh_fomod_conflicts(&mut self) {
        if let Some(ref installer) = self.fomod_installer {
            self.fomod_conflicts = installer.detect_conflicts().into();
        }
    }

    fn clear_game_scoped_state(&mut self) {
        self.selected_mod_index = None;
        self.selected_mod_details = None;
        self.selected_save_details = None;
        self.save_snapshots.clear();
        self.current_fingerprint = None;
        self.experiment_depth = 0;
        self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Idle;
        self.data_tab_conflicts.clear();
        self.data_tab_state.missing_store_mod_count = 0;
    }

    fn game_supports_save_profiles(game_id: &str) -> bool {
        modde_games::resolve_game_plugin(game_id)
            .is_some_and(modde_games::GamePlugin::supports_save_profiles)
    }

    fn current_game_supports_save_profiles(&self) -> bool {
        self.loaded_profile
            .as_ref()
            .map(|p| p.game_id.as_str())
            .or(self.selected_game.as_deref())
            .is_some_and(Self::game_supports_save_profiles)
    }

    fn resolve_save_dir(game_id: &str) -> Option<PathBuf> {
        let plugin = modde_games::resolve_game_plugin(game_id)?;
        plugin
            .supports_save_profiles()
            .then(|| plugin.save_directory())
            .flatten()
    }

    fn reload_profile(&mut self) {
        if let Some(ref name) = self.active_profile {
            if let Ok(pm) = ProfileManager::open() {
                let selected_game_id = self.selected_game.as_deref().map(GameId::from);
                if let Some(game_id) = selected_game_id.as_ref() {
                    self.profiles = pm.list_for_game(game_id).unwrap_or_default();
                } else {
                    self.profiles = pm.list().unwrap_or_default();
                }
                if let Ok(profile) = pm.load(name, selected_game_id.as_ref()) {
                    if let Ok(info) = pm.active(&profile.game_id) {
                        self.experiment_depth = info.map_or(0, |i| i.experiment_depth);
                    }

                    // Compute save fingerprint
                    self.current_fingerprint = {
                        let game_id = profile.game_id.as_str();
                        let staging_dir = ProfileManager::staging_dir(&profile.name);
                        modde_games::resolve_game_plugin(game_id)
                            .filter(|plugin| plugin.supports_save_profiles())
                            .map(|plugin| {
                                modde_core::save::SaveFingerprint::compute(
                                    &profile.mods,
                                    |mod_id| {
                                        let mod_path = staging_dir.join(mod_id);
                                        plugin.classify_mod(&mod_path).affects_saves()
                                    },
                                )
                            })
                    };

                    self.mod_id_filter_keys = modde_core::filter::mod_id_filter_keys(&profile.mods);
                    self.loaded_profile = Some(profile);
                }
            }
        } else {
            self.loaded_profile = None;
            self.mod_id_filter_keys.clear();
        }

        self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Idle;
        self.refresh_data_tab_conflicts();
        self.refresh_tools_state();
    }

    fn switch_game_context(&mut self, game_id: &str) {
        self.clear_game_scoped_state();

        let Ok(pm) = ProfileManager::open() else {
            self.profiles.clear();
            self.active_profile = None;
            self.loaded_profile = None;
            self.mod_id_filter_keys.clear();
            self.status_message = "Failed to open profile database".to_string();
            return;
        };

        let typed_game_id = GameId::from(game_id);
        self.profiles = pm.list_for_game(&typed_game_id).unwrap_or_default();
        self.active_profile = pm
            .active(&typed_game_id)
            .ok()
            .flatten()
            .map(|info| info.profile.name)
            .or_else(|| self.profiles.first().map(|p| p.name.clone()));

        if self.active_profile.is_some() {
            self.reload_profile();
        } else {
            self.loaded_profile = None;
            self.mod_id_filter_keys.clear();
            self.refresh_data_tab_conflicts();
            self.refresh_tools_state();
        }
    }

    fn accept_game_selection(&mut self, game_id: String, previous_game: Option<String>) {
        self.selected_game = Some(game_id.clone());
        self.settings.selected_game = Some(game_id.clone());
        let typed_game_id = GameId::from(game_id.as_str());

        let configured_path_valid = self
            .settings
            .game_path(&typed_game_id)
            .is_some_and(|path| path.is_dir());
        if !configured_path_valid {
            if let Some(path) = modde_games::find_detected_game(&typed_game_id)
                .map(|detected| detected.install_path)
                .or_else(|| {
                    modde_games::resolve_game_plugin(&game_id)
                        .and_then(modde_games::GamePlugin::detect_install)
                })
            {
                self.settings.set_game_path(&typed_game_id, path);
                self.detected_games.insert(game_id.clone());
            } else {
                self.game_path_dialog_open = true;
                self.pending_game_path_game_id = Some(game_id.clone());
                self.previous_game_before_path_dialog = previous_game;
                self.game_path_dialog_error = None;
                self.status_message = format!("Set the game directory for {game_id}");
                self.save_settings();
                return;
            }
        }

        self.game_path_dialog_open = false;
        self.pending_game_path_game_id = None;
        self.previous_game_before_path_dialog = None;
        self.game_path_dialog_error = None;
        self.switch_game_context(&game_id);
        self.sync_browse_game_to_current(true);
        self.save_settings();
        self.status_message = format!("Active game set to {game_id}");
    }

    fn save_settings(&self) {
        self.settings.save();
    }

    fn refresh_available_games(&mut self) {
        self.available_games = modde_games::supported_games()
            .iter()
            .map(|(id, name)| (id.to_string(), name.to_string()))
            .collect();
        self.detected_games = detected_game_ids(&self.settings, self.available_games.as_slice());
    }

    fn custom_games(&self) -> Vec<(String, String)> {
        self.available_games
            .iter()
            .filter(|(id, _)| !modde_games::SUPPORTED_GAME_IDS.contains(&id.as_str()))
            .cloned()
            .collect()
    }

    fn current_game_id(&self) -> Option<&str> {
        self.loaded_profile
            .as_ref()
            .map(|profile| profile.game_id.as_str())
            .or(self.selected_game.as_deref())
    }

    fn current_game_dir(&self) -> Option<PathBuf> {
        let game_id = self.current_game_id()?;
        self.settings
            .game_path(&GameId::from(game_id))
            .cloned()
            .or_else(|| {
                modde_games::resolve_game_plugin(game_id)
                    .and_then(modde_games::GamePlugin::detect_install)
            })
    }

    fn add_custom_game_modal(&self) -> Element<'_, Message> {
        opaque(
            container(crate::views::add_custom_game::add_dialog(
                &self.add_custom_game,
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
    }

    fn manage_custom_games_modal(&self) -> Element<'_, Message> {
        opaque(
            container(crate::views::add_custom_game::manage_dialog(
                self.custom_games(),
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
    }

    fn current_tool_game_context(&self) -> Option<modde_games::tools::ToolGameContext> {
        let game_id = self.current_game_id()?;
        let display_name = self
            .available_games
            .iter()
            .find(|(id, _)| id == game_id)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| game_id.to_string());
        let install_path = self.current_game_dir();
        let detected = modde_games::detection::find_detected_game(&GameId::from(game_id));
        Some(modde_games::tools::ToolGameContext::from_parts(
            game_id,
            display_name,
            install_path,
            detected.as_ref(),
        ))
    }

    fn refresh_data_tab_conflicts(&mut self) {
        let Some(profile) = self.loaded_profile.as_ref() else {
            self.data_tab_conflicts.clear();
            self.data_tab_state.missing_store_mod_count = 0;
            return;
        };

        let Ok(pm) = ProfileManager::open() else {
            self.data_tab_conflicts.clear();
            self.data_tab_state.missing_store_mod_count = 0;
            return;
        };

        let hidden = load_hidden_files(&pm, profile);
        let classifier = modde_games::resolve_collision_classifier(profile.game_id.as_str());

        match modde_core::diagnostics::analyze_profile_state(
            profile,
            &modde_core::paths::store_dir(),
            &hidden,
            classifier.as_deref(),
        ) {
            Ok(analysis) => {
                self.data_tab_state.missing_store_mod_count = analysis.missing_store_mods.len();
                self.data_tab_conflicts = build_conflict_rows(&analysis, &hidden);
            }
            Err(err) => {
                self.data_tab_conflicts.clear();
                self.data_tab_state.missing_store_mod_count = 0;
                self.status_message = format!("Failed to load data tab: {err}");
            }
        }
    }

    fn run_diagnostics_now(&mut self) {
        let Some(profile) = self.loaded_profile.clone() else {
            self.status_message = "Select a profile before running diagnostics".to_string();
            self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Error(
                "Select a profile before running diagnostics.".to_string(),
            );
            return;
        };

        let Ok(pm) = ProfileManager::open() else {
            self.status_message = "Failed to open profile database".to_string();
            self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Error(
                "Failed to open profile database.".to_string(),
            );
            return;
        };

        let hidden = load_hidden_files(&pm, &profile);
        let active_plugins = load_active_plugins(&pm, &profile);
        let integrity = Self::verify_staging_integrity(&ProfileManager::staging_dir(&profile.name));
        let engine = match profile.game_id.as_str() {
            "skyrim-se" | "skyrim-ae" | "fallout4" | "fallout76" => {
                modde_games::bethesda::diagnostics::bethesda_diagnostics()
            }
            _ => modde_core::diagnostics::base_diagnostics(),
        };
        let classifier = modde_games::resolve_collision_classifier(profile.game_id.as_str());

        match modde_core::diagnostics::run_profile_diagnostics(
            profile.game_id.as_str(),
            &profile,
            &active_plugins,
            &modde_core::paths::store_dir(),
            &ProfileManager::staging_dir(&profile.name),
            &hidden,
            classifier.as_deref(),
            &engine,
        ) {
            Ok((diagnostics, analysis)) => {
                self.data_tab_state.missing_store_mod_count = analysis.missing_store_mods.len();
                self.data_tab_conflicts = build_conflict_rows(&analysis, &hidden);
                let entries: Vec<_> = diagnostics.iter().map(format_diagnostic_entry).collect();
                let diagnostic_count = entries.len();
                let broken_count = integrity.broken_symlinks.len();
                self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Complete(
                    crate::views::diagnostics::DiagnosticsReport {
                        profile_name: profile.name.clone(),
                        game_id: profile.game_id.to_string(),
                        entries,
                        integrity,
                    },
                );
                self.status_message = if diagnostic_count == 0 && broken_count == 0 {
                    "Diagnostics complete: no issues found".to_string()
                } else {
                    format!(
                        "Diagnostics complete: {diagnostic_count} issue(s), {broken_count} broken symlink(s)"
                    )
                };
            }
            Err(err) => {
                self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Error(
                    format!("Diagnostics failed: {err}"),
                );
                self.status_message = format!("Diagnostics failed: {err}");
            }
        }
    }

    fn verify_staging_integrity(
        staging_dir: &std::path::Path,
    ) -> crate::views::diagnostics::IntegritySummary {
        let mut results = crate::views::diagnostics::IntegritySummary::default();
        if !staging_dir.exists() {
            return results;
        }

        fn walk(dir: &std::path::Path, results: &mut crate::views::diagnostics::IntegritySummary) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        walk(&path, results);
                    } else if path.is_symlink() {
                        match std::fs::read_link(&path) {
                            Ok(target) if target.exists() => {
                                results.ok_count += 1;
                            }
                            _ => results.broken_symlinks.push(path),
                        }
                    } else {
                        results.ok_count += 1;
                    }
                }
            }
        }

        walk(staging_dir, &mut results);
        results
    }

    fn start_tools_load(&mut self) -> Task<Message> {
        let Some(request) = self.tool_load_request() else {
            self.tool_state.entries.clear();
            self.tool_state.active_tool_id = None;
            self.tool_state.game_label = None;
            self.tool_state.game_dir_configured = false;
            self.tool_state.loading = false;
            self.tool_state.load_error = None;
            self.status_message = "Select a game before loading tools".to_string();
            return Task::none();
        };
        self.tool_state.load_generation = self.tool_state.load_generation.wrapping_add(1);
        let generation = self.tool_state.load_generation;
        self.tool_state.loading = true;
        self.tool_state.load_error = None;
        Task::perform(load_tools_state(request), move |result| {
            Message::ToolsLoaded { generation, result }
        })
    }

    fn tool_load_request(&self) -> Option<ToolLoadRequest> {
        let game_id = self.current_game_id()?.to_string();
        let display_name = self
            .available_games
            .iter()
            .find(|(id, _)| id == &game_id)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| game_id.clone());
        Some(ToolLoadRequest {
            game_id,
            display_name,
            configured_game_dir: self
                .settings
                .game_path(&GameId::from(self.current_game_id()?))
                .cloned(),
            optiscaler_releases: self.tool_state.optiscaler_releases.clone(),
            tool_option_catalog: self.tool_state.tool_option_catalog.clone(),
            previous_active_tool_id: self.tool_state.active_tool_id.clone(),
        })
    }

    fn apply_tool_snapshot(&mut self, snapshot: ToolLoadSnapshot) {
        self.tool_state.entries = snapshot.entries;
        self.tool_state.active_tool_id = snapshot.active_tool_id;
        self.tool_state.game_label = snapshot.game_label;
        self.tool_state.game_dir_configured = snapshot.game_dir_configured;
        self.tool_state.tool_option_catalog = snapshot.tool_option_catalog;
        self.tool_state.executables = snapshot.executables;
        self.tool_state.loading = false;
        self.tool_state.load_error = None;
    }

    fn start_executables_load(&mut self) -> Task<Message> {
        let Some(game_id) = self.current_game_id().map(str::to_string) else {
            self.tool_state.executables.clear();
            self.tool_state.game_label = None;
            self.tool_state.executables_loading = false;
            self.tool_state.executables_load_error = None;
            self.status_message = "Select a game before loading executables".to_string();
            return Task::none();
        };
        self.tool_state.game_label = self
            .available_games
            .iter()
            .find(|(id, _)| id == &game_id)
            .map(|(_, name)| name.clone())
            .or_else(|| Some(game_id.clone()));
        self.tool_state.executables_load_generation =
            self.tool_state.executables_load_generation.wrapping_add(1);
        let generation = self.tool_state.executables_load_generation;
        self.tool_state.executables_loading = true;
        self.tool_state.executables_load_error = None;
        Task::perform(load_executables_for_game(game_id), move |result| {
            Message::ExecutablesLoaded { generation, result }
        })
    }

    fn refresh_executables_or_tools(&mut self) -> Task<Message> {
        if matches!(self.active_view, View::Executables) {
            self.start_executables_load()
        } else if self.tool_load_request().is_some() {
            self.start_tools_load()
        } else {
            Task::none()
        }
    }

    fn refresh_tools_state(&mut self) {
        let Some(game_id) = self.current_game_id().map(str::to_string) else {
            self.tool_state.entries.clear();
            self.tool_state.active_tool_id = None;
            self.tool_state.game_label = None;
            self.tool_state.game_dir_configured = false;
            return;
        };

        let Ok(db) = modde_core::db::ModdeDb::open() else {
            self.tool_state.entries.clear();
            self.tool_state.active_tool_id = None;
            return;
        };

        self.tool_state.game_label = self
            .available_games
            .iter()
            .find(|(id, _)| id == &game_id)
            .map(|(_, name)| name.clone())
            .or_else(|| Some(game_id.clone()));
        self.tool_state.game_dir_configured = self.current_game_dir().is_some();
        if tool_options(
            &self.tool_state.tool_option_catalog,
            "proton",
            "selected_version",
        )
        .is_none()
        {
            set_tool_options(
                &mut self.tool_state.tool_option_catalog,
                "proton",
                "selected_version",
                modde_games::tools::proton::proton_version_options(),
            );
        }
        if !self.tool_state.optiscaler_releases.is_empty()
            && let Ok(config) = current_tool_config(&game_id, "optiscaler")
        {
            let mut config = config;
            sync_optiscaler_release_options(
                &mut self.tool_state.tool_option_catalog,
                &self.tool_state.optiscaler_releases,
                &mut config,
            );
        }
        let context = self.current_tool_game_context();

        let typed_game_id = GameId::from(game_id.as_str());
        self.tool_state.entries = modde_games::tools::all_tools()
            .iter()
            .map(|tool| {
                let row = db
                    .load_tool_config(&typed_game_id, tool.tool_id())
                    .ok()
                    .flatten();
                let availability = tool.detect_available();
                let applied_files = db
                    .load_applied_files(&typed_game_id, tool.tool_id())
                    .unwrap_or_default();
                let status_message = match &availability {
                    modde_games::tools::ToolAvailability::Available {
                        version: Some(version),
                    } => Some(format!("Detected {version}")),
                    modde_games::tools::ToolAvailability::NotInstalled { install_hint } => {
                        Some(install_hint.clone())
                    }
                    modde_games::tools::ToolAvailability::Available { version: None } => None,
                };
                let availability_text = format_tool_availability(&availability);
                let mut config = row.as_ref().map_or_else(
                    || tool.default_config_for(context.as_ref()),
                    |row| modde_games::tools::ToolConfig {
                        tool_id: row.tool_id.clone(),
                        enabled: row.enabled,
                        settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
                    },
                );
                let mut setting_specs = tool.settings_schema_for(context.as_ref(), &config);
                let normalized_settings =
                    normalize_tool_settings_for_specs(&config.settings, &setting_specs);
                if normalized_settings != config.settings {
                    config.settings = normalized_settings;
                    if let Ok(settings_json) = serde_json::to_string(&config.settings) {
                        let _ = db.save_tool_config(
                            &typed_game_id,
                            tool.tool_id(),
                            config.enabled,
                            &settings_json,
                        );
                    }
                    setting_specs = tool.settings_schema_for(context.as_ref(), &config);
                }
                config.set("_game_id", serde_json::json!(game_id));
                apply_derived_tool_settings(&mut config, context.as_ref());
                let mut apply_pending = tool_apply_is_pending(&config, &applied_files);
                let mut apply_missing_inputs = Vec::new();
                let generated_config_path = tool
                    .generate_config_for(context.as_ref(), &config)
                    .map(|generated| generated.path.display().to_string());
                let env_preview = tool
                    .env_vars_for(context.as_ref(), &config)
                    .into_iter()
                    .collect();
                let dll_overrides = tool
                    .wine_dll_overrides_for(context.as_ref(), &config)
                    .into_iter()
                    .collect();
                let wrapper_preview = tool
                    .wrapper_command(&config)
                    .map(|wrapper| {
                        if wrapper.args.is_empty() {
                            vec![wrapper.exe]
                        } else {
                            vec![format!("{} {}", wrapper.exe, wrapper.args)]
                        }
                    })
                    .unwrap_or_default();
                patch_tool_setting_options(
                    tool.tool_id(),
                    &mut setting_specs,
                    &self.tool_state.tool_option_catalog,
                );
                let mut derived_facts = build_tool_derived_facts(context.as_ref());
                if matches!(tool.tool_id(), "reshade" | "optiscaler")
                    && let Some(game_dir) = self.current_game_dir()
                {
                    match tool.preview_apply_for(&game_dir, context.as_ref(), &config) {
                        Ok(preview) => {
                            let has_changes = preview.has_changes();
                            apply_missing_inputs = preview.missing_inputs.clone();
                            apply_pending =
                                apply_missing_inputs.is_empty() && has_changes;
                            let summary = if !apply_missing_inputs.is_empty() {
                                format!("missing input: {}", apply_missing_inputs.join("; "))
                            } else if has_changes {
                                format!(
                                    "{} changed / {} unchanged",
                                    preview.changed_files.len(),
                                    preview.unchanged_files.len()
                                )
                            } else {
                                format!("no changes ({} file(s))", preview.planned_files.len())
                            };
                            derived_facts.push(("Apply preview".to_string(), summary));
                        }
                        Err(err) => {
                            derived_facts
                                .push(("Apply preview".to_string(), format!("failed: {err}")));
                        }
                    }
                }
                let (optiscaler_state, optiscaler_latest_backup, optiscaler_detected_files) =
                    if tool.tool_id() == "optiscaler" {
                        let managed =
                            modde_games::tools::optiscaler::managed_paths_from_config(&config);
                        if let Some(game_dir) = self.current_game_dir() {
                            if let Ok(state) =
                                modde_games::tools::optiscaler::scan_optiscaler_install(
                                    &game_id, &game_dir, &managed,
                                )
                            {
                                if !matches!(
                                    state.status,
                                    modde_games::tools::optiscaler::OptiScalerInstallStatus::Managed
                                        | modde_games::tools::optiscaler::OptiScalerInstallStatus::PartiallyManaged
                                ) {
                                    apply_pending = true;
                                }
                                derived_facts
                                    .push(("OptiScaler state".to_string(), state.summary()));
                                if let Some(path) = &state.config_path {
                                    derived_facts.push((
                                        "OptiScaler config".to_string(),
                                        format!(
                                            "{} ({} setting(s))",
                                            path.display(),
                                            state.ini_settings.len()
                                        ),
                                    ));
                                }
                                if let Some(path) = &state.latest_backup {
                                    derived_facts.push((
                                        "OptiScaler backup".to_string(),
                                        path.display().to_string(),
                                    ));
                                }
                                (
                                    Some(state.summary()),
                                    state.latest_backup.map(|path| path.display().to_string()),
                                    state.recognized_files.len(),
                                )
                            } else {
                                (None, None, 0)
                            }
                        } else {
                            (None, None, 0)
                        }
                    } else {
                        (None, None, 0)
                    };
                let setting_history = db
                    .list_tool_setting_history(&typed_game_id, tool.tool_id(), 8)
                    .unwrap_or_default()
                    .into_iter()
                    .map(ToolHistoryUiEntry::from_node)
                    .collect();

                ToolUiEntry {
                    tool_id: tool.tool_id().to_string(),
                    display_name: tool.display_name().to_string(),
                    description: tool.description().to_string(),
                    category: tool.category().to_string(),
                    available: availability.is_available(),
                    availability_text,
                    enabled: config.enabled,
                    settings: config.settings.clone(),
                    setting_specs,
                    generated_config_path,
                    applied_files,
                    has_file_patching: matches!(tool.tool_id(), "reshade" | "optiscaler"),
                    release_support: ToolReleaseSupport::from_supports_releases(
                        tool.supports_releases(),
                    ),
                    status_message,
                    env_preview,
                    dll_overrides,
                    wrapper_preview,
                    derived_facts,
                    optiscaler_state,
                    optiscaler_latest_backup,
                    optiscaler_detected_files,
                    apply_pending,
                    apply_missing_inputs,
                    setting_history,
                }
            })
            .collect();

        let active_still_valid = self
            .tool_state
            .active_tool_id
            .as_deref()
            .is_some_and(|active| {
                self.tool_state
                    .entries
                    .iter()
                    .any(|entry| entry.tool_id == active)
            });
        if !active_still_valid {
            self.tool_state.active_tool_id = self
                .tool_state
                .entries
                .first()
                .map(|entry| entry.tool_id.clone());
        }
    }

    fn track_download(&mut self, key: &str, name: &str) -> usize {
        if let Some(id) = self.download_lookup.get(key).copied() {
            return id;
        }

        let dest_root = self
            .settings
            .download_dir
            .clone()
            .unwrap_or_else(modde_core::paths::downloads_dir);
        let file_name = key.replace(['/', ':', ' '], "_");
        let dest = dest_root.join(format!("{file_name}.download"));
        let id = self.download_queue.enqueue(
            key.to_string(),
            dest,
            None,
            build_default_download_meta(key, name),
        );
        self.download_lookup.insert(key.to_string(), id);
        id
    }

    fn downloads_view_tasks(&self) -> Vec<crate::views::downloads::DownloadTask> {
        self.download_queue
            .all()
            .iter()
            .map(|task| {
                let state = match &task.state {
                    modde_sources::queue::DownloadState::Queued => {
                        crate::views::downloads::DownloadState::Queued
                    }
                    modde_sources::queue::DownloadState::Active {
                        bytes_downloaded,
                        total_bytes,
                    } => crate::views::downloads::DownloadState::Active {
                        bytes_downloaded: *bytes_downloaded,
                        total_bytes: *total_bytes,
                    },
                    modde_sources::queue::DownloadState::Paused {
                        bytes_downloaded,
                        total_bytes,
                    } => crate::views::downloads::DownloadState::Paused {
                        bytes_downloaded: *bytes_downloaded,
                        total_bytes: *total_bytes,
                    },
                    modde_sources::queue::DownloadState::Complete { path, .. } => {
                        crate::views::downloads::DownloadState::Complete { path: path.clone() }
                    }
                    modde_sources::queue::DownloadState::Failed { error } => {
                        crate::views::downloads::DownloadState::Failed {
                            error: error.clone(),
                        }
                    }
                };

                crate::views::downloads::DownloadTask {
                    id: task.id,
                    name: task
                        .meta
                        .mod_name
                        .clone()
                        .unwrap_or_else(|| task.url.clone()),
                    state,
                }
            })
            .collect()
    }

    // ── Browse Nexus helpers (Phase 6) ──────────────────────────

    /// Return the currently-selected game's Nexus domain, if the game
    /// plugin defines one. Used by the Browse Nexus view to issue
    /// GraphQL queries scoped to the right game.
    pub fn current_game_nexus_domain(&self) -> Option<String> {
        let game_id = self
            .loaded_profile
            .as_ref()
            .map(|p| p.game_id.to_string())
            .or_else(|| self.selected_game.clone())?;
        Self::nexus_domain_for_game(&game_id)
    }

    fn nexus_domain_for_game(game_id: &str) -> Option<String> {
        let game = modde_games::resolve_game(game_id)?;
        game.nexus_game_id?;
        game.nexus_domain.map(str::to_string)
    }

    fn first_supported_nexus_game(&self) -> Option<String> {
        self.available_games
            .iter()
            .map(|(id, _)| id)
            .find(|id| Self::nexus_domain_for_game(id).is_some())
            .cloned()
    }

    fn default_browse_game_id(&self) -> Option<String> {
        self.current_game_id()
            .filter(|game_id| Self::nexus_domain_for_game(game_id).is_some())
            .map(str::to_string)
            .or_else(|| self.first_supported_nexus_game())
    }

    fn browse_game_nexus_domain(&self) -> Option<String> {
        self.browse_nexus
            .selected_game_id
            .as_deref()
            .and_then(Self::nexus_domain_for_game)
    }

    fn clear_browse_results(&mut self) {
        self.browse_nexus.mods.clear();
        self.browse_nexus.collections.clear();
        self.browse_nexus.error = None;
        self.browse_nexus.install_status = None;
    }

    fn sync_browse_game_to_current(&mut self, force: bool) {
        let selected_is_supported = self
            .browse_nexus
            .selected_game_id
            .as_deref()
            .is_some_and(|game_id| Self::nexus_domain_for_game(game_id).is_some());
        if !force && selected_is_supported {
            return;
        }
        let next = self.default_browse_game_id();
        if self.browse_nexus.selected_game_id != next {
            self.browse_nexus.selected_game_id = next;
            self.clear_browse_results();
        }
    }

    fn initialize_wabbajack_game_filter(&self, state: &mut WabbajackInstallerState) {
        if state.game_filter_user_edited || state.game_filter.is_some() {
            return;
        }
        state.game_filter = self.current_game_id().map(str::to_string);
    }

    /// Kick off an async feed load for the Browse Nexus view. Picks
    /// the right GraphQL query based on the tab.
    pub fn spawn_browse_load(
        &mut self,
        tab: crate::views::browse_nexus::BrowseTab,
        game_domain: String,
        search_query: String,
    ) -> Task<Message> {
        use crate::views::browse_nexus::BrowseTab;
        self.browse_nexus.loading = true;
        self.browse_nexus.error = None;
        match tab {
            BrowseTab::Top | BrowseTab::Month => {
                let kind = match tab {
                    BrowseTab::Top => modde_sources::nexus::graphql::ModFeedKind::Trending,
                    _ => modde_sources::nexus::graphql::ModFeedKind::MonthlyTop,
                };
                Task::perform(
                    async move {
                        let api_key = modde_sources::nexus::auth::load_api_key()
                            .map_err(|e| e.to_string())?;
                        let client = reqwest::Client::new();
                        let api = modde_sources::nexus::api::NexusApi::new(client, api_key);
                        api.browse_feed_gql(&game_domain, kind)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    Message::BrowseModsLoaded,
                )
            }
            BrowseTab::Search => Task::perform(
                async move {
                    let api_key =
                        modde_sources::nexus::auth::load_api_key().map_err(|e| e.to_string())?;
                    let client = reqwest::Client::new();
                    let api = modde_sources::nexus::api::NexusApi::new(client, api_key);
                    api.search_mods_gql(&game_domain, &search_query, 1)
                        .await
                        .map_err(|e| e.to_string())
                },
                Message::BrowseModsLoaded,
            ),
            BrowseTab::Collections => {
                let term = if search_query.is_empty() {
                    None
                } else {
                    Some(search_query)
                };
                Task::perform(
                    async move {
                        let api_key = modde_sources::nexus::auth::load_api_key()
                            .map_err(|e| e.to_string())?;
                        let client = reqwest::Client::new();
                        let api = modde_sources::nexus::api::NexusApi::new(client, api_key);
                        api.collections_feed_gql(&game_domain, term.as_deref())
                            .await
                            .map_err(|e| e.to_string())
                    },
                    Message::BrowseCollectionsLoaded,
                )
            }
        }
    }
}

// ─── Application Messages ────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Message {
    /// External process (typically the CLI) notified the GUI that the
    /// profile DB has changed. Triggers a profile reload.
    ExternalRefresh,

    // Navigation
    SwitchView(View),
    ToggleSidebarGroup(SidebarGroup),
    SwitchProfile(String),
    CreateProfile {
        name: String,
        game_id: String,
    },
    DeleteProfile(String),
    ForkProfile {
        source: String,
        new_name: String,
    },

    // Profile dialog
    OpenNewProfileDialog,
    NewProfileNameChanged(String),
    CancelNewProfileDialog,
    SubmitNewProfileDialog,

    // Game selection
    SelectGame(String),
    GamePathDialogBrowse,
    GamePathDialogPathSelected {
        game_id: String,
        path: PathBuf,
    },
    CancelGamePathDialog,
    OpenAddCustomGame,
    BrowseAddCustomGameInstallPath,
    AddCustomGameFieldChanged {
        field: AddCustomGameDraftField,
        value: String,
    },
    AddCustomGameInstallPathPicked(PathBuf),
    AddCustomGameSubmit,
    AddCustomGameCancel,
    OpenManageCustomGames,
    CloseManageCustomGames,
    RemoveCustomGame(String),

    // Window controls (custom title bar)
    GotWindowId(Option<window::Id>),
    TitleBarDrag,
    WindowMinimize,
    WindowToggleMaximize,
    WindowClose,

    // Mod list
    ToggleMod {
        mod_id: String,
        enabled: bool,
    },
    FilterChanged(String),
    AddMod,
    AddModFromPath(PathBuf),
    RemoveMod(usize),
    SelectMod(usize),
    /// Initial Nexus v1 `get_mod` response for the selected mod. Carries
    /// `nexus_mod_id` so stale responses (from a previous selection) are
    /// discarded when they race a newer click.
    ModDetailsLoaded {
        nexus_mod_id: modde_core::NexusModId,
        result: Result<modde_sources::nexus::api::NexusMod, String>,
    },
    /// Gallery image URL list returned by the v2 GraphQL endpoint.
    ModGalleryLoaded {
        nexus_mod_id: modde_core::NexusModId,
        urls: Vec<String>,
    },
    /// Image bytes downloaded for a specific gallery slot. Guarded by both
    /// `nexus_mod_id` and `gallery_index` so clicking through the gallery
    /// rapidly doesn't let an old image overwrite a newer one.
    ModThumbnailLoaded {
        nexus_mod_id: modde_core::NexusModId,
        gallery_index: usize,
        bytes: Vec<u8>,
    },
    /// User clicked the thumbnail — advance to the next image in the gallery.
    ModGalleryNext,
    /// User clicked the "Open in Nexus" link.
    OpenModPage,
    Deploy,
    DeployComplete(Result<String, String>),

    // Load order
    /// Move a specific mod up or down by one position. Mod-id-based (not
    /// index-based) because the `load_order` view and `mod_list` view operate
    /// on different index spaces — `resolved_order` vs. `profile.mods` —
    /// and an index-based message was latently unsound. Also lets the
    /// handler consult the per-mod lock without an index round-trip.
    ReorderMod {
        mod_id: String,
        direction: ReorderDirection,
    },
    /// Pin an individual mod in place (per-mod lock).
    LockMod {
        mod_id: String,
    },
    /// Release an individual mod's per-mod pin.
    UnlockMod {
        mod_id: String,
    },

    // Collections
    SearchCollections(String),
    InstallCollection {
        slug: String,
        version: String,
    },

    // ── Browse Nexus (Phase 6) ───────────────────────────────
    /// Switch the active browse tab. Fires a task to load the feed
    /// for the new tab if its contents are empty.
    BrowseTabSwitched(crate::views::browse_nexus::BrowseTab),
    /// Switch the Nexus browser to a different supported game.
    BrowseGameChanged(Option<String>),
    /// Live search box keystroke.
    BrowseSearchChanged(String),
    /// Submit the search (Enter pressed). Runs the appropriate query
    /// depending on the active tab.
    BrowseSearchSubmit,
    /// Async result of a mods feed fetch.
    BrowseModsLoaded(Result<Vec<modde_sources::nexus::graphql::GqlModTile>, String>),
    /// Async result of a collections feed fetch.
    BrowseCollectionsLoaded(Result<Vec<modde_sources::nexus::graphql::GqlCollectionTile>, String>),
    /// User clicked "Install" on a mod tile. Runs the install
    /// pipeline via `modde_sources::nexus::install::install_single_mod`.
    BrowseInstallMod {
        game_domain: String,
        mod_id: modde_core::NexusModId,
    },
    /// Async completion of a browse install. The `Ok` payload is a
    /// short human-readable status message; `Err` is an error string.
    BrowseInstallResult {
        download_key: String,
        result: Result<String, String>,
    },

    // Wabbajack
    LoadWabbajackCatalog,
    WabbajackCatalogLoaded(
        Result<Vec<modde_sources::wabbajack::catalog::WabbajackCatalogEntry>, String>,
    ),
    WabbajackTabChanged(WabbajackTab),
    WabbajackSearchChanged(String),
    WabbajackGameFilterChanged(Option<String>),
    WabbajackToggleOfficialOnly(bool),
    WabbajackToggleNsfw(bool),
    WabbajackToggleDown(bool),
    WabbajackSelectEntry(usize),
    WabbajackManualSourceChanged(String),
    WabbajackHmProfileChanged(String),
    WabbajackHmGameChanged(String),
    WabbajackHmGameDirChanged(String),
    WabbajackDownloadSelected,
    WabbajackDownloadComplete(Result<PathBuf, String>),
    WabbajackGenerateHmSnippet,
    WabbajackHmSnippetGenerated(Result<String, String>),
    WabbajackCopyHmSnippet,
    WabbajackSaveHmSnippet,
    WabbajackHmSnippetSaved(Result<PathBuf, String>),
    WabbajackOpenUrl(String),
    OpenWabbajackFile,
    WabbajackFileSelected(PathBuf),
    WabbajackProgress(f32),
    WabbajackStartInstall,
    WabbajackInstallComplete(Result<(String, Vec<String>), String>),
    WabbajackLog(String),

    // FOMOD
    StartFOMOD {
        mod_path: PathBuf,
        dest_path: PathBuf,
    },
    FOMODChoice {
        step: usize,
        group: usize,
        option: usize,
        selected: bool,
    },
    FOMODNext,
    FOMODBack,
    FOMODCancel,
    FOMODUndo,
    FOMODInstallComplete(Result<(), String>),

    // Downloads
    DownloadProgress {
        id: String,
        bytes: u64,
        total: u64,
    },
    DownloadComplete {
        id: String,
    },
    DownloadFailed {
        id: String,
        error: String,
    },

    // Settings
    SetNexusApiKeyDraft(String),
    ToggleNexusApiKeyVisibility,
    ReplaceNexusApiKey,
    RemoveNexusConfigKey,
    SetGamePath {
        game_id: String,
        path: PathBuf,
    },
    SetDownloadDir(PathBuf),
    BrowseGamePath,
    BrowseDownloadDir,
    SetTheme(String),
    ValidateNexusKey,
    NexusKeyValidated(Result<(String, bool), String>),

    // Stock game
    CreateStockSnapshot,
    StockSnapshotCreated(Result<String, String>),
    VerifyStockSnapshot,
    StockVerifyResult(Result<String, String>),

    // Experiments
    TryProfile,
    RollbackExperiment,
    CommitExperiment,

    // Saves
    LoadSaveHistory,
    RestoreSaveSnapshot(String),
    SelectSaveSnapshot(String),

    // Mod list – category separators
    ToggleSeparator(Option<i64>),

    // Data tab
    DataTabFilterChanged(String),
    DataTabToggleConflicts(bool),

    // Diagnostics
    RunDiagnostics,

    // Tools
    LoadTools,
    ToolsLoaded {
        generation: u64,
        result: Result<ToolLoadSnapshot, String>,
    },
    RefreshTools,
    LoadExecutables,
    RefreshExecutables,
    ExecutablesLoaded {
        generation: u64,
        result: Result<Vec<ExecutableUiEntry>, String>,
    },
    SelectToolTab(String),
    UpdateToolSetting {
        tool_id: String,
        key: String,
        value: serde_json::Value,
    },
    ToggleTool {
        tool_id: String,
        enabled: bool,
    },
    ToggleToolAdvancedSettings,
    ApplyTool(String),
    RevertTool(String),
    ActivateOptiScaler,
    DeactivateOptiScaler,
    AdoptOptiScaler,
    RestoreOptiScalerBackup,
    ResetOptiScalerConfig,
    RestoreToolSettings {
        tool_id: String,
        node_id: String,
    },
    ToolSettingsRestored {
        tool_id: String,
        result: Result<String, String>,
    },
    RefreshOptiScalerReleases,
    OptiScalerReleasesLoaded(Result<Vec<modde_games::tools::ToolReleaseSummary>, String>),
    InstallOptiScalerRelease,
    OptiScalerReleaseInstalled(Result<String, String>),
    RefreshProtonVersions,
    ProtonVersionsLoaded(Result<Vec<String>, String>),
    InstallProtonVersion,
    ProtonVersionInstalled(Result<String, String>),
    ToolApplied {
        tool_id: String,
        result: Result<ToolApplyResult, String>,
    },
    ToolReverted {
        tool_id: String,
        result: Result<ToolRevertResult, String>,
    },
    UpdateExecutableDraft {
        field: ExecutableDraftField,
        value: String,
    },
    OpenExecutableEditor,
    ClearExecutableDraft,
    EditExecutable(String),
    SaveExecutable,
    ExecutableSaved(Result<String, String>),
    RemoveExecutable(String),
    ExecutableRemoved {
        name: String,
        result: Result<String, String>,
    },
    RunExecutable(String),
    ExecutableRunComplete {
        name: String,
        result: Result<String, String>,
    },
    BrowseExecutablePath,
    ExecutablePathSelected(Option<PathBuf>),
    BrowseExecutableWorkingDir,
    ExecutableWorkingDirSelected(Option<PathBuf>),

    // Downloads
    PauseDownload(usize),
    ResumeDownload(usize),
    CancelDownload(usize),

    // Sidebar mod detail — Nexus interactions
    /// User clicked the endorse/abstain toggle button.
    ModEndorseToggle,
    /// Async result of an endorse or abstain call. Carries the target
    /// status the handler optimistically applied, so it can roll back if
    /// the request failed.
    ModEndorseResult {
        nexus_mod_id: modde_core::NexusModId,
        new_status: String,
        result: Result<(), String>,
    },
    /// User clicked the track/untrack toggle button.
    ModTrackToggle,
    /// Async result of a track or untrack call.
    ModTrackResult {
        nexus_mod_id: modde_core::NexusModId,
        new_tracked: bool,
        result: Result<(), String>,
    },
    /// Async result of the initial `get_tracked_mods` call fired alongside
    /// `get_mod` when a mod is selected.
    ModTrackedSetLoaded {
        nexus_mod_id: modde_core::NexusModId,
        is_tracked: bool,
    },

    // Overwrite management
    ClearOverwrite,
    MoveOverwriteToMod(String),

    // Mod list filter toolbar
    ToggleFilterMode,
    CycleFilter(FilterKind),
    ClearFilters,
    ToggleCompactModList,

    // Button hover help
    ButtonHoverStarted {
        id: u64,
        description: &'static str,
    },
    ButtonHoverElapsed {
        id: u64,
    },
    ButtonHoverEnded {
        id: u64,
    },

    // Misc
    Noop,
    UpdateCheckLoaded(Result<Option<modde_core::update_check::UpdateInfo>, String>),
    OpenUpdateReleasePage,
    DismissUpdateBanner,
}

// ─── Application Logic ──────────────────────────────────────────

impl Modde {
    fn new() -> (Self, Task<Message>) {
        let settings = AppSettings::load();
        let theme_name = if settings.theme.is_empty() {
            "Dark".to_string()
        } else {
            settings.theme.clone()
        };
        let selected_game = settings.selected_game.clone();

        let all_profiles = ProfileManager::open()
            .and_then(|pm| pm.list())
            .unwrap_or_default();

        let available_games: SmallVec<[(String, String); 8]> = modde_games::supported_games()
            .iter()
            .map(|(id, name)| (id.to_string(), name.to_string()))
            .collect();
        let detected_games = detected_game_ids(&settings, available_games.as_slice());

        let mut app = Self {
            active_view: View::ModList,
            active_profile: None,
            profiles: Vec::new(),
            status_message: "Ready".to_string(),
            button_hover_toast: ButtonHoverToastState::default(),
            pending_tools_load_status_message: None,
            settings,
            collection_search: String::new(),
            collections: Vec::new(),
            fomod_installer: None,
            fomod_visible_step_indices: SmallVec::new(),
            fomod_wizard_pos: 0,
            fomod_source_dir: None,
            fomod_dest_dir: None,
            fomod_conflicts: SmallVec::new(),
            fomod_can_undo: false,
            fomod_selections: HashMap::new(),
            selected_mod_index: None,
            selected_mod_details: None,
            mod_filter: String::new(),
            mod_id_filter_keys: Vec::new(),
            theme_name,
            wabbajack_manifest: None,
            active_downloads: Vec::new(),
            download_queue: modde_sources::queue::DownloadQueue::new(2),
            download_lookup: HashMap::new(),
            loaded_profile: None,
            save_snapshots: Vec::new(),
            current_fingerprint: None,
            selected_save_details: None,
            experiment_depth: 0,
            nexus_status: None,
            nexus_api_key_draft: String::new(),
            nexus_api_key_visible: false,
            nexus_api_key_source: None,
            nexus_config_key_exists: false,
            new_profile_name: String::new(),
            new_profile_dialog_open: false,
            game_path_dialog_open: false,
            add_custom_game_dialog_open: false,
            manage_custom_games_dialog_open: false,
            pending_game_path_game_id: None,
            previous_game_before_path_dialog: None,
            game_path_dialog_error: None,
            add_custom_game: AddCustomGameState::default(),
            available_games,
            detected_games,
            selected_game,
            stock_snapshot_exists: false,
            window_id: window::Id::unique(),
            collapsed_categories: HashSet::new(),
            mod_categories: vec![(None, "Uncategorized".to_string())],
            data_tab_state: Default::default(),
            data_tab_conflicts: Vec::new(),
            diagnostics_state: Default::default(),
            tool_state: Default::default(),
            browse_nexus: Default::default(),
            filter_mode: FilterMode::default(),
            filter_criteria: vec![
                FilterCriterion::new(FilterKind::Enabled),
                FilterCriterion::new(FilterKind::HasNotes),
                FilterCriterion::new(FilterKind::HasNexusId),
            ],
            compact_mod_list: false,
            collapsed_sidebar_groups: HashSet::from([SidebarGroup::General]),
            update_available: None,
        };
        app.refresh_nexus_api_key_state();

        // Auto-detect: if no game is selected but profiles exist, pick the first profile's game
        if app.selected_game.is_none()
            && let Some(first) = all_profiles.first()
        {
            app.selected_game = Some(first.game_id.to_string());
            app.settings.selected_game = Some(first.game_id.to_string());
        }

        if let Some(game_id) = app.selected_game.clone() {
            app.accept_game_selection(game_id, None);
        }

        (
            app,
            Task::batch([
                window::oldest().map(Message::GotWindowId),
                Task::perform(
                    async {
                        modde_core::update_check::check_latest()
                            .await
                            .map_err(|error| error.to_string())
                    },
                    Message::UpdateCheckLoaded,
                ),
            ]),
        )
    }

    fn title(&self) -> String {
        "modde".to_string()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // External process pushed a refresh signal (CLI install,
            // update apply, uninstall, …). Re-read the profile from
            // the shared sqlite DB so the user sees the change without
            // restarting or switching profiles.
            Message::ExternalRefresh => {
                self.reload_profile();
                self.status_message = "Refreshed from external change".to_string();
            }
            Message::UpdateCheckLoaded(result) => match result {
                Ok(update) => {
                    self.update_available = update;
                }
                Err(error) => {
                    tracing::debug!(%error, "GUI update check failed");
                }
            },
            Message::OpenUpdateReleasePage => {
                if let Some(update) = self.update_available.clone() {
                    return Task::perform(
                        async move {
                            let _ = open::that(update.release_url);
                        },
                        |()| Message::Noop,
                    );
                }
            }
            Message::DismissUpdateBanner => {
                self.update_available = None;
            }

            // ── Navigation ───────────────────────────────────────
            Message::SwitchView(view) => match view {
                View::Saves => {
                    self.active_view = View::Saves;
                    if !self.current_game_supports_save_profiles() {
                        self.save_snapshots.clear();
                        self.selected_save_details = None;
                        self.current_fingerprint = None;
                        self.status_message =
                            "Save profiles are not supported for this game".to_string();
                        return Task::none();
                    }
                    return self.update(Message::LoadSaveHistory);
                }
                View::DataTab => {
                    self.active_view = View::DataTab;
                    self.refresh_data_tab_conflicts();
                }
                View::Tools => {
                    self.active_view = View::Tools;
                    return self.update(Message::LoadTools);
                }
                View::Executables => {
                    self.active_view = View::Executables;
                    return self.update(Message::LoadExecutables);
                }
                View::Diagnostics => {
                    self.active_view = View::Diagnostics;
                    return self.update(Message::RunDiagnostics);
                }
                View::WabbajackInstaller(state) => {
                    let mut state = state;
                    self.initialize_wabbajack_game_filter(&mut state);
                    let should_load = state.entries.is_empty();
                    self.active_view = View::WabbajackInstaller(state);
                    if should_load {
                        return self.update(Message::LoadWabbajackCatalog);
                    }
                }
                View::BrowseNexus => {
                    self.active_view = View::BrowseNexus;
                    self.sync_browse_game_to_current(false);
                }
                other => {
                    self.active_view = other;
                }
            },
            Message::ToggleSidebarGroup(group) => {
                if !self.collapsed_sidebar_groups.insert(group) {
                    self.collapsed_sidebar_groups.remove(&group);
                }
            }
            Message::SwitchProfile(name) => {
                self.active_profile = Some(name);
                self.reload_profile();
                self.sync_browse_game_to_current(true);
                self.selected_save_details = None;
                self.status_message = "Profile switched".to_string();
                if matches!(self.active_view, View::Diagnostics) {
                    return self.update(Message::RunDiagnostics);
                }
            }
            Message::CreateProfile { name, game_id } => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    self.status_message = "Profile name is required".to_string();
                    return Task::none();
                }
                match ProfileManager::open() {
                    Ok(pm) => {
                        let profile = modde_core::Profile {
                            id: None,
                            name: name.clone(),
                            game_id: modde_core::GameId::from(game_id.clone()),
                            source: modde_core::ProfileSource::Manual,
                            mods: Vec::new(),
                            overrides: PathBuf::from("overrides"),
                            load_order_rules: smallvec::SmallVec::new(),
                            load_order_lock: None,
                        };
                        match pm.create(&profile) {
                            Ok(_) => {
                                self.profiles = pm
                                    .list_for_game(&GameId::from(game_id.as_str()))
                                    .unwrap_or_default();
                                self.active_profile = Some(name);
                                self.selected_game = Some(game_id.clone());
                                self.settings.selected_game = Some(game_id);
                                self.save_settings();
                                self.reload_profile();
                                self.new_profile_name.clear();
                                self.new_profile_dialog_open = false;
                                self.status_message = "Profile created".to_string();
                            }
                            Err(e) => {
                                self.status_message = format!("Failed to create profile: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        self.status_message = format!("Failed to open profile manager: {e}");
                    }
                }
            }
            Message::DeleteProfile(name) => match ProfileManager::open() {
                Ok(pm) => match pm.delete(&name, None) {
                    Ok(()) => {
                        if let Some(game_id) = self.selected_game.clone() {
                            self.switch_game_context(&game_id);
                        } else {
                            self.profiles = pm.list().unwrap_or_default();
                        }
                        if self.active_profile.as_deref() == Some(&name) {
                            self.active_profile = self.profiles.first().map(|p| p.name.clone());
                            self.reload_profile();
                        }
                        self.status_message = format!("Profile '{name}' deleted");
                    }
                    Err(e) => self.status_message = format!("Failed to delete profile: {e}"),
                },
                Err(e) => self.status_message = format!("Error: {e}"),
            },
            Message::ForkProfile { source, new_name } => {
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    match ProfileManager::open() {
                        Ok(pm) => match pm.fork(&source, &new_name, &game_id) {
                            Ok(_) => {
                                self.profiles = pm.list().unwrap_or_default();
                                self.active_profile = Some(new_name.clone());
                                self.reload_profile();
                                self.status_message = format!("Profile forked as '{new_name}'");
                            }
                            Err(e) => self.status_message = format!("Fork failed: {e}"),
                        },
                        Err(e) => self.status_message = format!("Error: {e}"),
                    }
                }
            }

            // ── Profile dialog ───────────────────────────────────
            Message::OpenNewProfileDialog => {
                self.new_profile_dialog_open = true;
            }
            Message::NewProfileNameChanged(name) => self.new_profile_name = name,
            Message::CancelNewProfileDialog => {
                self.new_profile_dialog_open = false;
                self.new_profile_name.clear();
            }
            Message::SubmitNewProfileDialog => {
                let Some(game_id) = self.selected_game.clone() else {
                    self.status_message = "Select a game before creating a profile".to_string();
                    return Task::none();
                };
                let name = self.new_profile_name.trim().to_string();
                if name.is_empty() {
                    self.status_message = "Profile name is required".to_string();
                    return Task::none();
                }
                return self.update(Message::CreateProfile { name, game_id });
            }

            // ── Game selection ────────────────────────────────────
            Message::SelectGame(game_id) => {
                let previous_game = self.selected_game.clone();
                self.accept_game_selection(game_id, previous_game);
            }
            Message::GamePathDialogBrowse => {
                let Some(game_id) = self.pending_game_path_game_id.clone() else {
                    return Task::none();
                };
                return Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select Game Directory")
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    move |path| match path {
                        Some(path) => Message::GamePathDialogPathSelected {
                            game_id: game_id.clone(),
                            path,
                        },
                        None => Message::Noop,
                    },
                );
            }
            Message::GamePathDialogPathSelected { game_id, path } => {
                if !path.is_dir() {
                    self.game_path_dialog_error =
                        Some(format!("Not a directory: {}", path.display()));
                    self.status_message = "Select a valid game directory".to_string();
                    return Task::none();
                }
                self.settings
                    .set_game_path(&GameId::from(game_id.as_str()), path);
                self.detected_games.insert(game_id.clone());
                self.selected_game = Some(game_id.clone());
                self.settings.selected_game = Some(game_id.clone());
                self.game_path_dialog_open = false;
                self.pending_game_path_game_id = None;
                self.previous_game_before_path_dialog = None;
                self.game_path_dialog_error = None;
                self.switch_game_context(&game_id);
                self.save_settings();
                self.status_message = format!("Active game set to {game_id}");
            }
            Message::CancelGamePathDialog => {
                let previous = self.previous_game_before_path_dialog.clone();
                self.game_path_dialog_open = false;
                self.pending_game_path_game_id = None;
                self.previous_game_before_path_dialog = None;
                self.game_path_dialog_error = None;
                self.selected_game = previous.clone();
                self.settings.selected_game = previous.clone();
                if let Some(game_id) = previous {
                    self.switch_game_context(&game_id);
                    self.status_message = format!("Active game remains {game_id}");
                } else {
                    self.clear_game_scoped_state();
                    self.profiles.clear();
                    self.active_profile = None;
                    self.loaded_profile = None;
                    self.mod_id_filter_keys.clear();
                    self.status_message = "Game selection cancelled".to_string();
                }
                self.save_settings();
            }
            Message::OpenAddCustomGame => {
                self.add_custom_game_dialog_open = true;
                self.manage_custom_games_dialog_open = false;
                self.add_custom_game.error = None;
            }
            Message::BrowseAddCustomGameInstallPath => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select Custom Game Install Directory")
                            .pick_folder()
                            .await
                            .map(|handle| handle.path().to_path_buf())
                    },
                    |path| match path {
                        Some(path) => Message::AddCustomGameInstallPathPicked(path),
                        None => Message::Noop,
                    },
                );
            }
            Message::AddCustomGameFieldChanged { field, value } => {
                self.add_custom_game.error = None;
                match field {
                    AddCustomGameDraftField::Id => self.add_custom_game.draft.id = value,
                    AddCustomGameDraftField::DisplayName => {
                        self.add_custom_game.draft.display_name = value;
                    }
                    AddCustomGameDraftField::InstallPath => {
                        self.add_custom_game.draft.install_path = value;
                        self.add_custom_game.draft.executable_dir = None;
                        self.add_custom_game.detected_dirs.clear();
                    }
                    AddCustomGameDraftField::ExecutableDir => {
                        self.add_custom_game.draft.executable_dir = Some(value);
                    }
                    AddCustomGameDraftField::SteamAppId => {
                        self.add_custom_game.draft.steam_app_id = empty_to_none(Some(&value));
                    }
                    AddCustomGameDraftField::NexusDomain => {
                        self.add_custom_game.draft.nexus_domain = empty_to_none(Some(&value));
                    }
                    AddCustomGameDraftField::ProxyDlls => {
                        self.add_custom_game.draft.proxy_dlls_csv = value;
                    }
                }
            }
            Message::AddCustomGameInstallPathPicked(path) => {
                self.add_custom_game.error = None;
                self.add_custom_game.draft.install_path = path.display().to_string();
                match modde_games::detect_candidates(&path) {
                    Ok(candidates) => {
                        self.add_custom_game.detected_dirs = candidates;
                        self.add_custom_game.draft.executable_dir = self
                            .add_custom_game
                            .detected_dirs
                            .first()
                            .map(|candidate| candidate.relative_dir.clone());
                    }
                    Err(error) => {
                        self.add_custom_game.detected_dirs.clear();
                        self.add_custom_game.draft.executable_dir = None;
                        self.add_custom_game.error = Some(error.to_string());
                    }
                }
            }
            Message::AddCustomGameSubmit => {
                let install_path = PathBuf::from(self.add_custom_game.draft.install_path.trim());
                let spec = match self.add_custom_game.build_spec() {
                    Ok(spec) => spec,
                    Err(error) => {
                        self.add_custom_game.error = Some(error.clone());
                        self.status_message = error;
                        return Task::none();
                    }
                };
                match modde_games::add_user_game(&spec, false) {
                    Ok(_) => {
                        modde_games::reload_user_games();
                        self.settings
                            .set_game_path(&GameId::from(spec.id.as_str()), install_path);
                        self.refresh_available_games();
                        self.add_custom_game_dialog_open = false;
                        self.add_custom_game = AddCustomGameState::default();
                        self.accept_game_selection(spec.id.clone(), self.selected_game.clone());
                        self.status_message =
                            format!("Registered custom game '{}'", spec.display_name);
                    }
                    Err(error) => {
                        self.add_custom_game.error = Some(error.to_string());
                        self.status_message = format!("Custom game not saved: {error}");
                    }
                }
            }
            Message::AddCustomGameCancel => {
                self.add_custom_game_dialog_open = false;
                self.add_custom_game = AddCustomGameState::default();
            }
            Message::OpenManageCustomGames => {
                self.manage_custom_games_dialog_open = true;
                self.add_custom_game_dialog_open = false;
                self.add_custom_game.error = None;
            }
            Message::CloseManageCustomGames => {
                self.manage_custom_games_dialog_open = false;
            }
            Message::RemoveCustomGame(id) => match modde_games::remove_user_game(&id) {
                Ok(_) => {
                    modde_games::reload_user_games();
                    self.settings
                        .game_paths
                        .retain(|entry| entry.game_id.as_ref() != id.as_str());
                    if self.settings.selected_game.as_deref() == Some(id.as_str()) {
                        self.settings.selected_game = None;
                        self.selected_game = None;
                    }
                    self.refresh_available_games();
                    if self.selected_game.is_none()
                        && let Some((game_id, _)) = self.available_games.first().cloned()
                    {
                        self.accept_game_selection(game_id, None);
                    }
                    self.save_settings();
                    self.status_message = format!("Removed custom game '{id}'");
                }
                Err(error) => {
                    self.status_message = format!("Custom game not removed: {error}");
                }
            },

            // ── Window controls (custom title bar) ───────────────
            Message::GotWindowId(Some(id)) => {
                self.window_id = id;
            }
            Message::GotWindowId(None) => {}
            Message::TitleBarDrag => {
                return window::drag(self.window_id);
            }
            Message::WindowMinimize => {
                return window::minimize(self.window_id, true);
            }
            Message::WindowToggleMaximize => {
                return window::toggle_maximize(self.window_id);
            }
            Message::WindowClose => {
                return window::close(self.window_id);
            }

            // ── Mod list ─────────────────────────────────────────
            Message::ToggleMod { mod_id, enabled } => {
                if let Some(ref profile_name) = self.active_profile
                    && let Ok(pm) = ProfileManager::open()
                    && let Ok(mut profile) = pm.load(profile_name, None)
                {
                    if let Some(m) = profile.mods.iter_mut().find(|m| m.mod_id == mod_id) {
                        m.enabled = enabled;
                    }
                    let _ = pm
                        .create(&profile)
                        .or_else(|_| pm.update(&profile).map(|()| 0));
                    self.status_message = format!(
                        "Mod {mod_id} {}",
                        if enabled { "enabled" } else { "disabled" }
                    );
                    self.reload_profile();
                }
            }
            Message::FilterChanged(filter) => self.mod_filter = filter,
            Message::ToggleFilterMode => {
                self.filter_mode = self.filter_mode.toggle();
            }
            Message::CycleFilter(kind) => {
                if let Some(c) = self.filter_criteria.iter_mut().find(|c| c.kind == kind) {
                    c.state = c.state.cycle();
                }
            }
            Message::ClearFilters => {
                for c in &mut self.filter_criteria {
                    c.state = modde_core::filter::TriState::Ignore;
                }
            }
            Message::ToggleCompactModList => {
                self.compact_mod_list = !self.compact_mod_list;
            }
            Message::ToggleSeparator(cat_id) => {
                if !self.collapsed_categories.remove(&cat_id) {
                    self.collapsed_categories.insert(cat_id);
                }
            }
            Message::AddMod => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select Mod Archive or Directory")
                            .add_filter("Archives", &["zip", "7z", "rar"])
                            .pick_file()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    |path| match path {
                        Some(p) => Message::AddModFromPath(p),
                        None => Message::Noop,
                    },
                );
            }
            Message::AddModFromPath(path) => {
                if let Some(ref profile_name) = self.active_profile {
                    let mod_name = path.file_stem().map_or_else(
                        || "unknown-mod".to_string(),
                        |s| s.to_string_lossy().to_string(),
                    );

                    if let Ok(pm) = ProfileManager::open()
                        && let Ok(mut profile) = pm.load(profile_name, None)
                    {
                        profile.mods.push(modde_core::EnabledMod {
                            mod_id: mod_name.clone(),
                            enabled: true,
                            ..Default::default()
                        });
                        let _ = pm
                            .create(&profile)
                            .or_else(|_| pm.update(&profile).map(|()| 0));
                        self.status_message = format!("Added mod: {mod_name}");
                        self.reload_profile();
                    }
                } else {
                    self.status_message = "No active profile — create one first".to_string();
                }
            }
            Message::RemoveMod(index) => {
                if let Some(ref profile_name) = self.active_profile
                    && let Ok(pm) = ProfileManager::open()
                    && let Ok(mut profile) = pm.load(profile_name, None)
                    && index < profile.mods.len()
                {
                    let removed = profile.mods.remove(index);
                    let _ = pm
                        .create(&profile)
                        .or_else(|_| pm.update(&profile).map(|()| 0));
                    self.selected_mod_index = None;
                    self.status_message = format!("Removed mod: {}", removed.mod_id);
                    self.reload_profile();
                }
            }
            Message::SelectMod(index) => {
                self.selected_mod_index = Some(index);

                // Look up the selected mod and, if it carries Nexus metadata,
                // kick off an async fetch for its full details. Otherwise
                // clear the detail panel.
                //
                // Nexus game domains are canonically lowercase (e.g.
                // "cyberpunk2077"). Historical DB records came from URL path
                // segments and may be mixed-case, which causes Nexus v1 to
                // return 401 Unauthorized rather than 404 — so we lowercase
                // defensively before any API call.
                let nexus_info = self
                    .loaded_profile
                    .as_ref()
                    .and_then(|p| p.mods.get(index))
                    .and_then(|m| {
                        let nid = m.nexus_mod_id?;
                        let domain = m.nexus_game_domain.clone()?.to_lowercase();
                        Some((
                            nid,
                            domain,
                            m.display_name.clone().unwrap_or_else(|| m.mod_id.clone()),
                            m.version.clone().unwrap_or_default(),
                        ))
                    });

                match nexus_info {
                    Some((nexus_mod_id, game_domain, name, version)) => {
                        self.selected_mod_details =
                            Some(crate::views::mod_details::ModDetailsState::loading(
                                nexus_mod_id,
                                game_domain.clone(),
                                name,
                                version,
                            ));

                        return Task::perform(
                            async move {
                                let api_key = modde_sources::nexus::auth::load_api_key()
                                    .map_err(|e| e.to_string())?;
                                let client = reqwest::Client::new();
                                let api = modde_sources::nexus::api::NexusApi::new(client, api_key);
                                api.get_mod(&game_domain, nexus_mod_id)
                                    .await
                                    .map_err(|e| e.to_string())
                            },
                            move |result| Message::ModDetailsLoaded {
                                nexus_mod_id,
                                result,
                            },
                        );
                    }
                    None => {
                        self.selected_mod_details = None;
                    }
                }
            }
            Message::ModDetailsLoaded {
                nexus_mod_id,
                result,
            } => {
                // Guard against stale responses for a previous selection.
                let matches = self
                    .selected_mod_details
                    .as_ref()
                    .is_some_and(|s| s.nexus_mod_id == nexus_mod_id);
                if !matches {
                    return Task::none();
                }

                match result {
                    Ok(nexus_mod) => {
                        let picture_url = nexus_mod.picture_url.clone();
                        let game_domain = self
                            .selected_mod_details
                            .as_ref()
                            .map(|s| s.game_domain.clone())
                            .unwrap_or_default();

                        if let Some(ref mut s) = self.selected_mod_details {
                            s.loading = false;
                            s.name = nexus_mod.name;
                            s.author = nexus_mod.author;
                            s.version = nexus_mod.version;
                            s.summary = nexus_mod.summary;
                            s.endorse_status = nexus_mod
                                .endorsement
                                .as_ref()
                                .map(|e| e.endorse_status.clone());
                            s.endorsement_count = nexus_mod.endorsement_count;
                            if let Some(ref url) = picture_url {
                                s.gallery = vec![url.clone()];
                                s.gallery_index = 0;
                            }
                        }

                        // Fire follow-up fetches: (a) primary picture bytes,
                        // (b) full gallery via GraphQL. Both are best-effort.
                        // Key is loaded via `auth::load_api_key` inside the
                        // task so OAuth/keyring/env are all honored.
                        let mut tasks: Vec<Task<Message>> = Vec::new();

                        if let Some(url) = picture_url {
                            tasks.push(Task::perform(
                                async move {
                                    let api_key =
                                        modde_sources::nexus::auth::load_api_key().ok()?;
                                    let client = reqwest::Client::new();
                                    let api =
                                        modde_sources::nexus::api::NexusApi::new(client, api_key);
                                    api.fetch_bytes(&url).await.ok()
                                },
                                move |bytes_opt| match bytes_opt {
                                    Some(bytes) => Message::ModThumbnailLoaded {
                                        nexus_mod_id,
                                        gallery_index: 0,
                                        bytes,
                                    },
                                    None => Message::Noop,
                                },
                            ));
                        }

                        if !game_domain.is_empty() {
                            let domain = game_domain.clone();
                            tasks.push(Task::perform(
                                async move {
                                    let api_key = modde_sources::nexus::auth::load_api_key()
                                        .unwrap_or_default();
                                    if api_key.is_empty() {
                                        return Vec::new();
                                    }
                                    let client = reqwest::Client::new();
                                    let api =
                                        modde_sources::nexus::api::NexusApi::new(client, api_key);
                                    api.get_mod_media(&domain, nexus_mod_id)
                                        .await
                                        .unwrap_or_default()
                                },
                                move |urls| Message::ModGalleryLoaded { nexus_mod_id, urls },
                            ));
                        }

                        // Check whether the current user is tracking this
                        // mod. The v1 endpoint is not filterable by mod_id,
                        // so we download the full tracked list and filter.
                        // Best-effort — on failure, the Track button stays
                        // disabled (is_tracked = None).
                        if !game_domain.is_empty() {
                            let domain = game_domain.clone();
                            tasks.push(Task::perform(
                                async move {
                                    let api_key =
                                        modde_sources::nexus::auth::load_api_key().ok()?;
                                    let client = reqwest::Client::new();
                                    let api =
                                        modde_sources::nexus::api::NexusApi::new(client, api_key);
                                    let list = api.get_tracked_mods().await.ok()?;
                                    let target = nexus_mod_id;
                                    Some(list.iter().any(|t| {
                                        t.mod_id == target
                                            && t.domain_name.eq_ignore_ascii_case(&domain)
                                    }))
                                },
                                move |is_tracked_opt| match is_tracked_opt {
                                    Some(is_tracked) => Message::ModTrackedSetLoaded {
                                        nexus_mod_id,
                                        is_tracked,
                                    },
                                    None => Message::Noop,
                                },
                            ));
                        }

                        return Task::batch(tasks);
                    }
                    Err(e) => {
                        if let Some(ref mut s) = self.selected_mod_details {
                            s.loading = false;
                            s.error = Some(e);
                        }
                    }
                }
            }
            Message::ModGalleryLoaded { nexus_mod_id, urls } => {
                let Some(ref mut s) = self.selected_mod_details else {
                    return Task::none();
                };
                if s.nexus_mod_id != nexus_mod_id {
                    return Task::none();
                }
                if urls.is_empty() {
                    return Task::none();
                }

                // Merge: keep the existing picture_url (gallery[0]) as the
                // first entry so the already-fetched thumbnail stays valid,
                // then append any gallery URLs not already in the list.
                let mut merged: Vec<String> = s.gallery.clone();
                for url in urls {
                    if !merged.contains(&url) {
                        merged.push(url);
                    }
                }
                s.gallery = merged;
            }
            Message::ModThumbnailLoaded {
                nexus_mod_id,
                gallery_index,
                bytes,
            } => {
                let Some(ref mut s) = self.selected_mod_details else {
                    return Task::none();
                };
                if s.nexus_mod_id != nexus_mod_id || s.gallery_index != gallery_index {
                    return Task::none();
                }
                s.thumbnail = Some(resize_thumbnail_bytes(&bytes));
            }
            Message::ModGalleryNext => {
                let (nexus_mod_id, next_index, url) = {
                    let Some(ref mut s) = self.selected_mod_details else {
                        return Task::none();
                    };
                    if s.gallery.len() < 2 {
                        return Task::none();
                    }
                    s.gallery_index = (s.gallery_index + 1) % s.gallery.len();
                    s.thumbnail = None;
                    let url = s.gallery[s.gallery_index].clone();
                    (s.nexus_mod_id, s.gallery_index, url)
                };
                return Task::perform(
                    async move {
                        let api_key = modde_sources::nexus::auth::load_api_key().ok()?;
                        let client = reqwest::Client::new();
                        let api = modde_sources::nexus::api::NexusApi::new(client, api_key);
                        api.fetch_bytes(&url).await.ok()
                    },
                    move |bytes_opt| match bytes_opt {
                        Some(bytes) => Message::ModThumbnailLoaded {
                            nexus_mod_id,
                            gallery_index: next_index,
                            bytes,
                        },
                        None => Message::Noop,
                    },
                );
            }
            Message::OpenModPage => {
                if let Some(ref s) = self.selected_mod_details {
                    let url = s.mod_page_url.clone();
                    // Surface the URL in the status bar so the user can
                    // verify exactly what's being passed to the browser
                    // (useful for diagnosing case-sensitivity issues with
                    // historical capitalized DB records).
                    self.status_message = format!("Opening: {url}");
                    tracing::info!(url = %url, "opening mod page in browser");
                    // Spawn on blocking pool — `open::that` forks xdg-open
                    // and usually returns quickly, but we don't want any
                    // chance of stalling the UI event loop.
                    return Task::perform(
                        async move {
                            let _ = tokio::task::spawn_blocking(move || {
                                let _ = open::that(&url);
                            })
                            .await;
                        },
                        |()| Message::Noop,
                    );
                }
            }
            Message::ModEndorseToggle => {
                // Snapshot what we need from state and optimistically
                // flip the UI before the API request returns.
                let Some(ref mut s) = self.selected_mod_details else {
                    return Task::none();
                };
                if s.action_pending {
                    return Task::none();
                }
                let nexus_mod_id = s.nexus_mod_id;
                let game_domain = s.game_domain.clone();
                let version = s.version.clone();
                let was_endorsed = s.endorse_status.as_deref() == Some("Endorsed");
                let new_status = if was_endorsed {
                    "Abstained"
                } else {
                    "Endorsed"
                };
                s.endorse_status = Some(new_status.to_string());
                // Adjust the visible total: +1 when going to Endorsed, -1
                // when leaving it. `saturating_sub` guards against weirdness
                // if the count happens to be 0.
                if was_endorsed {
                    s.endorsement_count = s.endorsement_count.saturating_sub(1);
                } else {
                    s.endorsement_count = s.endorsement_count.saturating_add(1);
                }
                s.action_pending = true;

                let target_status = new_status.to_string();
                return Task::perform(
                    async move {
                        let api_key = modde_sources::nexus::auth::load_api_key()
                            .map_err(|e| e.to_string())?;
                        let client = reqwest::Client::new();
                        let api = modde_sources::nexus::api::NexusApi::new(client, api_key);
                        if was_endorsed {
                            api.abstain_mod(&game_domain, nexus_mod_id, &version)
                                .await
                                .map_err(|e| e.to_string())
                        } else {
                            api.endorse_mod(&game_domain, nexus_mod_id, &version)
                                .await
                                .map_err(|e| e.to_string())
                        }
                    },
                    move |result| Message::ModEndorseResult {
                        nexus_mod_id,
                        new_status: target_status.clone(),
                        result,
                    },
                );
            }
            Message::ModEndorseResult {
                nexus_mod_id,
                new_status,
                result,
            } => {
                let Some(ref mut s) = self.selected_mod_details else {
                    return Task::none();
                };
                if s.nexus_mod_id != nexus_mod_id {
                    return Task::none();
                }
                s.action_pending = false;
                match result {
                    Ok(()) => {
                        // Optimistic state already matches — nothing to do.
                        self.status_message = if new_status == "Endorsed" {
                            "Endorsed on Nexus".to_string()
                        } else {
                            "Endorsement withdrawn".to_string()
                        };
                    }
                    Err(e) => {
                        // Roll back the optimistic update.
                        let reverted = if new_status == "Endorsed" {
                            "Abstained"
                        } else {
                            "Endorsed"
                        };
                        s.endorse_status = Some(reverted.to_string());
                        if new_status == "Endorsed" {
                            s.endorsement_count = s.endorsement_count.saturating_sub(1);
                        } else {
                            s.endorsement_count = s.endorsement_count.saturating_add(1);
                        }
                        self.status_message = format!("Endorse failed: {e}");
                    }
                }
            }
            Message::ModTrackToggle => {
                let Some(ref mut s) = self.selected_mod_details else {
                    return Task::none();
                };
                if s.action_pending {
                    return Task::none();
                }
                let nexus_mod_id = s.nexus_mod_id;
                let game_domain = s.game_domain.clone();
                // If is_tracked is None (not yet fetched), assume not
                // tracked — clicking Track will try to track, and the
                // result is idempotent enough on the server side.
                let was_tracked = s.is_tracked.unwrap_or(false);
                let new_tracked = !was_tracked;
                s.is_tracked = Some(new_tracked);
                s.action_pending = true;

                return Task::perform(
                    async move {
                        let api_key = modde_sources::nexus::auth::load_api_key()
                            .map_err(|e| e.to_string())?;
                        let client = reqwest::Client::new();
                        let api = modde_sources::nexus::api::NexusApi::new(client, api_key);
                        if was_tracked {
                            api.untrack_mod(&game_domain, nexus_mod_id)
                                .await
                                .map_err(|e| e.to_string())
                        } else {
                            api.track_mod(&game_domain, nexus_mod_id)
                                .await
                                .map_err(|e| e.to_string())
                        }
                    },
                    move |result| Message::ModTrackResult {
                        nexus_mod_id,
                        new_tracked,
                        result,
                    },
                );
            }
            Message::ModTrackResult {
                nexus_mod_id,
                new_tracked,
                result,
            } => {
                let Some(ref mut s) = self.selected_mod_details else {
                    return Task::none();
                };
                if s.nexus_mod_id != nexus_mod_id {
                    return Task::none();
                }
                s.action_pending = false;
                match result {
                    Ok(()) => {
                        self.status_message = if new_tracked {
                            "Now tracking on Nexus".to_string()
                        } else {
                            "Stopped tracking".to_string()
                        };
                    }
                    Err(e) => {
                        // Roll back the optimistic flip.
                        s.is_tracked = Some(!new_tracked);
                        self.status_message = format!("Track toggle failed: {e}");
                    }
                }
            }
            Message::ModTrackedSetLoaded {
                nexus_mod_id,
                is_tracked,
            } => {
                if let Some(ref mut s) = self.selected_mod_details
                    && s.nexus_mod_id == nexus_mod_id
                {
                    s.is_tracked = Some(is_tracked);
                }
            }
            Message::Deploy => {
                self.status_message = "Deploying mods...".to_string();
                if let Some(ref profile) = self.loaded_profile {
                    let profile_name = profile.name.clone();
                    let game_id = profile.game_id.clone();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || -> Result<String, String> {
                                let pm = ProfileManager::open().map_err(|e| e.to_string())?;
                                let profile = pm
                                    .load(&profile_name, Some(&game_id))
                                    .map_err(|e| e.to_string())?;
                                let resolved = modde_core::resolver::resolve(&profile)
                                    .map_err(|e| e.to_string())?;
                                let game_plugin =
                                    modde_games::resolve_game_plugin(game_id.as_str())
                                        .ok_or_else(|| format!("unsupported game: {game_id}"))?;
                                let install_path =
                                    game_plugin.detect_install().ok_or_else(|| {
                                        format!("could not detect install for {game_id}")
                                    })?;
                                let staging_dir = ProfileManager::staging_dir(&profile.name);
                                game_plugin
                                    .deploy_to_install(&staging_dir, &install_path)
                                    .map_err(|e| e.to_string())?;
                                game_plugin
                                    .post_deploy(&install_path)
                                    .map_err(|e| e.to_string())?;
                                Ok(format!(
                                    "Deployed {} mod(s) for {}",
                                    resolved.order.len(),
                                    game_id
                                ))
                            })
                            .await
                            .map_err(|e| e.to_string())?
                        },
                        Message::DeployComplete,
                    );
                }
            }
            Message::DeployComplete(result) => match result {
                Ok(msg) => self.status_message = msg,
                Err(e) => self.status_message = format!("Deploy failed: {e}"),
            },

            // ── Load order ───────────────────────────────────────
            Message::ReorderMod { mod_id, direction } => {
                let Some(ref profile_name) = self.active_profile else {
                    return Task::none();
                };
                let Ok(pm) = ProfileManager::open() else {
                    self.status_message = "Failed to open profile database".to_string();
                    return Task::none();
                };
                let Ok(mut profile) = pm.load(profile_name, None) else {
                    return Task::none();
                };

                // All enforcement lives in `modde_core::profile::try_reorder`
                // — profile lock, per-mod pin, adjacent pin, boundary. The
                // UI just translates refusal reasons into status messages.
                use modde_core::profile::{ReorderError, try_reorder};
                match try_reorder(&mut profile, &mod_id, direction) {
                    Ok(()) => {
                        let _ = pm
                            .create(&profile)
                            .or_else(|_| pm.update(&profile).map(|()| 0));
                        self.status_message = format!(
                            "Moved '{mod_id}' {}",
                            match direction {
                                ReorderDirection::Up => "up",
                                ReorderDirection::Down => "down",
                            }
                        );
                        self.reload_profile();
                    }
                    Err(ReorderError::ProfileLocked { reason }) => {
                        self.status_message = format!(
                            "Load order is locked by {} — unlock the profile to reorder.",
                            format_lock_reason(&reason)
                        );
                    }
                    Err(ReorderError::ModPinned {
                        mod_id: mid,
                        reason,
                    }) => {
                        self.status_message = format!(
                            "'{mid}' is pinned ({}) — unpin it to reorder.",
                            format_lock_reason(&reason)
                        );
                    }
                    Err(ReorderError::AdjacentPinned { neighbor_id, .. }) => {
                        self.status_message =
                            format!("Cannot move past a pinned mod ('{neighbor_id}').");
                    }
                    Err(ReorderError::ModNotFound { mod_id: mid }) => {
                        self.status_message = format!("Mod not found in profile: {mid}");
                    }
                    Err(ReorderError::AtBoundary) => {
                        // Silent — the view shouldn't have offered the
                        // button, but if we got here anyway it's a no-op.
                    }
                }
            }

            Message::LockMod { mod_id } => {
                let Some(ref profile_name) = self.active_profile else {
                    return Task::none();
                };
                let Ok(pm) = ProfileManager::open() else {
                    return Task::none();
                };
                let Ok(mut profile) = pm.load(profile_name, None) else {
                    return Task::none();
                };
                if let Some(m) = profile.mods.iter_mut().find(|m| m.mod_id == mod_id) {
                    m.lock = Some(modde_core::LockReason::Manual { note: None });
                    if pm.update(&profile).is_ok() {
                        self.status_message = format!("Pinned '{mod_id}'");
                        self.reload_profile();
                    }
                }
            }
            Message::UnlockMod { mod_id } => {
                let Some(ref profile_name) = self.active_profile else {
                    return Task::none();
                };
                let Ok(pm) = ProfileManager::open() else {
                    return Task::none();
                };
                let Ok(mut profile) = pm.load(profile_name, None) else {
                    return Task::none();
                };
                if let Some(m) = profile.mods.iter_mut().find(|m| m.mod_id == mod_id) {
                    m.lock = None;
                    if pm.update(&profile).is_ok() {
                        self.status_message = format!("Unpinned '{mod_id}'");
                        self.reload_profile();
                    }
                }
            }

            // ── Collections ──────────────────────────────────────
            Message::SearchCollections(query) => {
                self.collection_search = query.clone();
                if query.is_empty() {
                    self.collections = Vec::new();
                    return Task::none();
                }
                self.status_message = "Searching collections...".to_string();
                return Task::perform(
                    async move { Ok::<Vec<CollectionManifest>, anyhow::Error>(Vec::new()) },
                    |result| match result {
                        Ok(_) => Message::Noop,
                        Err(_) => Message::Noop,
                    },
                );
            }
            Message::InstallCollection { slug, version } => {
                self.status_message = format!("Installing collection {slug} v{version}...");
            }

            // ── Browse Nexus (Phase 6) ───────────────────────────
            Message::BrowseTabSwitched(tab) => {
                self.sync_browse_game_to_current(false);
                self.browse_nexus.active_tab = tab;
                self.browse_nexus.error = None;
                let domain = match self.browse_game_nexus_domain() {
                    Some(d) => d,
                    None => return Task::none(),
                };
                return self.spawn_browse_load(tab, domain, self.browse_nexus.search_query.clone());
            }
            Message::BrowseGameChanged(game_id) => {
                self.browse_nexus.selected_game_id = game_id;
                self.clear_browse_results();
                let domain = match self.browse_game_nexus_domain() {
                    Some(d) => d,
                    None => return Task::none(),
                };
                let tab = self.browse_nexus.active_tab;
                let query = self.browse_nexus.search_query.clone();
                return self.spawn_browse_load(tab, domain, query);
            }
            Message::BrowseSearchChanged(query) => {
                self.browse_nexus.search_query = query;
            }
            Message::BrowseSearchSubmit => {
                self.sync_browse_game_to_current(false);
                self.browse_nexus.active_tab = crate::views::browse_nexus::BrowseTab::Search;
                self.browse_nexus.error = None;
                let domain = match self.browse_game_nexus_domain() {
                    Some(d) => d,
                    None => return Task::none(),
                };
                let tab = self.browse_nexus.active_tab;
                let query = self.browse_nexus.search_query.clone();
                return self.spawn_browse_load(tab, domain, query);
            }
            Message::BrowseModsLoaded(result) => {
                self.browse_nexus.loading = false;
                match result {
                    Ok(mods) => {
                        self.browse_nexus.mods = mods;
                        self.browse_nexus.error = None;
                    }
                    Err(e) => {
                        self.browse_nexus.mods.clear();
                        self.browse_nexus.error = Some(e);
                    }
                }
            }
            Message::BrowseCollectionsLoaded(result) => {
                self.browse_nexus.loading = false;
                match result {
                    Ok(cols) => {
                        self.browse_nexus.collections = cols;
                        self.browse_nexus.error = None;
                    }
                    Err(e) => {
                        self.browse_nexus.collections.clear();
                        self.browse_nexus.error = Some(e);
                    }
                }
            }
            Message::BrowseInstallMod {
                game_domain,
                mod_id,
            } => {
                self.browse_nexus.install_status = Some(format!("Installing mod {mod_id}…"));
                let download_key = format!("browse:{game_domain}:{mod_id}");
                let task_id = self.track_download(&download_key, &format!("Nexus mod {mod_id}"));
                if let Some(task) = self.download_queue.get_mut(task_id) {
                    task.state = modde_sources::queue::DownloadState::Active {
                        bytes_downloaded: 0,
                        total_bytes: None,
                    };
                    task.meta.status = "installing".to_string();
                }
                return Task::perform(
                    async move { run_browse_install(game_domain, mod_id).await },
                    move |result| Message::BrowseInstallResult {
                        download_key: download_key.clone(),
                        result,
                    },
                );
            }
            Message::BrowseInstallResult {
                download_key,
                result,
            } => match result {
                Ok(msg) => {
                    self.browse_nexus.install_status = Some(msg.clone());
                    self.status_message = msg;
                    if let Some(task_id) = self.download_lookup.get(&download_key).copied()
                        && let Some(task) = self.download_queue.get_mut(task_id)
                    {
                        task.state = modde_sources::queue::DownloadState::Complete {
                            path: task.dest.clone(),
                            hash: 0,
                        };
                        task.meta.status = "complete".to_string();
                    }
                    self.reload_profile();
                }
                Err(e) => {
                    self.browse_nexus.install_status = Some(format!("Install failed: {e}"));
                    self.status_message = format!("Install failed: {e}");
                    if let Some(task_id) = self.download_lookup.get(&download_key).copied()
                        && let Some(task) = self.download_queue.get_mut(task_id)
                    {
                        task.state =
                            modde_sources::queue::DownloadState::Failed { error: e.clone() };
                        task.meta.status = "failed".to_string();
                    }
                }
            },

            // ── Wabbajack ────────────────────────────────────────
            Message::LoadWabbajackCatalog => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.loading = true;
                    state.error = None;
                    state.status = "Loading Wabbajack catalogs...".to_string();
                }
                return Task::perform(
                    async {
                        let client = reqwest::Client::new();
                        modde_sources::wabbajack::catalog::fetch_catalog(
                            &client,
                            modde_sources::wabbajack::catalog::CatalogSource::Both,
                        )
                        .await
                        .map_err(|e| e.to_string())
                    },
                    Message::WabbajackCatalogLoaded,
                );
            }
            Message::WabbajackCatalogLoaded(result) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.loading = false;
                    match result {
                        Ok(entries) => {
                            state.status = format!("Loaded {} Wabbajack entries", entries.len());
                            state.entries = entries;
                            state.error = None;
                        }
                        Err(e) => {
                            state.status = format!("Failed to load catalog: {e}");
                            state.error = Some(e);
                        }
                    }
                }
            }
            Message::WabbajackTabChanged(tab) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.tab = tab;
                    state.selected_index = None;
                }
            }
            Message::WabbajackSearchChanged(value) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.search = value;
                }
            }
            Message::WabbajackGameFilterChanged(value) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.game_filter = value;
                    state.game_filter_user_edited = true;
                }
            }
            Message::WabbajackToggleOfficialOnly(value) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.official_only = value;
                }
            }
            Message::WabbajackToggleNsfw(value) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.include_nsfw = value;
                }
            }
            Message::WabbajackToggleDown(value) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.include_down = value;
                }
            }
            Message::WabbajackSelectEntry(index) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.selected_index = Some(index);
                    if let Some(entry) = state.entries.get(index) {
                        state.manual_source = entry.download_url.clone();
                        state.hm_profile = slugify_profile_name(&entry.title);
                        if let Some(game) = &entry.game {
                            state.hm_game = modde_games::normalize_wabbajack_game(game)
                                .unwrap_or(game)
                                .to_string();
                            state.hm_game_dir_user_edited = false;
                            prefill_wabbajack_game_dir(&self.settings, state);
                        }
                    }
                }
            }
            Message::WabbajackManualSourceChanged(value) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.manual_source = value;
                }
            }
            Message::WabbajackHmProfileChanged(value) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.hm_profile = value;
                }
            }
            Message::WabbajackHmGameChanged(value) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.hm_game = value;
                    prefill_wabbajack_game_dir(&self.settings, state);
                }
            }
            Message::WabbajackHmGameDirChanged(value) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.hm_game_dir = value;
                    state.hm_game_dir_user_edited = true;
                }
            }
            Message::WabbajackDownloadSelected => {
                let source = if let View::WabbajackInstaller(ref state) = self.active_view {
                    state.manual_source.clone()
                } else {
                    String::new()
                };
                if source.is_empty() {
                    self.status_message = "Enter or select a .wabbajack source first".to_string();
                    return Task::none();
                }
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.status = "Downloading .wabbajack file...".to_string();
                }
                return Task::perform(
                    async move { download_wabbajack_source(source).await },
                    Message::WabbajackDownloadComplete,
                );
            }
            Message::WabbajackDownloadComplete(result) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    match result {
                        Ok(path) => {
                            state.downloaded_path = Some(path.clone());
                            state.file_path = Some(path.clone());
                            state.status = format!("Downloaded {}", path.display());
                            state.log_lines.push(state.status.clone());
                            self.wabbajack_manifest =
                                modde_sources::wabbajack::runner::parse_wabbajack_manifest(&path)
                                    .ok();
                            if let Some(manifest) = &self.wabbajack_manifest {
                                state.hm_profile = slugify_profile_name(&manifest.name);
                                state.hm_game =
                                    modde_games::normalize_wabbajack_game(&manifest.game)
                                        .unwrap_or(&manifest.game)
                                        .to_string();
                                state.hm_game_dir_user_edited = false;
                                prefill_wabbajack_game_dir(&self.settings, state);
                            }
                        }
                        Err(e) => {
                            state.status = format!("Download failed: {e}");
                            state.log_lines.push(state.status.clone());
                        }
                    }
                }
            }
            Message::WabbajackGenerateHmSnippet => {
                let (source, profile, game, game_dir) =
                    if let View::WabbajackInstaller(ref state) = self.active_view {
                        (
                            state.manual_source.clone(),
                            state.hm_profile.clone(),
                            state.hm_game.clone(),
                            state.hm_game_dir.clone(),
                        )
                    } else {
                        (String::new(), String::new(), String::new(), String::new())
                    };
                if source.is_empty() || profile.is_empty() || game.is_empty() {
                    self.status_message =
                        "Wabbajack source, HM profile, and game are required".to_string();
                    return Task::none();
                }
                return Task::perform(
                    async move {
                        let client = reqwest::Client::new();
                        let cache_dir = modde_core::paths::downloads_dir().join("wabbajack");
                        let game_dir = (!game_dir.is_empty()).then(|| PathBuf::from(game_dir));
                        modde_sources::wabbajack::catalog::hm_snippet_for_source(
                            &client,
                            &source,
                            &profile,
                            &game,
                            game_dir.as_deref(),
                            &cache_dir,
                        )
                        .await
                        .map(|(snippet, _)| snippet)
                        .map_err(format_anyhow_error)
                    },
                    Message::WabbajackHmSnippetGenerated,
                );
            }
            Message::WabbajackHmSnippetGenerated(result) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    match result {
                        Ok(snippet) => {
                            state.hm_snippet = snippet;
                            state.status = "Generated Home Manager snippet".to_string();
                        }
                        Err(e) => {
                            state.status = format!("HM snippet failed: {e}");
                        }
                    }
                }
            }
            Message::WabbajackCopyHmSnippet => {
                if let View::WabbajackInstaller(ref state) = self.active_view
                    && !state.hm_snippet.is_empty()
                {
                    self.status_message = "Copied Home Manager snippet".to_string();
                    return iced::clipboard::write(state.hm_snippet.clone());
                }
            }
            Message::WabbajackSaveHmSnippet => {
                let snippet = if let View::WabbajackInstaller(ref state) = self.active_view {
                    state.hm_snippet.clone()
                } else {
                    String::new()
                };
                if snippet.is_empty() {
                    self.status_message = "Generate a Home Manager snippet first".to_string();
                    return Task::none();
                }
                return Task::perform(
                    async move {
                        let file = rfd::AsyncFileDialog::new()
                            .set_title("Save Home Manager snippet")
                            .set_file_name("modde-wabbajack.nix")
                            .save_file()
                            .await
                            .map(|h| h.path().to_path_buf());
                        let Some(path) = file else {
                            return Err("Save cancelled".to_string());
                        };
                        tokio::fs::write(&path, snippet)
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(path)
                    },
                    Message::WabbajackHmSnippetSaved,
                );
            }
            Message::WabbajackHmSnippetSaved(result) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    match result {
                        Ok(path) => {
                            state.status = format!("Saved snippet to {}", path.display());
                        }
                        Err(e) => {
                            state.status = e;
                        }
                    }
                }
            }
            Message::WabbajackOpenUrl(url) => {
                if let Err(e) = open::that(&url) {
                    self.status_message = format!("Failed to open {url}: {e}");
                }
            }
            Message::OpenWabbajackFile => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select .wabbajack File")
                            .add_filter("Wabbajack", &["wabbajack"])
                            .pick_file()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    |path| match path {
                        Some(p) => Message::WabbajackFileSelected(p),
                        None => Message::Noop,
                    },
                );
            }
            Message::WabbajackFileSelected(path) => {
                let manifest =
                    modde_sources::wabbajack::runner::parse_wabbajack_manifest(&path).ok();
                self.wabbajack_manifest = manifest;
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.file_path = Some(path.clone());
                    state.downloaded_path = Some(path.clone());
                    state.manual_source = path.display().to_string();
                    state.progress = 0.0;
                    state.status = format!("Selected: {}", path.display());
                    state
                        .log_lines
                        .push(format!("File selected: {}", path.display()));
                    if let Some(manifest) = &self.wabbajack_manifest {
                        state.hm_profile = slugify_profile_name(&manifest.name);
                        state.hm_game = modde_games::normalize_wabbajack_game(&manifest.game)
                            .unwrap_or(&manifest.game)
                            .to_string();
                        state.hm_game_dir_user_edited = false;
                        prefill_wabbajack_game_dir(&self.settings, state);
                    }
                }
                self.status_message = format!("Wabbajack file loaded: {}", path.display());
            }
            Message::WabbajackProgress(progress) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.progress = progress;
                    state.status = format!("{:.0}% complete", progress * 100.0);
                }
            }
            Message::WabbajackStartInstall => {
                let current_game_dir = self.current_game_dir();
                let (path, profile_name, game_dir) =
                    if let View::WabbajackInstaller(ref mut state) = self.active_view {
                        let Some(path) = state.file_path.clone() else {
                            self.status_message = "No wabbajack file selected".to_string();
                            return Task::none();
                        };
                        state.status = "Starting installation...".to_string();
                        state.log_lines.push("Installation started".to_string());
                        state.progress = 0.0;
                        (
                            path,
                            self.active_profile.clone().or_else(|| {
                                (!state.hm_profile.is_empty()).then(|| state.hm_profile.clone())
                            }),
                            current_game_dir,
                        )
                    } else {
                        self.status_message = "No wabbajack file selected".to_string();
                        return Task::none();
                    };
                return Task::perform(
                    async move { run_wabbajack_install_for_ui(path, profile_name, game_dir).await },
                    Message::WabbajackInstallComplete,
                );
            }
            Message::WabbajackInstallComplete(result) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    match result {
                        Ok((summary, lines)) => {
                            state.progress = 1.0;
                            state.status = summary.clone();
                            state.log_lines.extend(lines);
                            self.status_message = summary;
                            self.reload_profile();
                        }
                        Err(e) => {
                            state.status = format!("Install failed: {e}");
                            state.log_lines.push(state.status.clone());
                            self.status_message = state.status.clone();
                        }
                    }
                }
            }
            Message::WabbajackLog(line) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.log_lines.push(line.clone());
                    state.status = line;
                }
            }

            // ── FOMOD ────────────────────────────────────────────
            Message::StartFOMOD {
                mod_path,
                dest_path,
            } => {
                let config_path = mod_path.join("fomod").join("ModuleConfig.xml");
                let xml = match std::fs::read_to_string(&config_path) {
                    Ok(xml) => xml,
                    Err(e) => {
                        self.status_message = format!("Failed to read ModuleConfig.xml: {e}");
                        return Task::none();
                    }
                };
                let config = match fomod_oxide::ModuleConfig::parse(&xml) {
                    Ok(c) => c,
                    Err(e) => {
                        self.status_message = format!("Failed to parse ModuleConfig.xml: {e}");
                        return Task::none();
                    }
                };
                let installer = fomod_oxide::Installer::new(config);
                let mut state = FOMODWizardState::with_installer(installer);
                let defaults = state.default_selections();
                self.fomod_selections.clear();
                for (step_idx, group_idx, sel) in defaults {
                    state.select(step_idx, group_idx, sel.clone());
                    self.fomod_selections.insert((step_idx, group_idx), sel);
                }
                self.fomod_installer = Some(state);
                self.fomod_source_dir = Some(mod_path);
                self.fomod_dest_dir = Some(dest_path);
                self.fomod_wizard_pos = 0;
                self.fomod_can_undo = false;
                self.refresh_fomod_visible_steps();
                self.refresh_fomod_conflicts();
                self.active_view = View::FOMODWizard(FOMODWizardState::new());
                self.status_message = "FOMOD wizard started".to_string();
            }
            Message::FOMODChoice {
                step,
                group,
                option,
                selected,
            } => {
                if let Some(ref mut installer) = self.fomod_installer {
                    installer.checkpoint();
                    self.fomod_can_undo = true;
                    let group_type = installer.group_type_at(step, group);
                    let entry = self.fomod_selections.entry((step, group)).or_default();
                    match group_type {
                        Some(
                            fomod_oxide::config::GroupType::SelectExactlyOne
                            | fomod_oxide::config::GroupType::SelectAtMostOne,
                        ) => {
                            if selected {
                                *entry = vec![option];
                            } else {
                                entry.retain(|&o| o != option);
                            }
                        }
                        Some(fomod_oxide::config::GroupType::SelectAll) => {}
                        _ => {
                            if selected {
                                if !entry.contains(&option) {
                                    entry.push(option);
                                }
                            } else {
                                entry.retain(|&o| o != option);
                            }
                        }
                    }
                    let current_sel = entry.clone();
                    installer.select(step, group, current_sel);
                    self.refresh_fomod_visible_steps();
                    self.refresh_fomod_conflicts();
                }
            }
            Message::FOMODNext => {
                if self.fomod_is_last_step() {
                    let result = (|| -> Result<(), String> {
                        let installer = self
                            .fomod_installer
                            .as_ref()
                            .ok_or("No active FOMOD installer")?;
                        let source = self
                            .fomod_source_dir
                            .as_ref()
                            .ok_or("No source directory")?;
                        let dest = self
                            .fomod_dest_dir
                            .as_ref()
                            .ok_or("No destination directory")?;
                        let plan = installer.resolve();
                        plan.execute(source, dest).map_err(|e| e.to_string())?;
                        Ok(())
                    })();
                    match &result {
                        Ok(()) => {
                            self.status_message =
                                "FOMOD installation completed successfully".to_string();
                        }
                        Err(e) => self.status_message = format!("FOMOD installation failed: {e}"),
                    }
                    self.reset_fomod();
                    self.active_view = View::ModList;
                    return Task::done(Message::FOMODInstallComplete(result));
                }
                if let Some(ref mut installer) = self.fomod_installer {
                    installer.checkpoint();
                    self.fomod_can_undo = true;
                }
                self.fomod_wizard_pos += 1;
            }
            Message::FOMODBack => {
                if self.fomod_wizard_pos > 0 {
                    self.fomod_wizard_pos -= 1;
                }
            }
            Message::FOMODCancel => {
                self.reset_fomod();
                self.active_view = View::ModList;
                self.status_message = "FOMOD installation cancelled".to_string();
            }
            Message::FOMODUndo => {
                let rolled_back = self
                    .fomod_installer
                    .as_mut()
                    .is_some_and(FOMODWizardState::rollback);
                if rolled_back {
                    if let Some(ref installer) = self.fomod_installer {
                        self.fomod_selections = installer.selections();
                        self.fomod_can_undo = installer.history_len() > 0;
                    }
                    self.refresh_fomod_visible_steps();
                    self.refresh_fomod_conflicts();
                    self.status_message = "Undid last FOMOD selection".to_string();
                }
            }
            Message::FOMODInstallComplete(result) => match result {
                Ok(()) => self.status_message = "FOMOD installation complete!".to_string(),
                Err(e) => self.status_message = format!("FOMOD installation failed: {e}"),
            },

            // ── Downloads ────────────────────────────────────────
            Message::DownloadProgress { id, bytes, total } => {
                let task_id = self.track_download(&id, &id);
                if let Some(task) = self.download_queue.get_mut(task_id) {
                    task.meta.bytes_downloaded = bytes;
                    task.meta.total_bytes = Some(total);
                    task.meta.status = "downloading".to_string();
                    task.state = modde_sources::queue::DownloadState::Active {
                        bytes_downloaded: bytes,
                        total_bytes: Some(total),
                    };
                }
                let pct = if total > 0 {
                    (bytes as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                self.status_message = format!("Downloading {id}: {pct:.0}%");
            }
            Message::DownloadComplete { id } => {
                if let Some(task_id) = self.download_lookup.get(&id).copied()
                    && let Some(task) = self.download_queue.get_mut(task_id)
                {
                    task.meta.status = "complete".to_string();
                    task.state = modde_sources::queue::DownloadState::Complete {
                        path: task.dest.clone(),
                        hash: task.expected_hash.unwrap_or(0),
                    };
                }
                self.status_message = format!("Download complete: {id}");
            }
            Message::DownloadFailed { id, error } => {
                if let Some(task_id) = self.download_lookup.get(&id).copied()
                    && let Some(task) = self.download_queue.get_mut(task_id)
                {
                    task.meta.status = "failed".to_string();
                    task.state = modde_sources::queue::DownloadState::Failed {
                        error: error.clone(),
                    };
                }
                self.status_message = format!("Download failed ({id}): {error}");
            }

            // ── Settings ─────────────────────────────────────────
            Message::SetNexusApiKeyDraft(key) => {
                self.nexus_api_key_draft = key;
                self.nexus_status = None;
            }
            Message::ToggleNexusApiKeyVisibility => {
                self.nexus_api_key_visible = !self.nexus_api_key_visible;
            }
            Message::ReplaceNexusApiKey => {
                match modde_sources::nexus::auth::write_config_api_key(&self.nexus_api_key_draft) {
                    Ok(()) => {
                        self.refresh_nexus_api_key_state();
                        self.status_message = "Nexus API key saved to modde config".to_string();
                        self.nexus_status = None;
                    }
                    Err(e) => {
                        self.status_message = format!("Nexus key not saved: {e}");
                        self.nexus_status = Some(NexusAuthStatus::Invalid(e.to_string()));
                    }
                }
            }
            Message::RemoveNexusConfigKey => {
                match modde_sources::nexus::auth::delete_config_api_key() {
                    Ok(()) => {
                        self.refresh_nexus_api_key_state();
                        self.status_message = "Removed modde Nexus API key config".to_string();
                        self.nexus_status = None;
                    }
                    Err(e) => {
                        self.status_message = format!("Failed to remove modde Nexus key: {e}");
                        self.nexus_status = Some(NexusAuthStatus::Invalid(e.to_string()));
                    }
                }
            }
            Message::SetGamePath { game_id, path } => {
                let path_exists = path.is_dir();
                self.settings
                    .set_game_path(&GameId::from(game_id.as_str()), path);
                if path_exists {
                    self.detected_games.insert(game_id.clone());
                } else {
                    self.detected_games.remove(&game_id);
                }
                self.status_message = format!("Game path set for {game_id}");
                self.save_settings();
            }
            Message::SetDownloadDir(path) => {
                self.status_message = format!("Download directory set to {}", path.display());
                self.settings.download_dir = Some(path);
                self.save_settings();
            }
            Message::BrowseGamePath => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select Game Directory")
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    |path| match path {
                        Some(p) => Message::SetGamePath {
                            game_id: "default".to_string(),
                            path: p,
                        },
                        None => Message::Noop,
                    },
                );
            }
            Message::BrowseDownloadDir => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select Download Directory")
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    |path| match path {
                        Some(p) => Message::SetDownloadDir(p),
                        None => Message::Noop,
                    },
                );
            }
            Message::SetTheme(name) => {
                self.theme_name = name.clone();
                self.settings.theme = name;
                self.status_message = "Theme updated".to_string();
                self.save_settings();
            }
            Message::ValidateNexusKey => {
                self.nexus_status = Some(NexusAuthStatus::Checking);
                let api_key = self.nexus_api_key_draft.trim().to_string();
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || -> Result<(String, bool), String> {
                            if api_key.is_empty() {
                                return Err("No API key set".to_string());
                            }
                            let client = reqwest::blocking::Client::new();
                            let resp = client
                                .get("https://api.nexusmods.com/v1/users/validate.json")
                                .header("apikey", &api_key)
                                .send()
                                .map_err(|e| e.to_string())?;
                            if !resp.status().is_success() {
                                return Err(format!("HTTP {}", resp.status()));
                            }
                            let body: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
                            let name = body["name"].as_str().unwrap_or("Unknown").to_string();
                            let is_premium = body["is_premium"].as_bool().unwrap_or(false);
                            Ok((name, is_premium))
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    Message::NexusKeyValidated,
                );
            }
            Message::NexusKeyValidated(result) => match result {
                Ok((username, is_premium)) => {
                    self.nexus_status = Some(NexusAuthStatus::Valid {
                        username: username.clone(),
                        is_premium,
                    });
                    self.status_message = format!("Nexus: logged in as {username}");
                }
                Err(e) => {
                    self.nexus_status = Some(NexusAuthStatus::Invalid(e.clone()));
                    self.status_message = format!("Nexus key invalid: {e}");
                }
            },

            // ── Stock game ───────────────────────────────────────
            Message::CreateStockSnapshot => {
                self.status_message = "Creating stock game snapshot...".to_string();
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || -> Result<String, String> {
                                let game_plugin =
                                    modde_games::resolve_game_plugin(game_id.as_str())
                                        .ok_or_else(|| format!("unsupported game: {game_id}"))?;
                                let install_path =
                                    game_plugin.detect_install().ok_or_else(|| {
                                        format!("could not detect install for {game_id}")
                                    })?;
                                let mgr = modde_core::stock::StockGameManager::new(
                                    modde_core::stock::StockGameManager::default_dir(),
                                );
                                let rt = tokio::runtime::Handle::current();
                                rt.block_on(mgr.snapshot(&game_id, &install_path))
                                    .map_err(|e| e.to_string())?;
                                Ok(format!("Snapshot created for {game_id}"))
                            })
                            .await
                            .map_err(|e| e.to_string())?
                        },
                        Message::StockSnapshotCreated,
                    );
                }
                self.status_message = "No active profile".to_string();
            }
            Message::StockSnapshotCreated(result) => match result {
                Ok(msg) => {
                    self.stock_snapshot_exists = true;
                    self.status_message = msg;
                }
                Err(e) => self.status_message = format!("Snapshot failed: {e}"),
            },
            Message::VerifyStockSnapshot => {
                self.status_message = "Verifying stock snapshot...".to_string();
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || -> Result<String, String> {
                                let game_plugin =
                                    modde_games::resolve_game_plugin(game_id.as_str())
                                        .ok_or_else(|| format!("unsupported game: {game_id}"))?;
                                let _install_path =
                                    game_plugin.detect_install().ok_or_else(|| {
                                        format!("could not detect install for {game_id}")
                                    })?;
                                let mgr = modde_core::stock::StockGameManager::new(
                                    modde_core::stock::StockGameManager::default_dir(),
                                );
                                let rt = tokio::runtime::Handle::current();
                                match rt.block_on(mgr.verify(&game_id)) {
                                    Ok(true) => Ok("Stock snapshot verified: OK".to_string()),
                                    Ok(false) => Ok("Stock snapshot MODIFIED".to_string()),
                                    Err(e) => Err(e.to_string()),
                                }
                            })
                            .await
                            .map_err(|e| e.to_string())?
                        },
                        Message::StockVerifyResult,
                    );
                }
            }
            Message::StockVerifyResult(result) => match result {
                Ok(msg) => self.status_message = msg,
                Err(e) => self.status_message = format!("Verify failed: {e}"),
            },

            // ── Experiments ──────────────────────────────────────
            Message::TryProfile => {
                if let (Some(profile), Some(profile_name)) =
                    (&self.loaded_profile, &self.active_profile)
                {
                    let game_id = profile.game_id.clone();
                    let name = profile_name.clone();
                    match ProfileManager::open() {
                        Ok(pm) => {
                            let save_dir = Self::resolve_save_dir(game_id.as_str());
                            match pm.try_profile(&name, &game_id, save_dir.as_deref()) {
                                Ok(()) => {
                                    self.experiment_depth += 1;
                                    self.status_message = format!(
                                        "Experiment started (depth {})",
                                        self.experiment_depth
                                    );
                                }
                                Err(e) => self.status_message = format!("Try failed: {e}"),
                            }
                        }
                        Err(e) => self.status_message = format!("Error: {e}"),
                    }
                }
            }
            Message::RollbackExperiment => {
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    match ProfileManager::open() {
                        Ok(pm) => {
                            let save_dir = Self::resolve_save_dir(game_id.as_str());
                            match pm.rollback(&game_id, save_dir.as_deref()) {
                                Ok(prev_name) => {
                                    self.active_profile = Some(prev_name.clone());
                                    self.reload_profile();
                                    self.status_message = format!("Rolled back to '{prev_name}'");
                                }
                                Err(e) => self.status_message = format!("Rollback failed: {e}"),
                            }
                        }
                        Err(e) => self.status_message = format!("Error: {e}"),
                    }
                }
            }
            Message::CommitExperiment => {
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    match ProfileManager::open() {
                        Ok(pm) => match pm.commit(&game_id) {
                            Ok(()) => {
                                self.experiment_depth = 0;
                                self.status_message = "Experiment committed".to_string();
                            }
                            Err(e) => self.status_message = format!("Commit failed: {e}"),
                        },
                        Err(e) => self.status_message = format!("Error: {e}"),
                    }
                }
            }

            // ── Saves ────────────────────────────────────────────
            Message::LoadSaveHistory => {
                self.selected_save_details = None;
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    if !Self::game_supports_save_profiles(game_id.as_str()) {
                        self.save_snapshots.clear();
                        self.current_fingerprint = None;
                        self.status_message =
                            "Save profiles are not supported for this game".to_string();
                        return Task::none();
                    }
                    let profile_name = profile.name.clone();
                    match modde_core::save::SaveManager::history(&game_id, &profile_name, 20) {
                        Ok(history) => self.save_snapshots = history,
                        Err(e) => {
                            self.save_snapshots = Vec::new();
                            self.status_message = format!("Could not load save history: {e}");
                        }
                    }
                }
            }
            Message::SelectSaveSnapshot(commit_id) => {
                if let Some(snap) = self.save_snapshots.iter().find(|s| s.id == commit_id) {
                    let compat = snap
                        .fingerprint
                        .as_ref()
                        .zip(self.current_fingerprint.as_ref())
                        .map(|(_, current)| snap.check_compatibility(current));

                    let mut details =
                        crate::views::save_details::SaveDetailsState::from_snapshot(snap, compat);

                    // Load file list synchronously (fast git tree walk)
                    if let Some(ref profile) = self.loaded_profile {
                        match modde_core::save::SaveManager::snapshot_file_list(
                            &profile.game_id,
                            &commit_id,
                        ) {
                            Ok(files) => details.file_paths = Some(files),
                            Err(_) => details.file_paths = Some(Vec::new()),
                        }
                    }

                    self.selected_save_details = Some(details);
                }
            }
            Message::RestoreSaveSnapshot(commit_id) => {
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    let profile_name = profile.name.clone();
                    let save_dir = Self::resolve_save_dir(game_id.as_str());
                    if let Some(save_dir) = save_dir {
                        match modde_core::save::SaveManager::restore(
                            &game_id,
                            &profile_name,
                            &commit_id,
                            &save_dir,
                        ) {
                            Ok(count) => {
                                self.status_message = format!("Restored {count} save file(s)");
                            }
                            Err(e) => self.status_message = format!("Restore failed: {e}"),
                        }
                    } else if Self::game_supports_save_profiles(game_id.as_str()) {
                        self.status_message =
                            "Cannot detect save directory for this game".to_string();
                    } else {
                        self.status_message =
                            "Save profiles are not supported for this game".to_string();
                    }
                }
            }

            Message::DataTabFilterChanged(f) => {
                self.data_tab_state.filter = f;
            }
            Message::DataTabToggleConflicts(v) => {
                self.data_tab_state.show_conflicts_only = v;
            }
            Message::RunDiagnostics => {
                self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Running;
                self.status_message = "Running diagnostics...".to_string();
                self.run_diagnostics_now();
            }
            Message::LoadTools => {
                self.status_message = "Loading tools...".to_string();
                return self.start_tools_load();
            }
            Message::RefreshTools => {
                self.status_message = "Loading tools...".to_string();
                return self.start_tools_load();
            }
            Message::LoadExecutables => {
                self.status_message = "Loading executables...".to_string();
                return self.start_executables_load();
            }
            Message::RefreshExecutables => {
                self.status_message = "Loading executables...".to_string();
                return self.start_executables_load();
            }
            Message::ToolsLoaded { generation, result } => {
                if generation != self.tool_state.load_generation {
                    return Task::none();
                }
                self.tool_state.loading = false;
                match result {
                    Ok(snapshot) => {
                        let count = snapshot.entries.len();
                        self.apply_tool_snapshot(snapshot);
                        self.status_message =
                            if let Some(status) = self.pending_tools_load_status_message.take() {
                                status
                            } else if count == 0 {
                                "No tool state available for the current game".to_string()
                            } else {
                                format!("Loaded {count} tool(s)")
                            };
                    }
                    Err(err) => {
                        self.pending_tools_load_status_message = None;
                        self.tool_state.load_error = Some(err.clone());
                        self.status_message = format!("Failed to load tools: {err}");
                    }
                }
            }
            Message::ExecutablesLoaded { generation, result } => {
                if generation != self.tool_state.executables_load_generation {
                    return Task::none();
                }
                self.tool_state.executables_loading = false;
                match result {
                    Ok(executables) => {
                        let count = executables.len();
                        self.tool_state.executables = executables;
                        self.tool_state.executables_load_error = None;
                        self.status_message = format!("Loaded {count} executable(s)");
                    }
                    Err(err) => {
                        self.tool_state.executables_load_error = Some(err.clone());
                        self.status_message = format!("Failed to load executables: {err}");
                    }
                }
            }
            Message::RefreshOptiScalerReleases => {
                self.tool_state.optiscaler_releases_loading = true;
                self.status_message = "Loading OptiScaler releases...".to_string();
                return Task::perform(
                    load_tool_releases("optiscaler".to_string()),
                    Message::OptiScalerReleasesLoaded,
                );
            }
            Message::OptiScalerReleasesLoaded(result) => {
                self.tool_state.optiscaler_releases_loading = false;
                match result {
                    Ok(releases) => {
                        let Some(game_id) = self.current_game_id().map(str::to_string) else {
                            self.status_message =
                                "Select a game before loading OptiScaler releases".to_string();
                            return Task::none();
                        };
                        let Ok(mut config) = current_tool_config(&game_id, "optiscaler") else {
                            self.status_message =
                                "Failed to load OptiScaler configuration".to_string();
                            return Task::none();
                        };
                        let selected = sync_optiscaler_release_options(
                            &mut self.tool_state.tool_option_catalog,
                            &releases,
                            &mut config,
                        );
                        if let Some((tag, asset)) = selected {
                            config.set("release_tag", serde_json::json!(tag));
                            config.set("release_asset", serde_json::json!(asset));
                            let _ = save_tool_settings(&game_id, "optiscaler", &config);
                        }
                        self.tool_state.optiscaler_releases = releases;
                        self.tool_state.active_tool_id = Some("optiscaler".to_string());
                        self.status_message = format!(
                            "Loaded {} OptiScaler release(s)",
                            self.tool_state.optiscaler_releases.len()
                        );
                        return self.start_tools_load();
                    }
                    Err(err) => {
                        self.status_message = format!("Failed to load OptiScaler releases: {err}");
                    }
                }
            }
            Message::InstallOptiScalerRelease => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.status_message = "Select a game before installing OptiScaler".to_string();
                    return Task::none();
                };
                self.status_message = "Installing OptiScaler release...".to_string();
                return Task::perform(
                    install_selected_tool_release(game_id, "optiscaler".to_string()),
                    Message::OptiScalerReleaseInstalled,
                );
            }
            Message::OptiScalerReleaseInstalled(result) => match result {
                Ok(message) => {
                    self.tool_state.active_tool_id = Some("optiscaler".to_string());
                    self.status_message = message;
                    return self.start_tools_load();
                }
                Err(err) => {
                    self.status_message = format!("Failed to install OptiScaler: {err}");
                }
            },
            Message::RefreshProtonVersions => {
                self.tool_state.proton_versions_loading = true;
                self.status_message = "Loading Proton versions...".to_string();
                return Task::perform(load_proton_versions(), Message::ProtonVersionsLoaded);
            }
            Message::ProtonVersionsLoaded(result) => {
                self.tool_state.proton_versions_loading = false;
                match result {
                    Ok(versions) => {
                        let Some(game_id) = self.current_game_id().map(str::to_string) else {
                            self.status_message =
                                "Select a game before loading Proton versions".to_string();
                            return Task::none();
                        };
                        let versions = if versions.is_empty() {
                            modde_games::tools::proton::proton_version_options()
                        } else {
                            versions
                        };
                        set_tool_options(
                            &mut self.tool_state.tool_option_catalog,
                            "proton",
                            "selected_version",
                            versions.clone(),
                        );
                        set_tool_options(
                            &mut self.tool_state.tool_option_catalog,
                            "proton",
                            "_catalog_loaded",
                            vec!["true".to_string()],
                        );
                        if let Ok(mut config) = current_tool_config(&game_id, "proton") {
                            let selected = config.get_str("selected_version").unwrap_or("latest");
                            if !versions.iter().any(|version| version == selected) {
                                config.set("selected_version", serde_json::json!("latest"));
                                let _ = save_tool_settings(&game_id, "proton", &config);
                            }
                        }
                        self.tool_state.active_tool_id = Some("proton".to_string());
                        self.status_message =
                            format!("Loaded {} Proton version option(s)", versions.len());
                        return self.start_tools_load();
                    }
                    Err(err) => {
                        self.status_message = format!("Failed to load Proton versions: {err}");
                    }
                }
            }
            Message::InstallProtonVersion => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.status_message = "Select a game before installing Proton".to_string();
                    return Task::none();
                };
                self.status_message = "Installing Proton with protonup-rs...".to_string();
                return Task::perform(
                    install_selected_proton_version(game_id),
                    Message::ProtonVersionInstalled,
                );
            }
            Message::ProtonVersionInstalled(result) => match result {
                Ok(message) => {
                    self.tool_state.active_tool_id = Some("proton".to_string());
                    self.status_message = message;
                    return self.start_tools_load();
                }
                Err(err) => {
                    self.status_message = format!("Failed to install Proton: {err}");
                }
            },
            Message::SelectToolTab(tool_id) => {
                let should_load_optiscaler = tool_id == "optiscaler"
                    && self.tool_state.optiscaler_releases.is_empty()
                    && !self.tool_state.optiscaler_releases_loading;
                let should_load_proton = tool_id == "proton"
                    && !self.tool_state.proton_versions_loading
                    && tool_options(
                        &self.tool_state.tool_option_catalog,
                        "proton",
                        "_catalog_loaded",
                    )
                    .is_none();
                self.tool_state.active_tool_id = Some(tool_id);
                if should_load_optiscaler {
                    self.tool_state.optiscaler_releases_loading = true;
                    self.status_message = "Loading OptiScaler releases...".to_string();
                    return Task::perform(
                        load_tool_releases("optiscaler".to_string()),
                        Message::OptiScalerReleasesLoaded,
                    );
                }
                if should_load_proton {
                    self.tool_state.proton_versions_loading = true;
                    self.status_message = "Loading Proton versions...".to_string();
                    return Task::perform(load_proton_versions(), Message::ProtonVersionsLoaded);
                }
            }
            Message::UpdateToolSetting {
                tool_id,
                key,
                value,
            } => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.status_message = "Select a game before configuring tools".to_string();
                    return Task::none();
                };
                let Some(tool) = modde_games::tools::resolve_tool(&tool_id) else {
                    self.status_message = format!("Unknown tool: {tool_id}");
                    return Task::none();
                };
                let context = self.current_tool_game_context();
                match modde_core::db::ModdeDb::open() {
                    Ok(db) => {
                        let typed_game_id = GameId::from(game_id.as_str());
                        let mut config = db
                            .load_tool_config(&typed_game_id, &tool_id)
                            .ok()
                            .flatten()
                            .map_or_else(
                                || tool.default_config_for(context.as_ref()),
                                |row| modde_games::tools::ToolConfig {
                                    tool_id: row.tool_id,
                                    enabled: row.enabled,
                                    settings: serde_json::from_str(&row.settings_json)
                                        .unwrap_or_default(),
                                },
                            );
                        if tool_id == "optiscaler" {
                            let _ =
                                modde_games::tools::optiscaler::normalize_optiscaler_release_config(
                                    &mut config,
                                );
                        }
                        let setting_specs = tool.settings_schema_for(context.as_ref(), &config);
                        let normalized =
                            if let Some(spec) = setting_specs.iter().find(|spec| spec.key == key) {
                                normalize_tool_setting_value(
                                    &config.settings,
                                    &key,
                                    normalize_tool_setting_for_kind(value, &spec.kind),
                                )
                            } else {
                                normalize_tool_setting_value(&config.settings, &key, value)
                            };
                        set_nested_tool_setting(&mut config.settings, &key, normalized);
                        if tool_id == "optiscaler"
                            && key == "optiscaler_profile"
                            && let Some(profile_id) =
                                config.get_str("optiscaler_profile").map(str::to_string)
                        {
                            modde_games::tools::optiscaler::apply_profile_by_id(
                                &mut config,
                                &game_id,
                                &profile_id,
                            );
                        }
                        if tool_id == "optiscaler"
                            && key == "release_tag"
                            && let Some(tag) = config.get_str("release_tag").map(str::to_string)
                        {
                            if let Some(channel) =
                                modde_games::tools::optiscaler::optiscaler_goverlay_channel_for_tag(
                                    &tag,
                                )
                            {
                                config.set("source_mode", serde_json::json!("goverlay_builds"));
                                config.set("goverlay_channel", serde_json::json!(channel));
                            } else {
                                config.set("source_mode", serde_json::json!("github_release"));
                            }
                        }
                        if tool_id == "optiscaler"
                            && matches!(
                                key.as_str(),
                                "source_mode" | "goverlay_channel" | "release_tag"
                            )
                        {
                            if key == "source_mode" || key == "goverlay_channel" {
                                if config.get_str("source_mode") == Some("goverlay_builds")
                                    && config.get_str("goverlay_channel").is_none()
                                {
                                    config.set("goverlay_channel", serde_json::json!("edge"));
                                }
                                config.set("release_tag", serde_json::json!(""));
                                config.set("release_asset", serde_json::json!(""));
                            }
                            sync_optiscaler_release_options(
                                &mut self.tool_state.tool_option_catalog,
                                &self.tool_state.optiscaler_releases,
                                &mut config,
                            );
                        }
                        let settings_json = serde_json::to_string(&config.settings)
                            .unwrap_or_else(|_| "{}".to_string());
                        match db.save_tool_config_with_reason(
                            &typed_game_id,
                            &tool_id,
                            config.enabled,
                            &settings_json,
                            &format!("ui:set:{key}"),
                        ) {
                            Ok(()) => {
                                if config.enabled {
                                    let _ = modde_games::launcher::generate_tool_configs(
                                        &typed_game_id,
                                        &db,
                                    );
                                }
                                self.tool_state.active_tool_id = Some(tool_id);
                                self.status_message =
                                    format!("Updated {} setting", tool.display_name());
                                return self.start_tools_load();
                            }
                            Err(err) => {
                                self.status_message =
                                    format!("Failed to update tool setting: {err}");
                            }
                        }
                    }
                    Err(err) => {
                        self.status_message = format!("Failed to open tool database: {err}");
                    }
                }
            }
            Message::ToggleTool { tool_id, enabled } => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.status_message = "Select a game before toggling tools".to_string();
                    return Task::none();
                };
                let Some(tool) = modde_games::tools::resolve_tool(&tool_id) else {
                    self.status_message = format!("Unknown tool: {tool_id}");
                    return Task::none();
                };
                let context = self.current_tool_game_context();
                match modde_core::db::ModdeDb::open() {
                    Ok(db) => {
                        let typed_game_id = GameId::from(game_id.as_str());
                        let settings_json = db
                            .load_tool_config(&typed_game_id, &tool_id)
                            .ok()
                            .flatten()
                            .map_or_else(
                                || {
                                    serde_json::to_string(
                                        &tool.default_config_for(context.as_ref()).settings,
                                    )
                                    .unwrap_or_else(|_| "{}".to_string())
                                },
                                |row| row.settings_json,
                            );
                        match db.save_tool_config_with_reason(
                            &typed_game_id,
                            &tool_id,
                            enabled,
                            &settings_json,
                            if enabled { "ui:enable" } else { "ui:disable" },
                        ) {
                            Ok(()) => {
                                let _ = modde_games::launcher::generate_tool_configs(
                                    &typed_game_id,
                                    &db,
                                );
                                self.status_message = format!(
                                    "{} {}",
                                    tool.display_name(),
                                    if enabled { "enabled" } else { "disabled" }
                                );
                                return self.start_tools_load();
                            }
                            Err(err) => {
                                self.status_message = format!("Failed to update tool state: {err}");
                            }
                        }
                    }
                    Err(err) => {
                        self.status_message = format!("Failed to open tool database: {err}");
                    }
                }
            }
            Message::ToggleToolAdvancedSettings => {
                self.tool_state.show_advanced_settings = !self.tool_state.show_advanced_settings;
            }
            Message::ActivateOptiScaler => {
                let id = "optiscaler".to_string();
                if self.tool_state.is_tool_busy(&id) {
                    self.status_message = "OptiScaler operation already in progress".to_string();
                    return Task::none();
                }
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.status_message = "Select a game before activating OptiScaler".to_string();
                    return Task::none();
                };
                let Some(game_dir) = self.current_game_dir() else {
                    self.status_message = "Game install path is not configured".to_string();
                    return Task::none();
                };
                let context = self.current_tool_game_context();
                self.tool_state.active_operations.insert(id.clone());
                self.status_message = "Activating OptiScaler...".to_string();
                return Task::perform(
                    apply_tool_for_game(game_id, game_dir, id.clone(), context),
                    move |result| Message::ToolApplied {
                        tool_id: id.clone(),
                        result,
                    },
                );
            }
            Message::DeactivateOptiScaler => {
                let id = "optiscaler".to_string();
                if self.tool_state.is_tool_busy(&id) {
                    self.status_message = "OptiScaler operation already in progress".to_string();
                    return Task::none();
                }
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.status_message =
                        "Select a game before deactivating OptiScaler".to_string();
                    return Task::none();
                };
                let Some(game_dir) = self.current_game_dir() else {
                    self.status_message = "Game install path is not configured".to_string();
                    return Task::none();
                };
                self.tool_state.active_operations.insert(id.clone());
                self.status_message = "Deactivating OptiScaler...".to_string();
                return Task::perform(
                    deactivate_optiscaler_for_game(game_id, game_dir),
                    move |result| Message::ToolReverted {
                        tool_id: id.clone(),
                        result,
                    },
                );
            }
            Message::RestoreToolSettings { tool_id, node_id } => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.status_message =
                        "Select a game before restoring tool settings".to_string();
                    return Task::none();
                };
                self.status_message = "Restoring tool settings...".to_string();
                return Task::perform(
                    restore_tool_settings_for_game(game_id, tool_id.clone(), node_id),
                    move |result| Message::ToolSettingsRestored {
                        tool_id: tool_id.clone(),
                        result,
                    },
                );
            }
            Message::ToolSettingsRestored { tool_id, result } => match result {
                Ok(message) => {
                    self.tool_state.active_tool_id = Some(tool_id);
                    self.status_message = message;
                    if self.tool_load_request().is_some() {
                        return self.start_tools_load();
                    }
                }
                Err(err) => {
                    self.status_message = format!("Failed to restore tool settings: {err}");
                }
            },
            Message::ApplyTool(id) => {
                if self.tool_state.is_tool_busy(&id) {
                    self.status_message = format!("{id} operation already in progress");
                    return Task::none();
                }
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.status_message = "Select a game before applying tools".to_string();
                    return Task::none();
                };
                let Some(game_dir) = self.current_game_dir() else {
                    self.status_message = "Game install path is not configured".to_string();
                    return Task::none();
                };
                let Some(tool) = modde_games::tools::resolve_tool(&id) else {
                    self.status_message = format!("Unknown tool: {id}");
                    return Task::none();
                };
                let context = self.current_tool_game_context();
                self.tool_state.active_operations.insert(id.clone());
                self.status_message = format!("Applying {}...", tool.display_name());
                return Task::perform(
                    apply_tool_for_game(game_id, game_dir, id.clone(), context),
                    move |result| Message::ToolApplied {
                        tool_id: id.clone(),
                        result,
                    },
                );
            }
            Message::RevertTool(id) => {
                if self.tool_state.is_tool_busy(&id) {
                    self.status_message = format!("{id} operation already in progress");
                    return Task::none();
                }
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.status_message = "Select a game before reverting tools".to_string();
                    return Task::none();
                };
                let Some(game_dir) = self.current_game_dir() else {
                    self.status_message = "Game install path is not configured".to_string();
                    return Task::none();
                };
                let Some(tool) = modde_games::tools::resolve_tool(&id) else {
                    self.status_message = format!("Unknown tool: {id}");
                    return Task::none();
                };
                self.tool_state.active_operations.insert(id.clone());
                self.status_message = format!("Reverting {}...", tool.display_name());
                return Task::perform(
                    revert_tool_for_game(game_id, game_dir, id.clone()),
                    move |result| Message::ToolReverted {
                        tool_id: id.clone(),
                        result,
                    },
                );
            }
            Message::ToolApplied { tool_id, result } => {
                self.tool_state.active_operations.remove(&tool_id);
                match result {
                    Ok(result) => {
                        self.tool_state.active_tool_id = Some(tool_id);
                        let validation = result
                            .validation_message
                            .map(|message| format!("; {message}"))
                            .unwrap_or_default();
                        self.status_message = format!(
                            "Applied {} ({} file(s)){validation}",
                            result.display_name, result.applied_file_count
                        );
                        if self.tool_load_request().is_some() {
                            return self.start_tools_load();
                        }
                    }
                    Err(err) => {
                        self.status_message = format!("Failed to apply tool: {err}");
                    }
                }
            }
            Message::ToolReverted { tool_id, result } => {
                self.tool_state.active_operations.remove(&tool_id);
                match result {
                    Ok(result) => {
                        self.tool_state.active_tool_id = Some(tool_id);
                        self.status_message = format!("Reverted {}", result.display_name);
                        if self.tool_load_request().is_some() {
                            return self.start_tools_load();
                        }
                    }
                    Err(err) => {
                        self.status_message = format!("Failed to revert tool: {err}");
                    }
                }
            }
            Message::UpdateExecutableDraft { field, value } => {
                self.tool_state.executable_editor_open = true;
                match field {
                    ExecutableDraftField::Name => self.tool_state.executable_draft.name = value,
                    ExecutableDraftField::Path => {
                        self.tool_state.executable_draft.executable_path = value;
                    }
                    ExecutableDraftField::Arguments => {
                        self.tool_state.executable_draft.arguments = value;
                    }
                    ExecutableDraftField::WorkingDir => {
                        self.tool_state.executable_draft.working_dir = value;
                    }
                    ExecutableDraftField::Environment => {
                        self.tool_state.executable_draft.environment = value;
                    }
                    ExecutableDraftField::WineDllOverrides => {
                        self.tool_state.executable_draft.wine_dll_overrides = value;
                    }
                    ExecutableDraftField::OutputMod => {
                        self.tool_state.executable_draft.output_mod = value;
                    }
                }
                self.tool_state.executable_error = None;
            }
            Message::OpenExecutableEditor => {
                self.tool_state.executable_draft = ExecutableDraft::default();
                self.tool_state.executable_editor_open = true;
                self.tool_state.executable_error = None;
            }
            Message::ClearExecutableDraft => {
                self.tool_state.executable_draft = ExecutableDraft::default();
                self.tool_state.executable_editor_open = false;
                self.tool_state.executable_error = None;
            }
            Message::EditExecutable(name) => {
                if let Some(entry) = self
                    .tool_state
                    .executables
                    .iter()
                    .find(|entry| entry.name == name)
                    .cloned()
                {
                    self.tool_state.executable_draft = ExecutableDraft {
                        name: entry.name,
                        executable_path: entry.executable_path,
                        arguments: entry.arguments,
                        working_dir: entry.working_dir,
                        environment: entry.environment,
                        wine_dll_overrides: entry.wine_dll_overrides,
                        output_mod: entry.output_mod,
                    };
                    self.tool_state.executable_editor_open = true;
                    self.tool_state.executable_error = None;
                }
            }
            Message::SaveExecutable => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.status_message = "Select a game before saving executables".to_string();
                    return Task::none();
                };
                let row = match executable_draft_to_row(&game_id, &self.tool_state.executable_draft)
                {
                    Ok(row) => row,
                    Err(err) => {
                        self.tool_state.executable_error = Some(err.clone());
                        self.status_message = format!("Executable not saved: {err}");
                        return Task::none();
                    }
                };
                self.status_message = format!("Saving executable '{}'...", row.name);
                return Task::perform(save_executable_for_game(row), Message::ExecutableSaved);
            }
            Message::ExecutableSaved(result) => match result {
                Ok(message) => {
                    self.status_message = message;
                    self.tool_state.executable_error = None;
                    self.tool_state.executable_draft = ExecutableDraft::default();
                    self.tool_state.executable_editor_open = false;
                    return self.refresh_executables_or_tools();
                }
                Err(err) => {
                    self.tool_state.executable_error = Some(err.clone());
                    self.status_message = format!("Executable not saved: {err}");
                }
            },
            Message::RemoveExecutable(name) => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.status_message = "Select a game before removing executables".to_string();
                    return Task::none();
                };
                self.tool_state
                    .active_executable_operations
                    .insert(name.clone());
                return Task::perform(
                    remove_executable_for_game(game_id, name.clone()),
                    move |result| Message::ExecutableRemoved {
                        name: name.clone(),
                        result,
                    },
                );
            }
            Message::ExecutableRemoved { name, result } => {
                self.tool_state.active_executable_operations.remove(&name);
                match result {
                    Ok(message) => {
                        self.status_message = message;
                        return self.refresh_executables_or_tools();
                    }
                    Err(err) => {
                        self.tool_state.executable_error = Some(err.clone());
                        self.status_message = format!("Executable not removed: {err}");
                    }
                }
            }
            Message::RunExecutable(name) => {
                if self.tool_state.is_executable_busy(&name) {
                    self.status_message = format!("Executable '{name}' is already running");
                    return Task::none();
                }
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.status_message = "Select a game before running executables".to_string();
                    return Task::none();
                };
                self.tool_state
                    .active_executable_operations
                    .insert(name.clone());
                self.status_message = format!("Running executable '{name}'...");
                let profile = self.active_profile.clone();
                return Task::perform(
                    run_saved_executable_for_game(game_id, name.clone(), profile),
                    move |result| Message::ExecutableRunComplete {
                        name: name.clone(),
                        result,
                    },
                );
            }
            Message::ExecutableRunComplete { name, result } => {
                self.tool_state.active_executable_operations.remove(&name);
                match result {
                    Ok(message) => {
                        self.status_message = message;
                        return self.refresh_executables_or_tools();
                    }
                    Err(err) => {
                        self.tool_state.executable_error = Some(err.clone());
                        self.status_message = format!("Executable failed: {err}");
                    }
                }
            }
            Message::BrowseExecutablePath => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select executable")
                            .pick_file()
                            .await
                            .map(|handle| handle.path().to_path_buf())
                    },
                    Message::ExecutablePathSelected,
                );
            }
            Message::ExecutablePathSelected(path) => {
                if let Some(path) = path {
                    self.tool_state.executable_draft.executable_path = path.display().to_string();
                    self.tool_state.executable_editor_open = true;
                }
            }
            Message::BrowseExecutableWorkingDir => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select working directory")
                            .pick_folder()
                            .await
                            .map(|handle| handle.path().to_path_buf())
                    },
                    Message::ExecutableWorkingDirSelected,
                );
            }
            Message::ExecutableWorkingDirSelected(path) => {
                if let Some(path) = path {
                    self.tool_state.executable_draft.working_dir = path.display().to_string();
                    self.tool_state.executable_editor_open = true;
                }
            }
            Message::AdoptOptiScaler => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.status_message = "Select a game before adopting OptiScaler".to_string();
                    return Task::none();
                };
                let Some(game_dir) = self.current_game_dir() else {
                    self.status_message = "Game install path is not configured".to_string();
                    return Task::none();
                };
                match modde_core::db::ModdeDb::open() {
                    Ok(db) => {
                        let typed_game_id = GameId::from(game_id.as_str());
                        let tool = modde_games::tools::resolve_tool("optiscaler")
                            .expect("optiscaler tool is registered");
                        let context = self.current_tool_game_context();
                        let mut config = db
                            .load_tool_config(&typed_game_id, "optiscaler")
                            .ok()
                            .flatten()
                            .map_or_else(
                                || tool.default_config_for(context.as_ref()),
                                |row| modde_games::tools::ToolConfig {
                                    tool_id: row.tool_id,
                                    enabled: row.enabled,
                                    settings: serde_json::from_str(&row.settings_json)
                                        .unwrap_or_default(),
                                },
                            );
                        modde_games::tools::optiscaler::apply_game_defaults(
                            &mut config,
                            context.as_ref(),
                        );
                        let managed =
                            modde_games::tools::optiscaler::managed_paths_from_config(&config);
                        match modde_games::tools::optiscaler::scan_optiscaler_install(
                            &game_id, &game_dir, &managed,
                        ) {
                            Ok(state) => {
                                let paths = state
                                    .recognized_files
                                    .iter()
                                    .map(|file| state.executable_dir.join(&file.rel_path))
                                    .map(|path| {
                                        path.strip_prefix(&game_dir)
                                            .unwrap_or(&path)
                                            .to_string_lossy()
                                            .replace('\\', "/")
                                    })
                                    .collect::<Vec<_>>();
                                let applied = modde_games::tools::AppliedFiles {
                                    files: paths.iter().map(PathBuf::from).collect(),
                                };
                                config.enabled = true;
                                config.set(
                                    "managed_manifest",
                                    modde_games::tools::optiscaler::managed_manifest_json(
                                        &game_dir, &applied,
                                    ),
                                );
                                let settings_json = serde_json::to_string(&config.settings)
                                    .unwrap_or_else(|_| "{}".to_string());
                                let _ = db.save_tool_config(
                                    &typed_game_id,
                                    "optiscaler",
                                    true,
                                    &settings_json,
                                );
                                let _ = db.clear_applied_files(&typed_game_id, "optiscaler");
                                let _ = db.save_applied_files(&typed_game_id, "optiscaler", &paths);
                                self.tool_state.active_tool_id = Some("optiscaler".to_string());
                                self.status_message =
                                    format!("Adopted OptiScaler ({} file(s))", paths.len());
                                self.pending_tools_load_status_message =
                                    Some(self.status_message.clone());
                                return self.start_tools_load();
                            }
                            Err(err) => {
                                self.status_message = format!("Failed to scan OptiScaler: {err}");
                            }
                        }
                    }
                    Err(err) => {
                        self.status_message = format!("Failed to open tool database: {err}");
                    }
                }
            }
            Message::RestoreOptiScalerBackup => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.status_message = "Select a game before restoring OptiScaler".to_string();
                    return Task::none();
                };
                let Some(game_dir) = self.current_game_dir() else {
                    self.status_message = "Game install path is not configured".to_string();
                    return Task::none();
                };
                match modde_games::tools::optiscaler::restore_latest_optiscaler_backup(
                    &game_id, &game_dir,
                ) {
                    Ok(path) => {
                        self.tool_state.active_tool_id = Some("optiscaler".to_string());
                        self.status_message =
                            format!("Restored OptiScaler backup {}", path.display());
                        self.pending_tools_load_status_message = Some(self.status_message.clone());
                        return self.start_tools_load();
                    }
                    Err(err) => {
                        self.status_message = format!("Failed to restore OptiScaler: {err}");
                    }
                }
            }
            Message::ResetOptiScalerConfig => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.status_message = "Select a game before resetting OptiScaler".to_string();
                    return Task::none();
                };
                match modde_core::db::ModdeDb::open() {
                    Ok(db) => {
                        let typed_game_id = GameId::from(game_id.as_str());
                        let tool = modde_games::tools::resolve_tool("optiscaler")
                            .expect("optiscaler tool is registered");
                        let mut config = db
                            .load_tool_config(&typed_game_id, "optiscaler")
                            .ok()
                            .flatten()
                            .map_or_else(
                                || tool.default_config(),
                                |row| modde_games::tools::ToolConfig {
                                    tool_id: row.tool_id,
                                    enabled: row.enabled,
                                    settings: serde_json::from_str(&row.settings_json)
                                        .unwrap_or_default(),
                                },
                            );
                        if let serde_json::Value::Object(map) = &mut config.settings {
                            map.remove("ini_overrides");
                            map.insert("force_config_reset".to_string(), serde_json::json!(true));
                        }
                        let settings_json = serde_json::to_string(&config.settings)
                            .unwrap_or_else(|_| "{}".to_string());
                        match db.save_tool_config(
                            &typed_game_id,
                            "optiscaler",
                            config.enabled,
                            &settings_json,
                        ) {
                            Ok(()) => {
                                self.tool_state.active_tool_id = Some("optiscaler".to_string());
                                self.status_message =
                                    "Reset OptiScaler config overrides".to_string();
                                self.pending_tools_load_status_message =
                                    Some(self.status_message.clone());
                                return self.start_tools_load();
                            }
                            Err(err) => {
                                self.status_message =
                                    format!("Failed to reset OptiScaler config: {err}");
                            }
                        }
                    }
                    Err(err) => {
                        self.status_message = format!("Failed to open tool database: {err}");
                    }
                }
            }
            // Downloads
            Message::PauseDownload(id) => {
                self.download_queue.pause(id);
                self.status_message = "Download paused".to_string();
            }
            Message::ResumeDownload(id) => {
                self.download_queue.resume(id);
                if let Some(task) = self.download_queue.get_mut(id) {
                    task.state = modde_sources::queue::DownloadState::Active {
                        bytes_downloaded: task.meta.bytes_downloaded,
                        total_bytes: task.meta.total_bytes,
                    };
                    task.meta.status = "downloading".to_string();
                }
                self.status_message = "Download resumed".to_string();
            }
            Message::CancelDownload(id) => {
                self.download_lookup.retain(|_, value| *value != id);
                self.download_queue.cancel(id);
                self.status_message = "Download cancelled".to_string();
            }

            // Overwrite management
            Message::ClearOverwrite => {
                if let Some(profile) = &self.loaded_profile {
                    let _ = std::fs::remove_dir_all(&profile.overrides);
                    let _ = std::fs::create_dir_all(&profile.overrides);
                    self.status_message = "Overrides cleared".to_string();
                }
            }
            Message::MoveOverwriteToMod(mod_name) => {
                if let Some(profile) = &self.loaded_profile {
                    let store = modde_core::paths::store_dir();
                    let dest = store.join(&mod_name);
                    if profile.overrides.exists() {
                        let _ = std::fs::create_dir_all(&dest);
                        if let Ok(files) = modde_core::fs::walk_files_relative(&profile.overrides) {
                            for (rel, src) in &files {
                                let dst = dest.join(rel);
                                if let Some(parent) = dst.parent() {
                                    let _ = std::fs::create_dir_all(parent);
                                }
                                let _ = std::fs::rename(src, &dst);
                            }
                        }
                        self.status_message = format!("Moved overrides to mod '{mod_name}'");
                    }
                }
            }

            Message::ButtonHoverStarted { id, description } => {
                self.button_hover_toast.pending = Some(ButtonHoverToast { id, description });
                self.button_hover_toast.visible = None;
                return Task::perform(
                    async move {
                        tokio::time::sleep(BUTTON_HOVER_TOAST_DELAY).await;
                        id
                    },
                    |id| Message::ButtonHoverElapsed { id },
                );
            }
            Message::ButtonHoverElapsed { id } => {
                if self
                    .button_hover_toast
                    .pending
                    .is_some_and(|toast| toast.id == id)
                {
                    self.button_hover_toast.visible = self.button_hover_toast.pending;
                }
            }
            Message::ButtonHoverEnded { id } => {
                if self
                    .button_hover_toast
                    .pending
                    .is_some_and(|toast| toast.id == id)
                {
                    self.button_hover_toast.pending = None;
                }
                if self
                    .button_hover_toast
                    .visible
                    .is_some_and(|toast| toast.id == id)
                {
                    self.button_hover_toast.visible = None;
                }
            }

            Message::Noop => {}
        }
        Task::none()
    }

    // ─── View ────────────────────────────────────────────────────

    fn view(&self) -> Element<'_, Message> {
        // Show mod details in sidebar on all views except Saves;
        // show save details only on Saves view.
        let (mod_details_for_sidebar, save_details_for_sidebar) =
            if matches!(self.active_view, View::Saves) {
                (None, self.selected_save_details.as_ref())
            } else {
                (self.selected_mod_details.as_ref(), None)
            };
        let save_profiles_supported = self.current_game_supports_save_profiles();

        let sidebar = crate::views::sidebar::view(
            &self.active_view,
            &self.collapsed_sidebar_groups,
            &self.profiles,
            &self.active_profile,
            self.experiment_depth,
            save_profiles_supported,
            mod_details_for_sidebar,
            save_details_for_sidebar,
        );

        let mods = self
            .loaded_profile
            .as_ref()
            .map(|p| p.mods.as_slice())
            .unwrap_or(&[]);
        let settings_state = self.settings_state();

        let content: Element<Message> = match &self.active_view {
            View::ModList => crate::views::mod_list::view_filtered(
                mods,
                &self.mod_id_filter_keys,
                &self.mod_filter,
                self.selected_mod_index,
                self.filter_mode,
                &self.filter_criteria,
                &self.collapsed_categories,
                &self.mod_categories,
                self.compact_mod_list,
                self.loaded_profile
                    .as_ref()
                    .is_some_and(|p| p.load_order_lock.is_some()),
            ),
            View::Collections => crate::views::collections::view(
                &self.collection_search,
                &self.collections,
                &self.active_downloads,
            ),
            View::BrowseNexus => {
                let domain = self.browse_game_nexus_domain();
                crate::views::browse_nexus::view(
                    &self.browse_nexus,
                    self.available_games.as_slice(),
                    domain,
                )
            }
            View::FOMODWizard(_) => crate::views::fomod_wizard::view(self),
            View::Settings => crate::views::settings::view(settings_state),
            View::WabbajackInstaller(state) => crate::views::wabbajack::view(
                state,
                &self.wabbajack_manifest,
                self.available_games.as_slice(),
                self.current_game_id(),
            ),
            View::Saves => crate::views::saves::view(
                &self.save_snapshots,
                self.loaded_profile.as_ref().map(|p| p.name.as_str()),
                self.current_fingerprint.as_ref(),
                save_profiles_supported,
                self.selected_save_details
                    .as_ref()
                    .map(|d| d.commit_id.as_str()),
            ),
            View::Downloads => {
                let tasks = self.downloads_view_tasks();
                crate::views::downloads::view(&tasks)
            }
            View::DataTab => {
                crate::views::data_tab::view(&self.data_tab_state, &self.data_tab_conflicts)
            }
            View::Diagnostics => crate::views::diagnostics::view(&self.diagnostics_state),
            View::Tools => crate::views::tools::view(&self.tool_state),
            View::Executables => crate::views::executables::view(&self.tool_state),
        };

        // ── Custom title bar ──
        let game_options = crate::views::game_picker::supported_game_options_ordered(
            self.available_games.iter(),
            &self.detected_games,
        );
        let selected_game = self.selected_game.as_ref().and_then(|id| {
            game_options
                .iter()
                .find(|option| option.value == *id)
                .cloned()
        });
        let game_picker = crate::views::game_picker::game_pick_list(
            game_options,
            selected_game,
            "Select a game",
            |option| Message::SelectGame(option.value),
        );
        let add_custom_game_button = crate::semantics::test_id(
            "game_picker.add_custom_game",
            button(text("+ Add custom game").size(12))
                .style(button::secondary)
                .padding([4, 10])
                .on_action(ButtonAction::OpenAddCustomGame),
        );
        let manage_custom_games_button = crate::semantics::test_id(
            "game_picker.manage_custom_games",
            button(text("Manage custom games").size(12))
                .style(button::secondary)
                .padding([4, 10])
                .on_action(ButtonAction::OpenManageCustomGames),
        );

        let title_label = text("modde").size(14);

        let window_controls = row![
            button(text("\u{2212}").size(12))
                .style(button::secondary)
                .padding([2, 10])
                .on_action(ButtonAction::WindowMinimize),
            button(text("\u{25A1}").size(12))
                .style(button::secondary)
                .padding([2, 10])
                .on_action(ButtonAction::WindowToggleMaximize),
            button(text("\u{2715}").size(12))
                .style(button::danger)
                .padding([2, 10])
                .on_action(ButtonAction::WindowClose),
        ]
        .spacing(2);

        let title_bar_content = row![
            game_picker,
            add_custom_game_button,
            manage_custom_games_button,
            iced::widget::Space::new().width(Length::Fill),
            title_label,
            iced::widget::Space::new().width(Length::Fill),
            window_controls,
        ]
        .align_y(iced::Alignment::Center)
        .spacing(8);

        let title_bar = mouse_area(
            container(title_bar_content)
                .padding([4, 8])
                .width(Length::Fill)
                .style(container::rounded_box),
        )
        .on_press(Message::TitleBarDrag);

        let status_bar = container(text(&self.status_message).size(12)).padding(5);

        let update_banner = self
            .update_available
            .as_ref()
            .map(crate::components::update_banner::view);

        let body: Element<Message> = if let Some(update_banner) = update_banner {
            column![
                update_banner,
                row![sidebar, content].spacing(0).height(Length::Fill)
            ]
            .spacing(0)
            .into()
        } else {
            row![sidebar, content]
                .spacing(0)
                .height(Length::Fill)
                .into()
        };

        let main_layout = column![title_bar, body, status_bar,].spacing(0);

        let mut base: Element<Message> = container(main_layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        let toast_layer: Element<Message> = if let Some(toast) = self.button_hover_toast.visible {
            let toast_content: Element<Message> = container(text(toast.description).size(12))
                .padding([8, 12])
                .width(Length::Shrink)
                .style(container::rounded_box)
                .into();
            container(
                column![
                    iced::widget::Space::new().height(Length::Fill),
                    row![
                        iced::widget::Space::new().width(Length::Fill),
                        toast_content
                    ]
                ]
                .padding(16),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            container(iced::widget::Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        base = stack([base, toast_layer]).into();

        if self.new_profile_dialog_open {
            base = stack([base, self.new_profile_dialog()]).into();
        }
        if self.game_path_dialog_open {
            base = stack([base, self.game_path_dialog()]).into();
        }
        if self.add_custom_game_dialog_open {
            base = stack([base, self.add_custom_game_modal()]).into();
        }
        if self.manage_custom_games_dialog_open {
            base = stack([base, self.manage_custom_games_modal()]).into();
        }

        crate::shortcut_layer::shortcut_layer(base).into()
    }

    fn new_profile_dialog(&self) -> Element<'_, Message> {
        let trimmed_name = self.new_profile_name.trim();
        let can_create = !trimmed_name.is_empty() && self.selected_game.is_some();
        let submit = can_create.then_some(Message::SubmitNewProfileDialog);
        let submit_action = can_create.then_some(ButtonAction::SubmitNewProfileDialog);

        let dialog = container(
            column![
                text("New Profile").size(18),
                text_input("Profile name...", &self.new_profile_name)
                    .on_input(Message::NewProfileNameChanged)
                    .on_submit_maybe(submit.clone())
                    .padding(8)
                    .width(Length::Fill),
                row![
                    iced::widget::Space::new().width(Length::Fill),
                    button(text("Cancel").size(13))
                        .style(button::secondary)
                        .padding([6, 14])
                        .on_action(ButtonAction::CancelNewProfileDialog),
                    button(text("Create").size(13))
                        .style(button::success)
                        .padding([6, 14])
                        .on_action_maybe(
                            submit_action,
                            "Enter a profile name and select a game before creating the profile.",
                        ),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(12),
        )
        .width(Length::Fixed(360.0))
        .padding(16)
        .style(container::rounded_box);

        opaque(
            container(dialog)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
    }

    fn game_path_dialog(&self) -> Element<'_, Message> {
        let game_id = self
            .pending_game_path_game_id
            .as_deref()
            .unwrap_or("the selected game");
        let game_label = modde_games::resolve_game_plugin(game_id)
            .map(modde_games::GamePlugin::display_name)
            .unwrap_or(game_id);

        let mut body = column![
            text("Game Path Required").size(18),
            text(format!(
                "modde could not detect {game_label}. Select the game installation directory to continue."
            ))
            .size(13),
        ]
        .spacing(12);

        if let Some(error) = &self.game_path_dialog_error {
            body = body.push(text(error).size(12).color(iced::color!(0xFF6666)));
        }

        let dialog = container(
            body.push(
                row![
                    iced::widget::Space::new().width(Length::Fill),
                    button(text("Cancel").size(13))
                        .style(button::secondary)
                        .padding([6, 14])
                        .on_action(ButtonAction::CancelGamePathDialog),
                    button(text("Browse").size(13))
                        .style(button::primary)
                        .padding([6, 14])
                        .on_action(ButtonAction::GamePathDialogBrowse),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            ),
        )
        .width(Length::Fixed(420.0))
        .padding(16)
        .style(container::rounded_box);

        opaque(
            container(dialog)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill),
        )
    }

    fn theme(&self) -> Theme {
        match self.theme_name.as_str() {
            "Light" => Theme::Light,
            "Dracula" => Theme::Dracula,
            "Nord" => Theme::Nord,
            "Gruvbox Dark" => Theme::GruvboxDark,
            "Catppuccin Mocha" => Theme::CatppuccinMocha,
            _ => Theme::Dark,
        }
    }

    /// Subscribe to external refresh signals from the CLI.
    ///
    /// We bind a Unix domain socket at [`modde_core::ipc::socket_path`]
    /// and emit a [`Message::ExternalRefresh`] for every connection
    /// received. The socket is cleaned up on startup (in case a
    /// previous GUI crashed without unlinking) and on each new bind.
    /// While idle, the listener costs nothing — `accept()` just blocks
    /// in the kernel.
    fn subscription(&self) -> iced::Subscription<Message> {
        iced::Subscription::run(external_refresh_stream)
    }
}

fn external_refresh_stream() -> impl iced::futures::Stream<Item = Message> {
    use iced::futures::SinkExt as _;
    use tokio::io::AsyncReadExt as _;

    iced::stream::channel(8, async move |mut output| {
        // Per-process socket path: `modde-${euid}-${pid}.sock`. Each
        // GUI gets its own; the CLI fan-outs to every matching file
        // in `$XDG_RUNTIME_DIR`, so multi-window installs all see
        // every change.
        let path = modde_core::ipc::gui_socket_path();
        // Pid collisions are essentially impossible inside a single
        // boot, but be defensive: if a previous run of *this exact
        // pid* (rare; happens with pid wraparound on long-uptime
        // systems) left a node behind, unlink it.
        let _ = std::fs::remove_file(&path);

        let listener = match tokio::net::UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    socket = %path.display(),
                    "could not bind refresh socket; CLI → GUI live updates disabled \
                     for this window"
                );
                return;
            }
        };

        // Best-effort cleanup at process exit so the CLI's GC pass
        // doesn't have to do it. Held in a guard captured by the
        // listening task: dropped when the subscription stream is
        // torn down (window close / app shutdown), which unlinks
        // the socket. If the process panics the file leaks, but the
        // CLI side GCs unreachable sockets on every notify pass.
        let _guard = SocketGuard::new(path.clone());

        tracing::info!(socket = %path.display(), "listening for CLI refresh signals");

        loop {
            let (mut stream, _addr) = match listener.accept().await {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(error = %e, "accept failed; restarting listen loop");
                    continue;
                }
            };
            // Drain whatever the peer sent so the kernel buffer is
            // freed; we don't actually parse the payload — the
            // existence of the connection is the signal.
            let mut buf = [0u8; 64];
            let _ = stream.read(&mut buf).await;
            if output.send(Message::ExternalRefresh).await.is_err() {
                // Application is shutting down.
                break;
            }
        }
    })
}

/// Drop guard that unlinks a Unix socket when the listening task ends.
struct SocketGuard {
    path: std::path::PathBuf,
}

impl SocketGuard {
    fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        modde_core::ipc::cleanup_socket(&self.path);
    }
}

/// Max thumbnail dimensions — matches the sidebar image slot
/// (~166 px wide, 96 px tall).  We decode the fetched bytes, scale
/// down with a Lanczos3 filter, and hand the smaller RGBA buffer to
/// iced so it doesn't keep a multi-megapixel texture around.
const THUMB_MAX_W: u32 = 340;
const THUMB_MAX_H: u32 = 192;

fn resize_thumbnail_bytes(raw: &[u8]) -> iced::widget::image::Handle {
    let Ok(img) = image::load_from_memory(raw) else {
        // If decoding fails, fall back to letting iced try the raw bytes.
        return iced::widget::image::Handle::from_bytes(raw.to_vec());
    };

    let resized = img.resize(
        THUMB_MAX_W,
        THUMB_MAX_H,
        image::imageops::FilterType::Lanczos3,
    );
    let rgba = resized.to_rgba8();
    let (w, h) = rgba.dimensions();
    iced::widget::image::Handle::from_rgba(w, h, rgba.into_raw())
}

/// Map a matched shortcut action string to the corresponding
/// `Message`. Returns `None` for actions whose handlers aren't wired
/// yet — those shortcuts still register in `all_shortcuts()` for
/// help-text purposes but produce no messages until their handlers
/// exist (most need state-aware lookups or new `Message` variants).
pub(crate) fn shortcut_action_to_message(action: &str) -> Option<Message> {
    match action {
        "deploy" => Some(Message::Deploy),
        "dismiss_modal" => Some(Message::CancelNewProfileDialog),
        _ => None,
    }
}

/// Run the iced application.
pub fn run() -> iced::Result {
    iced::application(Modde::new, Modde::update, Modde::view)
        .title(Modde::title)
        .theme(Modde::theme)
        .subscription(Modde::subscription)
        .decorations(false)
        .run()
}

#[cfg(test)]
mod tests;
