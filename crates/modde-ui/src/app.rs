use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

#[cfg(test)]
use iced::Theme;
use iced::window;
use smallvec::SmallVec;

use modde_core::filter::{FilterCriterion, FilterKind, FilterMode};
use modde_core::manifest::collection::CollectionManifest;
use modde_core::profile::ProfileManager;
#[cfg(test)]
use modde_core::resolver::GameId;
use modde_core::save::SaveSnapshot;
use modde_core::settings::AppSettings;

mod fomod_wizard_state;
mod install_ops;
mod model;
mod state;
mod tool_ops;
mod tool_settings;
mod update;
mod view;

/// Drive an async database future to completion from iced's synchronous
/// `update`/`view` paths.
///
/// modde's storage layer is async (`sqlx`), but iced's `update(&mut self, …)`
/// handler cannot `.await`. This bridges the two: the future is driven on a
/// dedicated multi-threaded runtime, executed on a freshly spawned scoped
/// thread so `block_on` never runs inside iced's own ambient runtime (which
/// would panic with "Cannot start a runtime from within a runtime"). The scope
/// lets the future borrow local data; the runtime is shared across calls.
///
/// Prefer `.await` in code that is already async (e.g. `Task::perform`
/// closures); use this only on the synchronous `update`/`view`/`new` paths and
/// inside `spawn_blocking` closures.
pub(crate) fn block_on<F>(future: F) -> F::Output
where
    F: std::future::Future,
{
    use std::sync::OnceLock;
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    let rt = RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to build modde-ui database runtime")
    });
    // Enter the shared runtime so sqlx (which needs the tokio reactor/timer)
    // has a handle, then drive the future to completion on *this* thread with
    // a current-thread executor. Driving on the current thread sidesteps the
    // "Cannot start a runtime from within a runtime" panic that `Runtime::block_on`
    // would hit on iced's ambient thread, and avoids requiring the future to be
    // `Send` (sqlx connection futures are not `Send` under a higher-ranked
    // bound), so it works for futures that borrow `&db`/`&self`.
    let _guard = rt.enter();
    futures::executor::block_on(future)
}

pub use self::fomod_wizard_state::FOMODWizardState;
pub(crate) use self::state::format_lock_reason;
pub use self::state::{
    AddCustomGameDraft, AddCustomGameDraftField, AddCustomGameState, DataTabConflicts,
    ExecutableDraft, ExecutableDraftField, ExecutableUiEntry, ProfileContextSnapshot,
    ProfileLoadOutcome, ReorderDirection, SidebarGroup, ToolApplyResult, ToolHistoryUiEntry,
    ToolLoadSnapshot, ToolReleaseSupport, ToolRevertResult, ToolState, ToolUiEntry, View,
    WabbajackInstallerState, WabbajackTab,
};
pub use self::tool_ops::parse_executable_environment;
#[cfg(test)]
use self::tool_ops::{apply_tool_for_game, validate_optiscaler_apply};
#[cfg(test)]
use self::tool_settings::{
    get_tool_setting_value, normalize_tool_settings_for_specs, set_nested_tool_setting,
    tool_apply_is_pending, tool_apply_signature,
};

pub type ToolOptionCatalog = HashMap<String, Vec<String>>;

const BUTTON_HOVER_TOAST_DELAY: Duration = Duration::from_secs(2);
// The delayed hover-toast lifecycle is still owned by Modde: app::update
// schedules Message::ButtonHoverElapsed after this delay.

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
    pub(crate) db: modde_core::db::ModdeDb,
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
    /// Generation guard for the async profile/game-context load. Bumped on
    /// every `Message::ProfileContextLoaded`-producing kickoff; a resolved load
    /// whose captured generation no longer matches is discarded, so a slow load
    /// for game A can never clobber state after the user switched to game B.
    pub context_generation: u64,
    /// Generation guard for the lightweight async data-tab conflict refresh
    /// (`Message::DataTabConflictsLoaded`). Independent of `context_generation`
    /// so opening the Data tab never cancels an in-flight profile reload.
    pub data_tab_generation: u64,
}

fn load_hidden_files(
    pm: &ProfileManager,
    profile: &modde_core::Profile,
) -> HashSet<(String, String)> {
    profile
        .id
        .and_then(|profile_id| crate::app::block_on(pm.db().list_hidden_files(profile_id)).ok())
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
        .and_then(|profile_id| crate::app::block_on(pm.db().get_plugin_order(profile_id)).ok())
        .unwrap_or_default();

    if plugins.is_empty() {
        plugins =
            modde_games::read_native_plugin_order(profile.game_id.as_str()).unwrap_or_default();
        if let Some(profile_id) = profile.id {
            let _ = crate::app::block_on(pm.db().set_plugin_order(profile_id, &plugins));
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

// ─── Application Messages ────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Message {
    /// External process (typically the CLI) notified the GUI that the
    /// profile DB has changed. Triggers a profile reload.
    ExternalRefresh,

    /// Async result of `load_profile_context` — the off-thread profile +
    /// data-tab + tool reload. `generation` guards against stale loads
    /// clobbering newer state (see `Modde::context_generation`).
    ProfileContextLoaded {
        generation: u64,
        result: Result<ProfileContextSnapshot, String>,
    },
    /// Async result of the lightweight data-tab conflict refresh fired when the
    /// Data tab is opened (see `Modde::data_tab_generation`).
    DataTabConflictsLoaded {
        generation: u64,
        result: Result<DataTabConflicts, String>,
    },

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

#[cfg(unix)]
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

#[cfg(not(unix))]
fn external_refresh_stream() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(1, |_output| async move {
        std::future::pending::<()>().await;
    })
}

/// Drop guard that unlinks a Unix socket when the listening task ends.
#[cfg(unix)]
struct SocketGuard {
    path: std::path::PathBuf,
}

#[cfg(unix)]
impl SocketGuard {
    fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

#[cfg(unix)]
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
