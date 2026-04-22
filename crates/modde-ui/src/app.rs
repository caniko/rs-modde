use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use iced::widget::{button, column, container, mouse_area, pick_list, row, text};
use iced::{Element, Length, Subscription, Task, Theme, keyboard, window};
use smallvec::SmallVec;

use modde_core::filter::{FilterCriterion, FilterKind, FilterMode};
use modde_core::manifest::collection::CollectionManifest;
use modde_core::profile::ProfileManager;
use modde_core::save::SaveSnapshot;
use modde_core::settings::AppSettings;

/// Settings view state — consumed by the settings view.
#[derive(Debug, Clone, Default)]
pub struct SettingsState {
    pub nexus_api_key: String,
    pub game_path: Option<PathBuf>,
    pub download_dir: Option<PathBuf>,
    pub has_stock_snapshot: bool,
    pub theme_name: String,
    pub nexus_status: Option<NexusAuthStatus>,
}

#[derive(Debug, Clone)]
pub enum NexusAuthStatus {
    Checking,
    Valid { username: String, is_premium: bool },
    Invalid(String),
}

/// Top-level application state.
pub struct Modde {
    pub active_view: View,
    pub active_profile: Option<String>,
    pub profiles: Vec<modde_core::profile::ProfileSummary>,
    pub status_message: String,
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
    pub verify: VerifyState,
    pub new_profile_name: String,
    pub available_games: SmallVec<[(String, String); 8]>,
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
}

#[derive(Debug, Clone)]
pub struct VerifyResults {
    pub missing_mods: SmallVec<[String; 8]>,
    pub hash_mismatches: Vec<(PathBuf, String, String)>,
    pub broken_symlinks: SmallVec<[PathBuf; 8]>,
    pub ok_count: usize,
}

/// Compile-time–friendly state machine for the verification pipeline.
///
/// Replaces the previous `verify_running: bool` + `verify_results: Option<VerifyResults>`
/// pair, which could represent invalid states (e.g. `running = false, results = None`
/// after a failed run vs. before any run). This enum makes every state explicit.
#[derive(Debug, Clone, Default)]
pub enum VerifyState {
    /// No verification has been requested.
    #[default]
    Idle,
    /// Verification is in progress.
    Running,
    /// Verification completed with results.
    Complete(VerifyResults),
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
            nexus_api_key: self.settings.nexus_api_key.clone(),
            game_path: self.settings.game_paths.first().map(|gp| gp.path.clone()),
            download_dir: self.settings.download_dir.clone(),
            has_stock_snapshot: self.stock_snapshot_exists,
            theme_name: self.theme_name.clone(),
            nexus_status: self.nexus_status.clone(),
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

    fn reload_profile(&mut self) {
        if let Some(ref name) = self.active_profile {
            if let Ok(pm) = ProfileManager::open() {
                self.profiles = pm.list().unwrap_or_default();
                if let Ok(profile) = pm.load(name, None) {
                    if let Ok(info) = pm.active(&profile.game_id) {
                        self.experiment_depth = info.map(|i| i.experiment_depth).unwrap_or(0);
                    }

                    // Compute save fingerprint
                    self.current_fingerprint = {
                        let game_id = profile.game_id.as_str();
                        let staging_dir = ProfileManager::staging_dir(&profile.name);
                        modde_games::resolve_game_plugin(game_id).map(|plugin| {
                            modde_core::save::SaveFingerprint::compute(&profile.mods, |mod_id| {
                                let mod_path = staging_dir.join(mod_id);
                                plugin.classify_mod(&mod_path).affects_saves()
                            })
                        })
                    };

                    self.loaded_profile = Some(profile);
                }
            }
        } else {
            self.loaded_profile = None;
        }

        self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Idle;
        self.refresh_data_tab_conflicts();
        self.refresh_tools_state();
    }

    fn save_settings(&self) {
        self.settings.save();
    }

    fn current_game_id(&self) -> Option<&str> {
        self.loaded_profile
            .as_ref()
            .map(|profile| profile.game_id.as_str())
            .or(self.selected_game.as_deref())
    }

    fn current_game_dir(&self) -> Option<PathBuf> {
        let game_id = self.current_game_id()?;
        self.settings.game_path(game_id).cloned().or_else(|| {
            modde_games::resolve_game_plugin(game_id).and_then(|plugin| plugin.detect_install())
        })
    }

    fn refresh_data_tab_conflicts(&mut self) {
        let Some(profile) = self.loaded_profile.as_ref() else {
            self.data_tab_conflicts.clear();
            return;
        };

        let Ok(pm) = ProfileManager::open() else {
            self.data_tab_conflicts.clear();
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
                self.data_tab_conflicts = build_conflict_rows(&analysis, &hidden);
            }
            Err(err) => {
                self.data_tab_conflicts.clear();
                self.status_message = format!("Failed to load data tab: {err}");
            }
        }
    }

    fn run_diagnostics_now(&mut self) {
        let Some(profile) = self.loaded_profile.clone() else {
            self.status_message = "Select a profile before running diagnostics".to_string();
            self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Idle;
            return;
        };

        let Ok(pm) = ProfileManager::open() else {
            self.status_message = "Failed to open profile database".to_string();
            self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Idle;
            return;
        };

        let hidden = load_hidden_files(&pm, &profile);
        let active_plugins = load_active_plugins(&pm, &profile);
        let engine = match profile.game_id.as_str() {
            "skyrim-se" | "skyrim-ae" | "fallout4" | "fallout76" => {
                modde_games::bethesda::diagnostics::bethesda_diagnostics()
            }
            _ => modde_core::diagnostics::DiagnosticEngine::new(),
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
                self.data_tab_conflicts = build_conflict_rows(&analysis, &hidden);
                let entries: Vec<_> = diagnostics.iter().map(format_diagnostic_entry).collect();
                let count = entries.len();
                self.diagnostics_state =
                    crate::views::diagnostics::DiagnosticsState::Complete(entries);
                self.status_message = if count == 0 {
                    "Diagnostics complete: no issues found".to_string()
                } else {
                    format!("Diagnostics complete: {count} issue(s)")
                };
            }
            Err(err) => {
                self.diagnostics_state =
                    crate::views::diagnostics::DiagnosticsState::Complete(vec![
                        crate::views::diagnostics::DiagnosticEntry {
                            severity: crate::views::diagnostics::DiagnosticSeverity::Error,
                            message: format!("Diagnostics failed: {err}"),
                        },
                    ]);
                self.status_message = format!("Diagnostics failed: {err}");
            }
        }
    }

    fn refresh_tools_state(&mut self) {
        let Some(game_id) = self.current_game_id().map(str::to_string) else {
            self.tool_state.entries.clear();
            return;
        };

        let Ok(db) = modde_core::db::ModdeDb::open() else {
            self.tool_state.entries.clear();
            return;
        };

        self.tool_state.entries = modde_games::tools::all_tools()
            .iter()
            .map(|tool| {
                let row = db.load_tool_config(&game_id, tool.tool_id()).ok().flatten();
                let availability = tool.detect_available();
                let applied_files = db
                    .load_applied_files(&game_id, tool.tool_id())
                    .map(|files| files.len())
                    .unwrap_or(0);
                let status_message = match availability {
                    modde_games::tools::ToolAvailability::Available {
                        version: Some(version),
                    } => Some(format!("Detected {version}")),
                    modde_games::tools::ToolAvailability::NotInstalled { install_hint } => {
                        Some(install_hint)
                    }
                    modde_games::tools::ToolAvailability::Available { version: None } => None,
                };

                ToolUiEntry {
                    tool_id: tool.tool_id().to_string(),
                    display_name: tool.display_name().to_string(),
                    category: tool.category().to_string(),
                    available: tool.detect_available().is_available(),
                    enabled: row.as_ref().is_some_and(|config| config.enabled),
                    applied_files,
                    has_file_patching: matches!(tool.tool_id(), "reshade" | "optiscaler"),
                    status_message,
                }
            })
            .collect();
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
        modde_games::resolve_game_plugin(&game_id)
            .and_then(|p| p.nexus_game_domain())
            .map(str::to_string)
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

/// Run the full install pipeline for a single Nexus mod, invoked from
/// the Browse Nexus **Install** button. Owned as a free function so
/// the `update()` arm can hand it to `Task::perform` without borrowing
/// `self`.
async fn run_browse_install(game_domain: String, mod_id: u64) -> Result<String, String> {
    let api_key = modde_sources::nexus::auth::load_api_key().map_err(|e| e.to_string())?;
    let client = reqwest::Client::new();
    let api = modde_sources::nexus::api::NexusApi::new(client.clone(), api_key.clone());

    // Look up the latest MAIN file id.
    let files = api
        .get_mod_files(&game_domain, mod_id)
        .await
        .map_err(|e| e.to_string())?;
    let mut candidates: Vec<_> = files
        .files
        .into_iter()
        .filter(|f| f.category_name.as_deref() == Some("MAIN"))
        .collect();
    candidates.sort_by_key(|f| std::cmp::Reverse(f.uploaded_timestamp));
    let file_id = candidates
        .first()
        .map(|f| f.file_id)
        .ok_or_else(|| format!("no MAIN file found for mod {mod_id}"))?;

    let mod_info = api
        .get_mod(&game_domain, mod_id)
        .await
        .map_err(|e| e.to_string())?;

    // Build a probe from whichever game plugin is registered.
    let probe = modde_games::resolve_game_plugin(&game_domain)
        .map(modde_games::game_probe)
        .unwrap_or_else(modde_core::installer::InstallProbe::noop);

    let outcome = modde_sources::nexus::install::install_single_mod(
        &client,
        &api_key,
        &game_domain,
        mod_id,
        file_id,
        &mod_info,
        &probe,
    )
    .await
    .map_err(|e| e.to_string())?;

    use modde_core::installer::InstallStatus;
    use modde_sources::nexus::install::InstallOutcome;

    let mod_id_str = format!("{game_domain}_{mod_id}_{file_id}");
    let pm = modde_core::profile::ProfileManager::open().map_err(|e| e.to_string())?;

    // Prefer an existing profile for the game; fall back to creating a
    // Manual profile named after the game domain if none exist.
    let profile_name = pm
        .list()
        .ok()
        .and_then(|profiles| {
            profiles
                .into_iter()
                .find(|p| p.game_id.as_str() == game_domain)
                .map(|p| p.name)
        })
        .unwrap_or_else(|| game_domain.clone());
    let mut profile = match pm.load(&profile_name, None) {
        Ok(p) => p,
        Err(_) => modde_core::profile::Profile {
            id: None,
            name: profile_name.clone(),
            game_id: modde_core::resolver::GameId::from(game_domain.clone()),
            source: modde_core::profile::ProfileSource::Manual,
            mods: Vec::new(),
            overrides: modde_core::profile::ProfileManager::default_overrides(&profile_name),
            load_order_rules: smallvec::SmallVec::new(),
            load_order_lock: None,
        },
    };
    let status = match &outcome {
        InstallOutcome::Installed(_) | InstallOutcome::AlreadyStaged => InstallStatus::Installed,
        InstallOutcome::PendingUserInput { .. } => InstallStatus::PendingUserInput,
        InstallOutcome::Unknown { .. } => InstallStatus::Unknown,
    };
    if !profile.mods.iter().any(|m| m.mod_id == mod_id_str) {
        profile.mods.push(modde_core::profile::EnabledMod {
            mod_id: mod_id_str.clone(),
            display_name: Some(mod_info.name.clone()),
            enabled: true,
            version: Some(mod_info.version.clone()),
            nexus_mod_id: Some(mod_id as i64),
            nexus_file_id: Some(file_id as i64),
            nexus_game_domain: Some(game_domain.clone()),
            install_status: Some(status.as_str().to_string()),
            ..Default::default()
        });
    }
    pm.create_or_update(&profile).map_err(|e| e.to_string())?;

    if let InstallOutcome::Installed(plan) = &outcome {
        let mut db = modde_core::ModdeDb::open().map_err(|e| e.to_string())?;
        let profile_id = pm
            .load(&profile_name, None)
            .map_err(|e| e.to_string())?
            .id
            .ok_or_else(|| "saved profile has no id".to_string())?;
        db.record_install(profile_id, &mod_id_str, plan, InstallStatus::Installed)
            .map_err(|e| e.to_string())?;
    }

    Ok(match outcome {
        InstallOutcome::Installed(_) | InstallOutcome::AlreadyStaged => {
            format!("Installed '{}'", mod_info.name)
        }
        InstallOutcome::PendingUserInput { method } => {
            format!("'{}' needs {method} wizard", mod_info.name)
        }
        InstallOutcome::Unknown { dossier_path, .. } => {
            format!(
                "Unknown install layout — dossier: {}",
                dossier_path.display()
            )
        }
    })
}

/// Direction for `Message::ReorderMod`. Re-export of the core type so
/// view and message construction sites don't have to import from
/// `modde_core::profile` directly. The enforcement logic itself lives in
/// `modde_core::profile::try_reorder` and is the single source of truth.
pub use modde_core::profile::ReorderDirection;

/// Render a `LockReason` as a short, human-readable phrase for status
/// messages and UI banners. Centralised so lock_reason strings stay
/// consistent across the handler, load_order banner, and mod_list row.
pub(crate) fn format_lock_reason(reason: &modde_core::LockReason) -> String {
    use modde_core::LockReason::*;
    match reason {
        Wabbajack { manifest_hash } => format!("Wabbajack (hash {manifest_hash})"),
        NexusCollection { slug, version } => format!("Nexus Collection '{slug}' v{version}"),
        TomlImport { source_path } => format!("TOML import from {source_path}"),
        Manual { note: Some(n) } => format!("manual ({n})"),
        Manual { note: None } => "manual".to_string(),
    }
}

/// Which view is currently displayed.
#[derive(Debug, Clone)]
pub enum View {
    ModList,
    Collections,
    /// Unified Nexus browse surface — Top / Month / Collections / Search.
    BrowseNexus,
    WabbajackInstaller(WabbajackInstallerState),
    FOMODWizard(FOMODWizardState),
    Settings,
    Saves,
    Verify,
    Downloads,
    DataTab,
    Diagnostics,
    Tools,
}

#[derive(Debug, Clone, Default)]
pub struct ToolState {
    pub entries: Vec<ToolUiEntry>,
}

#[derive(Debug, Clone)]
pub struct ToolUiEntry {
    pub tool_id: String,
    pub display_name: String,
    pub category: String,
    pub available: bool,
    pub enabled: bool,
    pub applied_files: usize,
    pub has_file_patching: bool,
    pub status_message: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct WabbajackInstallerState {
    pub file_path: Option<PathBuf>,
    pub progress: f32,
    pub status: String,
    pub log_lines: Vec<String>,
}

pub struct FOMODWizardState {
    pub current_step: usize,
    pub total_steps: usize,
    inner: Option<fomod_oxide::installer::Installer>,
}

impl std::fmt::Debug for FOMODWizardState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FOMODWizardState")
            .field("current_step", &self.current_step)
            .field("total_steps", &self.total_steps)
            .field("has_inner", &self.inner.is_some())
            .finish()
    }
}

impl Clone for FOMODWizardState {
    fn clone(&self) -> Self {
        Self {
            current_step: self.current_step,
            total_steps: self.total_steps,
            inner: None,
        }
    }
}

impl FOMODWizardState {
    pub fn new() -> Self {
        Self {
            current_step: 0,
            total_steps: 0,
            inner: None,
        }
    }

    pub fn with_installer(installer: fomod_oxide::installer::Installer) -> Self {
        let total = installer.visible_steps().len();
        Self {
            current_step: 0,
            total_steps: total,
            inner: Some(installer),
        }
    }

    pub fn visible_steps(&self) -> Vec<(usize, &fomod_oxide::config::InstallStep)> {
        match &self.inner {
            Some(installer) => installer.visible_steps(),
            None => vec![],
        }
    }

    pub fn config(&self) -> &fomod_oxide::config::ModuleConfig {
        self.inner.as_ref().expect("no FOMOD installer").config()
    }

    pub fn module_image_path(&self) -> Option<&str> {
        self.inner.as_ref()?.module_image_path()
    }

    pub fn resolve_image(&self, base_path: &std::path::Path, image_path: &str) -> Option<PathBuf> {
        self.inner.as_ref()?.resolve_image(base_path, image_path)
    }

    pub fn completion_status(&self) -> fomod_oxide::installer::CompletionStatus {
        match &self.inner {
            Some(installer) => installer.completion_status(),
            None => fomod_oxide::installer::CompletionStatus {
                total_steps: 0,
                visible_steps: 0,
                total_groups: 0,
                satisfied_groups: 0,
            },
        }
    }

    pub fn validate_step(&self, step_index: usize) -> Vec<fomod_oxide::installer::ValidationHint> {
        match &self.inner {
            Some(installer) => installer.validate_step(step_index),
            None => vec![],
        }
    }

    pub fn plugin_type_at(
        &self,
        step: usize,
        group: usize,
        plugin: usize,
    ) -> Option<fomod_oxide::config::PluginType> {
        self.inner.as_ref()?.plugin_type_at(step, group, plugin)
    }

    pub fn plugin_image_path(&self, step: usize, group: usize, plugin: usize) -> Option<&str> {
        self.inner.as_ref()?.plugin_image_path(step, group, plugin)
    }

    pub fn preview_plugin(
        &self,
        step: usize,
        group: usize,
        plugin: usize,
    ) -> Vec<fomod_oxide::installer::FileOperation> {
        match &self.inner {
            Some(installer) => installer.preview_plugin(step, group, plugin),
            None => vec![],
        }
    }

    pub fn preview_current(&self) -> fomod_oxide::installer::InstallPlan {
        match &self.inner {
            Some(installer) => installer.preview_current(),
            None => fomod_oxide::installer::InstallPlan { operations: vec![] },
        }
    }

    pub fn is_ready_to_install(&self) -> bool {
        match &self.inner {
            Some(installer) => installer.is_ready_to_install(),
            None => false,
        }
    }

    pub fn group_type_at(
        &self,
        step: usize,
        group: usize,
    ) -> Option<fomod_oxide::config::GroupType> {
        self.inner.as_ref()?.group_type_at(step, group)
    }

    pub fn select(&mut self, step: usize, group: usize, plugin_indices: Vec<usize>) {
        if let Some(ref mut installer) = self.inner {
            installer.select(step, group, plugin_indices);
        }
    }

    pub fn checkpoint(&mut self) {
        if let Some(ref mut installer) = self.inner {
            installer.checkpoint();
        }
    }

    pub fn rollback(&mut self) -> bool {
        match &mut self.inner {
            Some(installer) => installer.rollback(),
            None => false,
        }
    }

    pub fn history_len(&self) -> usize {
        match &self.inner {
            Some(installer) => installer.history_len(),
            None => 0,
        }
    }

    pub fn selections(&self) -> HashMap<(usize, usize), Vec<usize>> {
        match &self.inner {
            Some(installer) => installer.selections().clone(),
            None => HashMap::new(),
        }
    }

    pub fn detect_conflicts(&self) -> Vec<String> {
        match &self.inner {
            Some(installer) => installer
                .detect_conflicts()
                .into_iter()
                .map(|c| c.destination)
                .collect(),
            None => vec![],
        }
    }

    pub fn resolve(&self) -> fomod_oxide::installer::InstallPlan {
        match &self.inner {
            Some(installer) => installer.resolve(),
            None => fomod_oxide::installer::InstallPlan { operations: vec![] },
        }
    }

    pub fn default_selections(&self) -> Vec<(usize, usize, Vec<usize>)> {
        let installer = match &self.inner {
            Some(i) => i,
            None => return vec![],
        };
        let visible = installer.visible_steps();
        let mut defaults = Vec::new();
        for &(step_idx, step) in &visible {
            if let Some(ref groups) = step.optional_file_groups {
                for (group_idx, group) in groups.groups.iter().enumerate() {
                    let sel = fomod_oxide::Installer::default_selections(group);
                    defaults.push((step_idx, group_idx, sel));
                }
            }
        }
        defaults
    }
}

// ─── Application Messages ────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Message {
    // Navigation
    SwitchView(View),
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
    NewProfileNameChanged(String),

    // Game selection
    SelectGame(String),

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
        nexus_mod_id: i64,
        result: Result<modde_sources::nexus::api::NexusMod, String>,
    },
    /// Gallery image URL list returned by the v2 GraphQL endpoint.
    ModGalleryLoaded {
        nexus_mod_id: i64,
        urls: Vec<String>,
    },
    /// Image bytes downloaded for a specific gallery slot. Guarded by both
    /// `nexus_mod_id` and `gallery_index` so clicking through the gallery
    /// rapidly doesn't let an old image overwrite a newer one.
    ModThumbnailLoaded {
        nexus_mod_id: i64,
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
    /// index-based) because the load_order view and mod_list view operate
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
        mod_id: u64,
    },
    /// Async completion of a browse install. The `Ok` payload is a
    /// short human-readable status message; `Err` is an error string.
    BrowseInstallResult {
        download_key: String,
        result: Result<String, String>,
    },

    // Wabbajack
    OpenWabbajackFile,
    WabbajackFileSelected(PathBuf),
    WabbajackProgress(f32),
    WabbajackStartInstall,
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
    SetNexusApiKey(String),
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

    // Verification
    RunVerify,
    VerifyComplete(VerifyResults),

    // Mod list – category separators
    ToggleSeparator(Option<i64>),

    // Data tab
    DataTabFilterChanged(String),
    DataTabToggleConflicts(bool),

    // Diagnostics
    RunDiagnostics,

    // Tools
    RefreshTools,
    ToggleTool {
        tool_id: String,
        enabled: bool,
    },
    ApplyTool(String),
    RevertTool(String),

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
        nexus_mod_id: i64,
        new_status: String,
        result: Result<(), String>,
    },
    /// User clicked the track/untrack toggle button.
    ModTrackToggle,
    /// Async result of a track or untrack call.
    ModTrackResult {
        nexus_mod_id: i64,
        new_tracked: bool,
        result: Result<(), String>,
    },
    /// Async result of the initial `get_tracked_mods` call fired alongside
    /// `get_mod` when a mod is selected.
    ModTrackedSetLoaded {
        nexus_mod_id: i64,
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

    // Misc
    Noop,
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

        let profiles = ProfileManager::open()
            .and_then(|pm| pm.list())
            .unwrap_or_default();

        let available_games: SmallVec<[(String, String); 8]> = modde_games::supported_games()
            .iter()
            .map(|(id, name)| (id.to_string(), name.to_string()))
            .collect();

        let mut app = Self {
            active_view: View::ModList,
            active_profile: profiles.first().map(|p| p.name.clone()),
            profiles,
            status_message: "Ready".to_string(),
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
            verify: VerifyState::Idle,
            new_profile_name: String::new(),
            available_games,
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
        };

        // Auto-detect: if no game is selected but profiles exist, pick the first profile's game
        if app.selected_game.is_none() {
            if let Some(first) = app.profiles.first() {
                app.selected_game = Some(first.game_id.to_string());
                app.settings.selected_game = Some(first.game_id.to_string());
            }
        }

        // Auto-detect: if the selected game has no path in settings, try detect_install()
        if let Some(ref game_id) = app.selected_game {
            if app.settings.game_path(game_id).is_none() {
                if let Some(plugin) = modde_games::resolve_game_plugin(game_id) {
                    if let Some(path) = plugin.detect_install() {
                        app.settings.set_game_path(game_id, path);
                    }
                }
            }
            app.save_settings();
        }

        app.reload_profile();
        (app, window::oldest().map(Message::GotWindowId))
    }

    fn title(&self) -> String {
        "modde".to_string()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Navigation ───────────────────────────────────────
            Message::SwitchView(view) => match view {
                View::Saves => {
                    self.active_view = View::Saves;
                    return self.update(Message::LoadSaveHistory);
                }
                View::DataTab => {
                    self.active_view = View::DataTab;
                    self.refresh_data_tab_conflicts();
                }
                View::Tools => {
                    self.active_view = View::Tools;
                    self.refresh_tools_state();
                }
                other => {
                    self.active_view = other;
                }
            },
            Message::SwitchProfile(name) => {
                self.active_profile = Some(name);
                self.reload_profile();
                self.selected_save_details = None;
                self.status_message = "Profile switched".to_string();
            }
            Message::CreateProfile { name, game_id } => match ProfileManager::open() {
                Ok(pm) => {
                    let profile = modde_core::Profile {
                        id: None,
                        name: name.clone(),
                        game_id: modde_core::GameId::from(game_id),
                        source: modde_core::ProfileSource::Manual,
                        mods: Vec::new(),
                        overrides: PathBuf::from("overrides"),
                        load_order_rules: smallvec::SmallVec::new(),
                        load_order_lock: None,
                    };
                    match pm.create(&profile) {
                        Ok(_) => {
                            self.profiles = pm.list().unwrap_or_default();
                            self.active_profile = Some(name);
                            self.reload_profile();
                            self.new_profile_name.clear();
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
            },
            Message::DeleteProfile(name) => match ProfileManager::open() {
                Ok(pm) => match pm.delete(&name, None) {
                    Ok(()) => {
                        self.profiles = pm.list().unwrap_or_default();
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
            Message::NewProfileNameChanged(name) => self.new_profile_name = name,

            // ── Game selection ────────────────────────────────────
            Message::SelectGame(game_id) => {
                self.selected_game = Some(game_id.clone());
                self.settings.selected_game = Some(game_id);
                self.save_settings();
                self.refresh_tools_state();
            }

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
                if let Some(ref profile_name) = self.active_profile {
                    if let Ok(pm) = ProfileManager::open() {
                        if let Ok(mut profile) = pm.load(profile_name, None) {
                            if let Some(m) = profile.mods.iter_mut().find(|m| m.mod_id == mod_id) {
                                m.enabled = enabled;
                            }
                            let _ = pm
                                .create(&profile)
                                .or_else(|_| pm.update(&profile).map(|_| 0));
                            self.status_message = format!(
                                "Mod {mod_id} {}",
                                if enabled { "enabled" } else { "disabled" }
                            );
                            self.reload_profile();
                        }
                    }
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
                    let mod_name = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "unknown-mod".to_string());

                    if let Ok(pm) = ProfileManager::open() {
                        if let Ok(mut profile) = pm.load(profile_name, None) {
                            profile.mods.push(modde_core::EnabledMod {
                                mod_id: mod_name.clone(),
                                enabled: true,
                                ..Default::default()
                            });
                            let _ = pm
                                .create(&profile)
                                .or_else(|_| pm.update(&profile).map(|_| 0));
                            self.status_message = format!("Added mod: {mod_name}");
                            self.reload_profile();
                        }
                    }
                } else {
                    self.status_message = "No active profile — create one first".to_string();
                }
            }
            Message::RemoveMod(index) => {
                if let Some(ref profile_name) = self.active_profile {
                    if let Ok(pm) = ProfileManager::open() {
                        if let Ok(mut profile) = pm.load(profile_name, None) {
                            if index < profile.mods.len() {
                                let removed = profile.mods.remove(index);
                                let _ = pm
                                    .create(&profile)
                                    .or_else(|_| pm.update(&profile).map(|_| 0));
                                self.selected_mod_index = None;
                                self.status_message = format!("Removed mod: {}", removed.mod_id);
                                self.reload_profile();
                            }
                        }
                    }
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
                                api.get_mod(&game_domain, nexus_mod_id as u64)
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
                                    api.get_mod_media(&domain, nexus_mod_id as u64)
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
                                    let target = nexus_mod_id as u64;
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
                        |_| Message::Noop,
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
                            api.abstain_mod(&game_domain, nexus_mod_id as u64, &version)
                                .await
                                .map_err(|e| e.to_string())
                        } else {
                            api.endorse_mod(&game_domain, nexus_mod_id as u64, &version)
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
                            api.untrack_mod(&game_domain, nexus_mod_id as u64)
                                .await
                                .map_err(|e| e.to_string())
                        } else {
                            api.track_mod(&game_domain, nexus_mod_id as u64)
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
                if let Some(ref mut s) = self.selected_mod_details {
                    if s.nexus_mod_id == nexus_mod_id {
                        s.is_tracked = Some(is_tracked);
                    }
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
                                let game_plugin = modde_games::resolve_game_plugin(&game_id)
                                    .ok_or_else(|| format!("unsupported game: {game_id}"))?;
                                let install_path =
                                    game_plugin.detect_install().ok_or_else(|| {
                                        format!("could not detect install for {game_id}")
                                    })?;
                                let mod_dir = game_plugin.mod_directory(&install_path);
                                let staging_dir = ProfileManager::staging_dir(&profile.name);
                                game_plugin
                                    .deploy(&staging_dir, &mod_dir)
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
                            .or_else(|_| pm.update(&profile).map(|_| 0));
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
                self.browse_nexus.active_tab = tab;
                self.browse_nexus.error = None;
                let domain = match self.current_game_nexus_domain() {
                    Some(d) => d,
                    None => return Task::none(),
                };
                return self.spawn_browse_load(tab, domain, self.browse_nexus.search_query.clone());
            }
            Message::BrowseSearchChanged(query) => {
                self.browse_nexus.search_query = query;
            }
            Message::BrowseSearchSubmit => {
                self.browse_nexus.active_tab = crate::views::browse_nexus::BrowseTab::Search;
                self.browse_nexus.error = None;
                let domain = match self.current_game_nexus_domain() {
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
                    if let Some(task_id) = self.download_lookup.get(&download_key).copied() {
                        if let Some(task) = self.download_queue.get_mut(task_id) {
                            task.state = modde_sources::queue::DownloadState::Complete {
                                path: task.dest.clone(),
                                hash: 0,
                            };
                            task.meta.status = "complete".to_string();
                        }
                    }
                    self.reload_profile();
                }
                Err(e) => {
                    self.browse_nexus.install_status = Some(format!("Install failed: {e}"));
                    self.status_message = format!("Install failed: {e}");
                    if let Some(task_id) = self.download_lookup.get(&download_key).copied() {
                        if let Some(task) = self.download_queue.get_mut(task_id) {
                            task.state =
                                modde_sources::queue::DownloadState::Failed { error: e.clone() };
                            task.meta.status = "failed".to_string();
                        }
                    }
                }
            },

            // ── Wabbajack ────────────────────────────────────────
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
                let manifest = (|| -> Option<modde_core::WabbajackManifest> {
                    let file = std::fs::File::open(&path).ok()?;
                    let mut archive = zip::ZipArchive::new(file).ok()?;
                    let name = if archive.file_names().any(|n| n == "modlist.json") {
                        "modlist.json"
                    } else {
                        "modlist"
                    };
                    let mut entry = archive.by_name(name).ok()?;
                    let mut buf = String::new();
                    std::io::Read::read_to_string(&mut entry, &mut buf).ok()?;
                    serde_json::from_str(&buf).ok()
                })();
                self.wabbajack_manifest = manifest;
                self.active_view = View::WabbajackInstaller(WabbajackInstallerState {
                    file_path: Some(path.clone()),
                    progress: 0.0,
                    status: format!("Selected: {}", path.display()),
                    log_lines: vec![format!("File selected: {}", path.display())],
                });
                self.status_message = format!("Wabbajack file loaded: {}", path.display());
            }
            Message::WabbajackProgress(progress) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.progress = progress;
                    state.status = format!("{:.0}% complete", progress * 100.0);
                }
            }
            Message::WabbajackStartInstall => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    if let Some(ref wj_path) = state.file_path {
                        state.status = "Starting installation...".to_string();
                        state.log_lines.push("Installation started".to_string());
                        state.progress = 0.0;
                        let path = wj_path.clone();
                        return Task::perform(
                            async move {
                                let file = std::fs::File::open(&path)?;
                                let mut archive = zip::ZipArchive::new(file)?;
                                let manifest: modde_core::WabbajackManifest = {
                                    let name = if archive.file_names().any(|n| n == "modlist.json")
                                    {
                                        "modlist.json"
                                    } else {
                                        "modlist"
                                    };
                                    let mut entry = archive.by_name(name)?;
                                    let mut buf = String::new();
                                    std::io::Read::read_to_string(&mut entry, &mut buf)?;
                                    serde_json::from_str(&buf)?
                                };
                                Ok::<String, anyhow::Error>(manifest.name)
                            },
                            |result: Result<String, anyhow::Error>| match result {
                                Ok(name) => {
                                    Message::WabbajackLog(format!("Parsed manifest: {name}"))
                                }
                                Err(e) => Message::WabbajackLog(format!("Error: {e}")),
                            },
                        );
                    }
                }
                self.status_message = "No wabbajack file selected".to_string();
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
                        Some(fomod_oxide::config::GroupType::SelectExactlyOne)
                        | Some(fomod_oxide::config::GroupType::SelectAtMostOne) => {
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
                                "FOMOD installation completed successfully".to_string()
                        }
                        Err(e) => self.status_message = format!("FOMOD installation failed: {e}"),
                    }
                    self.reset_fomod();
                    self.active_view = View::ModList;
                    return Task::done(Message::FOMODInstallComplete(result));
                } else {
                    if let Some(ref mut installer) = self.fomod_installer {
                        installer.checkpoint();
                        self.fomod_can_undo = true;
                    }
                    self.fomod_wizard_pos += 1;
                }
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
                    .map(|i| i.rollback())
                    .unwrap_or(false);
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
                if let Some(task_id) = self.download_lookup.get(&id).copied() {
                    if let Some(task) = self.download_queue.get_mut(task_id) {
                        task.meta.status = "complete".to_string();
                        task.state = modde_sources::queue::DownloadState::Complete {
                            path: task.dest.clone(),
                            hash: task.expected_hash.unwrap_or(0),
                        };
                    }
                }
                self.status_message = format!("Download complete: {id}");
            }
            Message::DownloadFailed { id, error } => {
                if let Some(task_id) = self.download_lookup.get(&id).copied() {
                    if let Some(task) = self.download_queue.get_mut(task_id) {
                        task.meta.status = "failed".to_string();
                        task.state = modde_sources::queue::DownloadState::Failed {
                            error: error.clone(),
                        };
                    }
                }
                self.status_message = format!("Download failed ({id}): {error}");
            }

            // ── Settings ─────────────────────────────────────────
            Message::SetNexusApiKey(key) => {
                self.settings.nexus_api_key = key;
                self.status_message = "Nexus API key updated".to_string();
                self.save_settings();
            }
            Message::SetGamePath { game_id, path } => {
                self.settings.set_game_path(&game_id, path);
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
                let api_key = self.settings.nexus_api_key.clone();
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
                                let game_plugin = modde_games::resolve_game_plugin(&game_id)
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
                } else {
                    self.status_message = "No active profile".to_string();
                }
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
                                let game_plugin = modde_games::resolve_game_plugin(&game_id)
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
                            let save_dir = modde_games::resolve_game_plugin(&game_id)
                                .and_then(|g| g.save_directory());
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
                            let save_dir = modde_games::resolve_game_plugin(&game_id)
                                .and_then(|g| g.save_directory());
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
                    let save_dir =
                        modde_games::resolve_game_plugin(&game_id).and_then(|g| g.save_directory());
                    if let Some(save_dir) = save_dir {
                        match modde_core::save::SaveManager::restore(
                            &game_id,
                            &profile_name,
                            &commit_id,
                            &save_dir,
                        ) {
                            Ok(count) => {
                                self.status_message = format!("Restored {count} save file(s)")
                            }
                            Err(e) => self.status_message = format!("Restore failed: {e}"),
                        }
                    } else {
                        self.status_message =
                            "Cannot detect save directory for this game".to_string();
                    }
                }
            }

            // ── Verification ─────────────────────────────────────
            Message::RunVerify => {
                self.verify = VerifyState::Running;
                self.status_message = "Running verification...".to_string();
                if let Some(ref profile) = self.loaded_profile {
                    let profile_name = profile.name.clone();
                    let staging_dir = ProfileManager::staging_dir(&profile_name);
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || -> VerifyResults {
                                let mut results = VerifyResults {
                                    missing_mods: SmallVec::new(),
                                    hash_mismatches: Vec::new(),
                                    broken_symlinks: SmallVec::new(),
                                    ok_count: 0,
                                };
                                if !staging_dir.exists() {
                                    return results;
                                }
                                fn walk(dir: &std::path::Path, results: &mut VerifyResults) {
                                    if let Ok(entries) = std::fs::read_dir(dir) {
                                        for entry in entries.flatten() {
                                            let path = entry.path();
                                            if path.is_dir() {
                                                walk(&path, results);
                                            } else if path.is_symlink() {
                                                match std::fs::read_link(&path) {
                                                    Ok(target) if target.exists() => {
                                                        results.ok_count += 1
                                                    }
                                                    _ => results.broken_symlinks.push(path),
                                                }
                                            } else {
                                                results.ok_count += 1;
                                            }
                                        }
                                    }
                                }
                                walk(&staging_dir, &mut results);
                                results
                            })
                            .await
                            .unwrap_or(VerifyResults {
                                missing_mods: SmallVec::new(),
                                hash_mismatches: Vec::new(),
                                broken_symlinks: SmallVec::new(),
                                ok_count: 0,
                            })
                        },
                        Message::VerifyComplete,
                    );
                }
                self.verify = VerifyState::Idle;
            }
            Message::VerifyComplete(results) => {
                let ok = results.ok_count;
                let broken = results.broken_symlinks.len();
                self.status_message = format!("Verify: {ok} OK, {broken} broken symlink(s)");
                self.verify = VerifyState::Complete(results);
                self.active_view = View::Verify;
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
            Message::RefreshTools => {
                self.refresh_tools_state();
                self.status_message = if self.tool_state.entries.is_empty() {
                    "No tool state available for the current game".to_string()
                } else {
                    format!("Loaded {} tool(s)", self.tool_state.entries.len())
                };
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
                match modde_core::db::ModdeDb::open() {
                    Ok(db) => {
                        let settings_json = db
                            .load_tool_config(&game_id, &tool_id)
                            .ok()
                            .flatten()
                            .map(|row| row.settings_json)
                            .unwrap_or_else(|| {
                                serde_json::to_string(&tool.default_config().settings)
                                    .unwrap_or_else(|_| "{}".to_string())
                            });
                        match db.save_tool_config(&game_id, &tool_id, enabled, &settings_json) {
                            Ok(()) => {
                                let _ = modde_games::launcher::generate_tool_configs(&game_id, &db);
                                self.refresh_tools_state();
                                self.status_message = format!(
                                    "{} {}",
                                    tool.display_name(),
                                    if enabled { "enabled" } else { "disabled" }
                                );
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
            Message::ApplyTool(id) => {
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
                match modde_core::db::ModdeDb::open() {
                    Ok(db) => {
                        let row = db.load_tool_config(&game_id, &id).ok().flatten();
                        let mut config = row
                            .map(|row| modde_games::tools::ToolConfig {
                                tool_id: row.tool_id,
                                enabled: row.enabled,
                                settings: serde_json::from_str(&row.settings_json)
                                    .unwrap_or_default(),
                            })
                            .unwrap_or_else(|| tool.default_config());
                        config.enabled = true;
                        config.set("_game_id", serde_json::json!(game_id));
                        match tool.apply(&game_dir, &config) {
                            Ok(applied) => {
                                let paths: Vec<String> = applied
                                    .files
                                    .iter()
                                    .map(|path| path.to_string_lossy().replace('\\', "/"))
                                    .collect();
                                let settings_json = serde_json::to_string(&config.settings)
                                    .unwrap_or_else(|_| "{}".to_string());
                                let _ = db.save_tool_config(&game_id, &id, true, &settings_json);
                                let _ = db.clear_applied_files(&game_id, &id);
                                let _ = db.save_applied_files(&game_id, &id, &paths);
                                let _ = modde_games::launcher::generate_tool_configs(&game_id, &db);
                                self.refresh_tools_state();
                                self.status_message = format!(
                                    "Applied {} ({} file(s))",
                                    tool.display_name(),
                                    paths.len()
                                );
                            }
                            Err(err) => {
                                self.status_message = format!("Failed to apply tool: {err}");
                            }
                        }
                    }
                    Err(err) => {
                        self.status_message = format!("Failed to open tool database: {err}");
                    }
                }
            }
            Message::RevertTool(id) => {
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
                match modde_core::db::ModdeDb::open() {
                    Ok(db) => {
                        let applied_paths =
                            db.load_applied_files(&game_id, &id).unwrap_or_default();
                        let applied = modde_games::tools::AppliedFiles {
                            files: applied_paths.iter().map(PathBuf::from).collect(),
                        };
                        match tool.revert(&game_dir, &applied) {
                            Ok(()) => {
                                let _ = db.clear_applied_files(&game_id, &id);
                                self.refresh_tools_state();
                                self.status_message = format!("Reverted {}", tool.display_name());
                            }
                            Err(err) => {
                                self.status_message = format!("Failed to revert tool: {err}");
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

        let sidebar = crate::views::sidebar::view(
            &self.active_view,
            &self.profiles,
            &self.active_profile,
            self.experiment_depth,
            &self.new_profile_name,
            &self.selected_game,
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
                // Resolve the game domain here so the view can render
                // an empty state when no profile/game is loaded yet.
                // Pass by value so the Element's lifetime isn't tied
                // to this local string.
                let domain = self.current_game_nexus_domain();
                crate::views::browse_nexus::view(&self.browse_nexus, domain)
            }
            View::FOMODWizard(_) => crate::views::fomod_wizard::view(self),
            View::Settings => crate::views::settings::view(settings_state),
            View::WabbajackInstaller(state) => {
                crate::views::wabbajack::view(state, &self.wabbajack_manifest)
            }
            View::Saves => crate::views::saves::view(
                &self.save_snapshots,
                self.loaded_profile.as_ref().map(|p| p.name.as_str()),
                self.current_fingerprint.as_ref(),
                self.selected_save_details
                    .as_ref()
                    .map(|d| d.commit_id.as_str()),
            ),
            View::Verify => crate::views::verify::view(&self.verify),
            View::Downloads => {
                let tasks = self.downloads_view_tasks();
                crate::views::downloads::view(&tasks)
            }
            View::DataTab => {
                crate::views::data_tab::view(&self.data_tab_state, &self.data_tab_conflicts)
            }
            View::Diagnostics => crate::views::diagnostics::view(&self.diagnostics_state),
            View::Tools => crate::views::tools::view(&self.tool_state),
        };

        // ── Custom title bar ──
        let game_names: Vec<String> = self
            .available_games
            .iter()
            .map(|(_, name)| name.clone())
            .collect();
        let selected_game_display = self.selected_game.as_ref().and_then(|id| {
            self.available_games
                .iter()
                .find(|(gid, _)| gid == id)
                .map(|(_, name)| name.clone())
        });
        let available_games = self.available_games.clone();
        let game_picker = pick_list(game_names, selected_game_display, move |name: String| {
            let game_id = available_games
                .iter()
                .find(|(_, n)| *n == name)
                .map(|(id, _)| id.clone())
                .unwrap_or(name);
            Message::SelectGame(game_id)
        })
        .placeholder("Select a game")
        .width(Length::Fixed(200.0));

        let title_label = text("modde").size(14);

        let window_controls = row![
            button(text("\u{2212}").size(12))
                .on_press(Message::WindowMinimize)
                .style(button::secondary)
                .padding([2, 10]),
            button(text("\u{25A1}").size(12))
                .on_press(Message::WindowToggleMaximize)
                .style(button::secondary)
                .padding([2, 10]),
            button(text("\u{2715}").size(12))
                .on_press(Message::WindowClose)
                .style(button::danger)
                .padding([2, 10]),
        ]
        .spacing(2);

        let title_bar_content = row![
            game_picker,
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

        let main_layout = column![
            title_bar,
            row![sidebar, content].spacing(0).height(Length::Fill),
            status_bar,
        ]
        .spacing(0);

        let base: Element<Message> = container(main_layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        base
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

    /// Route widget-ignored keyboard events through the shortcuts
    /// module. `keyboard::listen()` yields only events that no focused
    /// widget consumed — so text inputs keep their keys while global
    /// shortcuts fire on unhandled presses. The closure must be
    /// non-capturing (a `Subscription::filter_map` constraint), so it
    /// calls only free functions and cannot read `&self`.
    fn subscription(&self) -> Subscription<Message> {
        keyboard::listen().filter_map(|event| match event {
            keyboard::Event::KeyPressed { key, modifiers, .. } => {
                let action = crate::shortcuts::match_shortcut(&key, modifiers)?;
                shortcut_action_to_message(action)
            }
            _ => None,
        })
    }
}

/// Map a matched shortcut action string to the corresponding
/// `Message`. Returns `None` for actions whose handlers aren't wired
/// yet — those shortcuts still register in `all_shortcuts()` for
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

/// help-text purposes but produce no messages until their handlers
/// exist (most need state-aware lookups or new `Message` variants).
fn shortcut_action_to_message(action: &str) -> Option<Message> {
    match action {
        "deploy" => Some(Message::Deploy),
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
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_app() -> Modde {
        Modde {
            active_view: View::ModList,
            active_profile: None,
            profiles: Vec::new(),
            status_message: "Ready".to_string(),
            settings: AppSettings::default(),
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
            theme_name: "Dark".to_string(),
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
            verify: VerifyState::Idle,
            new_profile_name: String::new(),
            available_games: smallvec::smallvec![(
                "skyrim-se".to_string(),
                "Skyrim SE".to_string()
            )],
            selected_game: None,
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
        }
    }

    #[test]
    fn test_initial_state() {
        let app = test_app();
        assert!(matches!(app.active_view, View::ModList));
        assert!(app.active_profile.is_none());
        assert_eq!(app.status_message, "Ready");
        assert_eq!(app.theme_name, "Dark");
        assert!(app.fomod_installer.is_none());
        assert_eq!(app.experiment_depth, 0);
        assert!(matches!(app.verify, VerifyState::Idle));
    }

    #[test]
    fn test_title() {
        let app = test_app();
        assert_eq!(app.title(), "modde");
    }

    #[test]
    fn test_switch_view_settings() {
        let mut app = test_app();
        let _ = app.update(Message::SwitchView(View::Settings));
        assert!(matches!(app.active_view, View::Settings));
    }

    #[test]
    fn test_switch_view_saves() {
        let mut app = test_app();
        let _ = app.update(Message::SwitchView(View::Saves));
        assert!(matches!(app.active_view, View::Saves));
    }

    #[test]
    fn test_switch_view_verify() {
        let mut app = test_app();
        let _ = app.update(Message::SwitchView(View::Verify));
        assert!(matches!(app.active_view, View::Verify));
    }

    #[test]
    fn test_switch_profile() {
        let mut app = test_app();
        let _ = app.update(Message::SwitchProfile("test-profile".to_string()));
        assert_eq!(app.active_profile.as_deref(), Some("test-profile"));
        assert_eq!(app.status_message, "Profile switched");
    }

    #[test]
    fn test_filter_changed() {
        let mut app = test_app();
        let _ = app.update(Message::FilterChanged("skyui".to_string()));
        assert_eq!(app.mod_filter, "skyui");
    }

    #[test]
    fn test_select_mod() {
        let mut app = test_app();
        let _ = app.update(Message::SelectMod(3));
        assert_eq!(app.selected_mod_index, Some(3));
    }

    #[test]
    fn test_deploy_complete_ok() {
        let mut app = test_app();
        let _ = app.update(Message::DeployComplete(Ok("Deployed 5 mods".to_string())));
        assert!(app.status_message.contains("Deployed"));
    }

    #[test]
    fn test_deploy_complete_err() {
        let mut app = test_app();
        let _ = app.update(Message::DeployComplete(Err("game not found".to_string())));
        assert!(app.status_message.contains("Deploy failed"));
    }

    #[test]
    fn test_set_nexus_api_key() {
        let mut app = test_app();
        let _ = app.update(Message::SetNexusApiKey("abc123".to_string()));
        assert_eq!(app.settings.nexus_api_key, "abc123");
    }

    #[test]
    fn test_set_theme() {
        let mut app = test_app();
        let _ = app.update(Message::SetTheme("Nord".to_string()));
        assert_eq!(app.theme_name, "Nord");
        assert_eq!(app.settings.theme, "Nord");
    }

    #[test]
    fn test_theme_returns_correct_variant() {
        let mut app = test_app();
        assert_eq!(app.theme(), Theme::Dark);
        app.theme_name = "Light".to_string();
        assert_eq!(app.theme(), Theme::Light);
        app.theme_name = "Nord".to_string();
        assert_eq!(app.theme(), Theme::Nord);
        app.theme_name = "Dracula".to_string();
        assert_eq!(app.theme(), Theme::Dracula);
        app.theme_name = "Gruvbox Dark".to_string();
        assert_eq!(app.theme(), Theme::GruvboxDark);
        app.theme_name = "Catppuccin Mocha".to_string();
        assert_eq!(app.theme(), Theme::CatppuccinMocha);
    }

    #[test]
    fn test_new_profile_name_changed() {
        let mut app = test_app();
        let _ = app.update(Message::NewProfileNameChanged("my-profile".to_string()));
        assert_eq!(app.new_profile_name, "my-profile");
    }

    #[test]
    fn test_select_game() {
        let mut app = test_app();
        let _ = app.update(Message::SelectGame("cyberpunk2077".to_string()));
        assert_eq!(app.selected_game, Some("cyberpunk2077".to_string()));
    }

    #[test]
    fn test_verify_complete() {
        let mut app = test_app();
        let results = VerifyResults {
            missing_mods: SmallVec::new(),
            hash_mismatches: vec![],
            broken_symlinks: smallvec::smallvec![PathBuf::from("/broken")],
            ok_count: 42,
        };
        let _ = app.update(Message::VerifyComplete(results));
        assert!(matches!(app.active_view, View::Verify));
        assert!(matches!(app.verify, VerifyState::Complete(_)));
        assert!(app.status_message.contains("42 OK"));
    }

    #[test]
    fn test_noop() {
        let mut app = test_app();
        let old = app.status_message.clone();
        let _ = app.update(Message::Noop);
        assert_eq!(app.status_message, old);
    }

    #[test]
    fn test_shortcut_action_to_message_maps_deploy() {
        assert!(matches!(
            shortcut_action_to_message("deploy"),
            Some(Message::Deploy)
        ));
    }

    #[test]
    fn test_shortcut_action_to_message_drops_unmapped() {
        assert!(shortcut_action_to_message("refresh").is_none());
        assert!(shortcut_action_to_message("nonexistent").is_none());
    }

    #[test]
    fn test_fomod_cancel() {
        let mut app = test_app();
        app.fomod_installer = Some(FOMODWizardState::new());
        app.active_view = View::FOMODWizard(FOMODWizardState::new());
        let _ = app.update(Message::FOMODCancel);
        assert!(matches!(app.active_view, View::ModList));
        assert!(app.status_message.contains("cancelled"));
    }

    #[test]
    fn test_fomod_back() {
        let mut app = test_app();
        app.fomod_wizard_pos = 2;
        let _ = app.update(Message::FOMODBack);
        assert_eq!(app.fomod_wizard_pos, 1);
    }

    #[test]
    fn test_reset_fomod() {
        let mut app = test_app();
        app.fomod_installer = Some(FOMODWizardState::new());
        app.fomod_source_dir = Some(PathBuf::from("/src"));
        app.fomod_wizard_pos = 1;
        app.fomod_can_undo = true;
        app.reset_fomod();
        assert!(app.fomod_installer.is_none());
        assert_eq!(app.fomod_wizard_pos, 0);
        assert!(!app.fomod_can_undo);
    }

    // ── UI handler lock refusal tests (DB-isolated) ──────────────
    //
    // These exercise the DB-touching branches of `Message::ReorderMod`
    // / `LockMod` / `UnlockMod`.
    //
    // DB isolation is a process-wide `OnceLock<TempDir>` via
    // `modde_core::paths::set_data_dir` — the same pattern used by
    // `crates/modde-core/tests/load_order_lock_tests.rs`. All tests in
    // this module share one tempdir; collision avoidance is by unique
    // profile names (so there's no mutable-shared-state risk). See
    // `/home/can/.claude/plans/greedy-shimmying-pine.md` for the full
    // plan.

    use modde_core::profile::{LoadOrderLock, LockReason, ProfileManager, ProfileSource};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static ISOLATED_DATA_DIR: OnceLock<tempfile::TempDir> = OnceLock::new();

    /// Process-wide test mutex. All lock-refusal tests share one
    /// file-backed SQLite DB (via the `ISOLATED_DATA_DIR` OnceLock) and
    /// open a fresh `ProfileManager` connection per test, which runs
    /// [`ModdeDb::migrate`](../../../../modde-core/src/db.rs) on every
    /// open. The V2 migration is **not idempotent** under parallel
    /// writes (`ALTER TABLE ... ADD COLUMN` with no guard race on first
    /// open) — so we serialize tests that touch the isolated DB.
    ///
    /// modde-core's own lock tests don't hit this because they use
    /// `ModdeDb::open_memory()` (one fresh in-memory DB per test). We
    /// can't do that here because the UI handlers call
    /// `ProfileManager::open()` internally — no injection point for an
    /// in-memory DB.
    static DB_LOCK: Mutex<()> = Mutex::new(());

    /// Acquire the serial lock. Discards poisoning (a prior panicking
    /// test shouldn't block the rest of the suite).
    fn db_lock() -> MutexGuard<'static, ()> {
        DB_LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Redirect `modde_core::paths::modde_data_dir` to a per-process
    /// tempdir so lock-refusal tests don't touch `~/.local/share/modde`.
    /// `OnceLock::get_or_init` makes this safe (idempotent) on every call.
    fn isolated_data_dir() {
        ISOLATED_DATA_DIR.get_or_init(|| {
            let dir = tempfile::tempdir().expect("create isolated modde data dir");
            modde_core::paths::set_data_dir(dir.path().to_path_buf());
            dir
        });
    }

    /// Build an `EnabledMod` for the refusal-test fixtures. `lock` controls
    /// the per-mod pin — `None` for a normal entry, `Some(reason)` to pin.
    fn seed_mod(id: &str, lock: Option<LockReason>) -> modde_core::profile::EnabledMod {
        modde_core::profile::EnabledMod {
            mod_id: id.to_string(),
            display_name: Some(id.to_string()),
            enabled: true,
            lock,
            ..Default::default()
        }
    }

    /// Write a profile directly to the isolated DB. Profile names must be
    /// unique across all tests in this module (they share the OnceLock
    /// tempdir). The fake `game_id = "test-game"` is deliberate — it makes
    /// `modde_games::resolve_game_plugin` return `None`, which in turn
    /// makes `reload_profile`'s fingerprint block short-circuit, avoiding
    /// a walk of the non-existent staging dir.
    fn seed_profile(
        name: &str,
        mods: Vec<modde_core::profile::EnabledMod>,
        lock: Option<LoadOrderLock>,
    ) {
        isolated_data_dir();
        let pm = ProfileManager::open().expect("open isolated DB");
        let profile = modde_core::profile::Profile {
            id: None,
            name: name.to_string(),
            game_id: modde_core::GameId::from("test-game"),
            source: ProfileSource::Manual,
            mods,
            overrides: PathBuf::from("/tmp/overrides"),
            load_order_rules: SmallVec::new(),
            load_order_lock: lock,
        };
        pm.create(&profile).expect("seed profile");
    }

    /// Build a `Modde` with `active_profile` set to the seeded profile
    /// and `loaded_profile` populated via `reload_profile` (reading from
    /// the isolated DB).
    fn loaded_test_app(name: &str) -> Modde {
        let mut app = test_app();
        app.active_profile = Some(name.to_string());
        app.reload_profile();
        app
    }

    /// Read the profile back from the isolated DB. Assertions should use
    /// this (not `app.loaded_profile`) to verify *persisted* state.
    fn reload_seeded(name: &str) -> modde_core::profile::Profile {
        let pm = ProfileManager::open().expect("open isolated DB");
        pm.load(name, Some("test-game"))
            .expect("load seeded profile")
    }

    /// Shorthand: return the `mod_id`s of a profile in current order.
    fn mod_ids(profile: &modde_core::profile::Profile) -> Vec<&str> {
        profile.mods.iter().map(|m| m.mod_id.as_str()).collect()
    }

    // ─── ReorderMod refusal / allow paths ────────────────────────

    #[test]
    fn reorder_refused_when_profile_wabbajack_locked() {
        let _guard = db_lock();
        seed_profile(
            "reorder_refuse_wabbajack",
            vec![
                seed_mod("a", None),
                seed_mod("b", None),
                seed_mod("c", None),
            ],
            Some(LoadOrderLock::now(LockReason::Wabbajack {
                manifest_hash: "deadbeef".to_string(),
            })),
        );
        let mut app = loaded_test_app("reorder_refuse_wabbajack");
        let _ = app.update(Message::ReorderMod {
            mod_id: "a".to_string(),
            direction: ReorderDirection::Down,
        });
        let persisted = reload_seeded("reorder_refuse_wabbajack");
        assert_eq!(
            mod_ids(&persisted),
            vec!["a", "b", "c"],
            "order must be unchanged when profile is Wabbajack-locked"
        );
        assert!(
            app.status_message.contains("locked by Wabbajack"),
            "status message should name the lock reason, got: {}",
            app.status_message
        );
    }

    #[test]
    fn reorder_refused_when_target_mod_pinned() {
        let _guard = db_lock();
        seed_profile(
            "reorder_refuse_target_pinned",
            vec![
                seed_mod("a", None),
                seed_mod("b", Some(LockReason::Manual { note: None })),
                seed_mod("c", None),
            ],
            None,
        );
        let mut app = loaded_test_app("reorder_refuse_target_pinned");
        let _ = app.update(Message::ReorderMod {
            mod_id: "b".to_string(),
            direction: ReorderDirection::Up,
        });
        let persisted = reload_seeded("reorder_refuse_target_pinned");
        assert_eq!(mod_ids(&persisted), vec!["a", "b", "c"]);
        assert!(
            app.status_message.contains("pinned"),
            "status message should mention the pin, got: {}",
            app.status_message
        );
    }

    #[test]
    fn reorder_refused_when_swap_partner_pinned() {
        let _guard = db_lock();
        seed_profile(
            "reorder_refuse_partner_pinned",
            vec![
                seed_mod("a", None),
                seed_mod("b", None),
                seed_mod("c", Some(LockReason::Manual { note: None })),
            ],
            None,
        );
        let mut app = loaded_test_app("reorder_refuse_partner_pinned");
        // Move b down → swap partner is c (pinned) → should refuse.
        let _ = app.update(Message::ReorderMod {
            mod_id: "b".to_string(),
            direction: ReorderDirection::Down,
        });
        let persisted = reload_seeded("reorder_refuse_partner_pinned");
        assert_eq!(mod_ids(&persisted), vec!["a", "b", "c"]);
        assert!(
            app.status_message.contains("Cannot move past a pinned mod"),
            "status message should explain the adjacent pin, got: {}",
            app.status_message
        );
    }

    #[test]
    fn reorder_allowed_when_unlocked_moves_up() {
        let _guard = db_lock();
        seed_profile(
            "reorder_allow_up",
            vec![
                seed_mod("a", None),
                seed_mod("b", None),
                seed_mod("c", None),
            ],
            None,
        );
        let mut app = loaded_test_app("reorder_allow_up");
        // Move b up → swap with a → [b, a, c].
        let _ = app.update(Message::ReorderMod {
            mod_id: "b".to_string(),
            direction: ReorderDirection::Up,
        });
        let persisted = reload_seeded("reorder_allow_up");
        assert_eq!(mod_ids(&persisted), vec!["b", "a", "c"]);
        assert!(
            app.status_message.contains("up"),
            "status should confirm the upward move, got: {}",
            app.status_message
        );
    }

    #[test]
    fn reorder_allowed_when_unlocked_moves_down() {
        let _guard = db_lock();
        seed_profile(
            "reorder_allow_down",
            vec![
                seed_mod("a", None),
                seed_mod("b", None),
                seed_mod("c", None),
            ],
            None,
        );
        let mut app = loaded_test_app("reorder_allow_down");
        // Move a down → swap with b → [b, a, c].
        let _ = app.update(Message::ReorderMod {
            mod_id: "a".to_string(),
            direction: ReorderDirection::Down,
        });
        let persisted = reload_seeded("reorder_allow_down");
        assert_eq!(mod_ids(&persisted), vec!["b", "a", "c"]);
        assert!(
            app.status_message.contains("down"),
            "status should confirm the downward move, got: {}",
            app.status_message
        );
    }

    #[test]
    fn reorder_noop_at_top_edge() {
        let _guard = db_lock();
        seed_profile(
            "reorder_noop_edge",
            vec![
                seed_mod("a", None),
                seed_mod("b", None),
                seed_mod("c", None),
            ],
            None,
        );
        let mut app = loaded_test_app("reorder_noop_edge");
        let status_before = app.status_message.clone();
        let _ = app.update(Message::ReorderMod {
            mod_id: "a".to_string(),
            direction: ReorderDirection::Up,
        });
        let persisted = reload_seeded("reorder_noop_edge");
        assert_eq!(
            mod_ids(&persisted),
            vec!["a", "b", "c"],
            "order unchanged at top boundary"
        );
        // The handler silently short-circuits on `AtBoundary` — status
        // message is not rewritten. Contract documented at
        // `Message::ReorderMod` handler.
        assert_eq!(
            app.status_message, status_before,
            "AtBoundary should not mutate the status message"
        );
    }

    // ─── LockMod / UnlockMod (per-mod pins) ──────────────────────

    #[test]
    fn lock_mod_sets_per_mod_lock() {
        let _guard = db_lock();
        seed_profile(
            "lock_mod_sets_pin",
            vec![
                seed_mod("a", None),
                seed_mod("b", None),
                seed_mod("c", None),
            ],
            None,
        );
        let mut app = loaded_test_app("lock_mod_sets_pin");
        let _ = app.update(Message::LockMod {
            mod_id: "b".to_string(),
        });
        let persisted = reload_seeded("lock_mod_sets_pin");
        assert!(
            matches!(
                persisted.mods[1].lock,
                Some(LockReason::Manual { note: None })
            ),
            "mod 'b' should be pinned with Manual reason, got {:?}",
            persisted.mods[1].lock
        );
        // Other mods untouched.
        assert!(persisted.mods[0].lock.is_none());
        assert!(persisted.mods[2].lock.is_none());
    }

    #[test]
    fn unlock_mod_clears_per_mod_lock() {
        let _guard = db_lock();
        seed_profile(
            "unlock_mod_clears_pin",
            vec![
                seed_mod("a", None),
                seed_mod("b", Some(LockReason::Manual { note: None })),
                seed_mod("c", None),
            ],
            None,
        );
        let mut app = loaded_test_app("unlock_mod_clears_pin");
        let _ = app.update(Message::UnlockMod {
            mod_id: "b".to_string(),
        });
        let persisted = reload_seeded("unlock_mod_clears_pin");
        assert!(
            persisted.mods[1].lock.is_none(),
            "mod 'b' pin should be cleared"
        );
    }
}
