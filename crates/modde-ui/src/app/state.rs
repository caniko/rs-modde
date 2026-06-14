use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use modde_core::resolver::GameId;
use modde_core::settings::AppSettings;

use super::{FOMODWizardState, ToolOptionCatalog};

/// Direction for `Message::ReorderMod`. Re-export of the core type so
/// view and message construction sites don't have to import from
/// `modde_core::profile` directly. The enforcement logic itself lives in
/// `modde_core::profile::try_reorder` and is the single source of truth.
pub use modde_core::profile::ReorderDirection;

/// Render a `LockReason` as a short, human-readable phrase for status
/// messages and UI banners. Centralised so `lock_reason` strings stay
/// consistent across the handler, `load_order` banner, and `mod_list` row.
pub(crate) fn format_lock_reason(reason: &modde_core::LockReason) -> String {
    use modde_core::LockReason::{Manual, NexusCollection, TomlImport, Wabbajack};
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
    Downloads,
    DataTab,
    Diagnostics,
    Tools,
    Executables,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidebarGroup {
    Game,
    Install,
    General,
}

impl SidebarGroup {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            SidebarGroup::Game => "Game",
            SidebarGroup::Install => "Install",
            SidebarGroup::General => "General",
        }
    }
}

#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct ToolState {
    pub entries: Vec<ToolUiEntry>,
    pub active_tool_id: Option<String>,
    pub game_label: Option<String>,
    pub game_dir_configured: bool,
    pub loading: bool,
    pub load_error: Option<String>,
    pub load_generation: u64,
    pub show_advanced_settings: bool,
    pub active_operations: HashSet<String>,
    pub tool_option_catalog: ToolOptionCatalog,
    pub optiscaler_releases: Vec<modde_games::tools::ToolReleaseSummary>,
    pub optiscaler_releases_loading: bool,
    pub proton_versions_loading: bool,
    pub executables: Vec<ExecutableUiEntry>,
    pub executables_loading: bool,
    pub executables_load_error: Option<String>,
    pub executables_load_generation: u64,
    pub executable_draft: ExecutableDraft,
    pub executable_editor_open: bool,
    pub executable_error: Option<String>,
    pub active_executable_operations: HashSet<String>,
}

impl ToolState {
    #[must_use]
    pub fn is_tool_busy(&self, tool_id: &str) -> bool {
        self.active_operations.contains(tool_id)
    }

    #[must_use]
    pub fn is_executable_busy(&self, name: &str) -> bool {
        self.active_executable_operations.contains(name)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutableUiEntry {
    pub name: String,
    pub executable_path: String,
    pub arguments: String,
    pub working_dir: String,
    pub environment: String,
    pub wine_dll_overrides: String,
    pub output_mod: String,
    pub enabled: bool,
}

impl ExecutableUiEntry {
    pub(super) fn from_row(row: modde_core::db::ExecutableConfigRow) -> Self {
        let args = serde_json::from_str::<Vec<String>>(&row.arguments_json).unwrap_or_default();
        let env = serde_json::from_str::<HashMap<String, String>>(&row.environment_json)
            .unwrap_or_default();
        let mut env_lines = env
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>();
        env_lines.sort();
        Self {
            name: row.name,
            executable_path: row.executable_path.display().to_string(),
            arguments: args.join(" "),
            working_dir: row
                .working_dir
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            environment: env_lines.join("\n"),
            wine_dll_overrides: row.wine_dll_overrides.unwrap_or_default(),
            output_mod: row.output_mod,
            enabled: row.enabled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableDraft {
    pub name: String,
    pub executable_path: String,
    pub arguments: String,
    pub working_dir: String,
    pub environment: String,
    pub wine_dll_overrides: String,
    pub output_mod: String,
}

impl Default for ExecutableDraft {
    fn default() -> Self {
        Self {
            name: String::new(),
            executable_path: String::new(),
            arguments: String::new(),
            working_dir: String::new(),
            environment: String::new(),
            wine_dll_overrides: String::new(),
            output_mod: "__overwrite__".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutableDraftField {
    Name,
    Path,
    Arguments,
    WorkingDir,
    Environment,
    WineDllOverrides,
    OutputMod,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddCustomGameDraft {
    pub id: String,
    pub display_name: String,
    pub install_path: String,
    pub executable_dir: Option<String>,
    pub steam_app_id: Option<String>,
    pub nexus_domain: Option<String>,
    pub proxy_dlls_csv: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddCustomGameState {
    pub draft: AddCustomGameDraft,
    pub detected_dirs: Vec<modde_games::DetectCandidateDir>,
    pub error: Option<String>,
}

impl AddCustomGameState {
    #[must_use]
    pub fn can_submit(&self) -> bool {
        self.build_spec().is_ok()
    }

    pub fn build_spec(&self) -> Result<modde_games::generic::spec::GameSpec, String> {
        let install_path = PathBuf::from(self.draft.install_path.trim());
        if self.draft.id.trim().is_empty()
            || self.draft.display_name.trim().is_empty()
            || self.draft.executable_dir.is_none()
            || self.draft.install_path.trim().is_empty()
        {
            return Err("Fill in the required custom game fields.".to_string());
        }
        if !install_path.is_dir() {
            return Err(format!(
                "Install path does not exist: {}",
                install_path.display()
            ));
        }

        let spec = modde_games::generic::spec::GameSpec {
            id: self.draft.id.trim().to_string(),
            display_name: self.draft.display_name.trim().to_string(),
            steam_app_id: empty_to_none(self.draft.steam_app_id.as_deref()),
            install_dir_name: None,
            install_path_override: None,
            executable_dir: PathBuf::from(
                self.draft
                    .executable_dir
                    .as_deref()
                    .unwrap_or_default()
                    .trim(),
            ),
            mod_dir: None,
            nexus_domain: empty_to_none(self.draft.nexus_domain.as_deref()),
            proxy_dlls: self
                .draft
                .proxy_dlls_csv
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect(),
        };

        spec.validate().map_err(|error| error.to_string())?;
        Ok(spec)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddCustomGameDraftField {
    Id,
    DisplayName,
    InstallPath,
    ExecutableDir,
    SteamAppId,
    NexusDomain,
    ProxyDlls,
}

pub(super) fn empty_to_none(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct ToolUiEntry {
    pub tool_id: String,
    pub display_name: String,
    pub description: String,
    pub category: String,
    pub available: bool,
    pub availability_text: String,
    pub enabled: bool,
    pub settings: serde_json::Value,
    pub setting_specs: Vec<modde_games::tools::ToolSettingSpec>,
    pub generated_config_path: Option<String>,
    pub applied_files: Vec<String>,
    pub has_file_patching: bool,
    pub release_support: ToolReleaseSupport,
    pub status_message: Option<String>,
    pub env_preview: Vec<(String, String)>,
    pub dll_overrides: Vec<String>,
    pub wrapper_preview: Vec<String>,
    pub derived_facts: Vec<(String, String)>,
    pub optiscaler_state: Option<String>,
    pub optiscaler_latest_backup: Option<String>,
    pub optiscaler_detected_files: usize,
    pub apply_pending: bool,
    pub apply_missing_inputs: Vec<String>,
    pub setting_history: Vec<ToolHistoryUiEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolHistoryUiEntry {
    pub node_id: String,
    pub label: String,
    pub reason: String,
    pub enabled: bool,
    pub is_current: bool,
}

impl ToolHistoryUiEntry {
    pub(super) fn from_node(node: modde_core::db::ToolSettingHistoryNode) -> Self {
        let short_id = node.node_id.chars().take(18).collect::<String>();
        let state = if node.enabled { "enabled" } else { "disabled" };
        Self {
            node_id: node.node_id,
            label: format!("{} - {state}", node.created_at),
            reason: node.reason,
            enabled: node.enabled,
            is_current: node.is_current,
        }
        .with_short_id(short_id)
    }

    fn with_short_id(mut self, short_id: String) -> Self {
        if !short_id.is_empty() {
            self.label = format!("{} ({short_id})", self.label);
        }
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolReleaseSupport {
    None,
    Supported,
}

impl ToolReleaseSupport {
    #[must_use]
    pub fn from_supports_releases(supports_releases: bool) -> Self {
        if supports_releases {
            Self::Supported
        } else {
            Self::None
        }
    }

    #[must_use]
    pub fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }
}

#[derive(Debug, Clone)]
pub struct ToolApplyResult {
    pub display_name: String,
    pub applied_file_count: usize,
    pub validation_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolRevertResult {
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub struct ToolLoadSnapshot {
    pub entries: Vec<ToolUiEntry>,
    pub active_tool_id: Option<String>,
    pub game_label: Option<String>,
    pub game_dir_configured: bool,
    pub tool_option_catalog: ToolOptionCatalog,
    pub executables: Vec<ExecutableUiEntry>,
}

#[derive(Debug, Clone)]
pub struct ToolSettingWriteResult {
    pub status_message: String,
    pub tool_option_catalog: Option<ToolOptionCatalog>,
}

#[derive(Debug, Clone)]
pub(super) struct ToolLoadRequest {
    pub(super) game_id: String,
    pub(super) display_name: String,
    pub(super) configured_game_dir: Option<PathBuf>,
    pub(super) optiscaler_releases: Vec<modde_games::tools::ToolReleaseSummary>,
    pub(super) tool_option_catalog: ToolOptionCatalog,
    pub(super) previous_active_tool_id: Option<String>,
}

/// Owned, off-thread-computed result of (re)loading the profile + data-tab +
/// tool context for a game/profile pair.
///
/// Built by `load_profile_context` on a blocking task and
/// applied synchronously by `Modde::apply_profile_context`, so the iced event
/// loop never blocks on the multi-query profile reload that
/// `reload_profile`/`switch_game_context` used to perform inline.
#[derive(Debug, Clone)]
pub struct ProfileContextSnapshot {
    pub profiles: Vec<modde_core::profile::ProfileSummary>,
    pub active_profile: Option<String>,
    pub profile_outcome: ProfileLoadOutcome,
    pub data_tab_conflicts: Vec<(String, Vec<String>)>,
    pub missing_store_mod_count: usize,
    /// Folded tool snapshot. `Some` when a game is in scope (apply it via
    /// `apply_tool_snapshot`); `None` when no game is selected (clear the tool
    /// state, mirroring the old `refresh_tools_state` empty branch).
    pub tools: Option<ToolLoadSnapshot>,
    /// Re-run diagnostics after applying, when the Diagnostics view is active.
    /// Only set by the profile-switch handler (preserves the old synchronous
    /// `SwitchProfile` behavior).
    pub rerun_diagnostics: bool,
}

#[derive(Debug, Clone)]
pub struct DiagnosticsComputed {
    pub report: crate::views::diagnostics::DiagnosticsReport,
    pub data_tab_conflicts: Vec<(String, Vec<String>)>,
    pub missing_store_mod_count: usize,
}

/// What happened when the loader tried to (re)load the active profile.
#[derive(Debug, Clone)]
pub enum ProfileLoadOutcome {
    /// No active profile — clear `loaded_profile` and `mod_id_filter_keys`.
    Cleared,
    /// Profile loaded fresh from the DB.
    Loaded {
        profile: Box<modde_core::Profile>,
        experiment_depth: usize,
        current_fingerprint: Option<modde_core::save::SaveFingerprint>,
        mod_id_filter_keys: Vec<String>,
    },
    /// `pm.load` failed — leave the previously-loaded profile (and its derived
    /// fields) untouched, mirroring the old `reload_profile` behavior on a load
    /// error.
    KeepPrevious,
}

/// Inputs for [`super::model::load_profile_context`], built on the iced thread
/// from `&Modde` and moved into the blocking loader.
pub(super) struct ProfileContextRequest {
    pub(super) selected_game: Option<String>,
    pub(super) active_profile: Option<String>,
    /// `true` for game switches: recompute the active profile from the DB
    /// (active pointer, then first listed profile) instead of trusting
    /// `active_profile`.
    pub(super) recompute_active: bool,
    /// The currently-loaded profile, used to recompute data-tab conflicts when
    /// the load fails ([`ProfileLoadOutcome::KeepPrevious`]).
    pub(super) fallback_profile: Option<modde_core::Profile>,
    /// `Some` to fold a tool reload into the same off-thread job; `None` clears
    /// the tool state on apply.
    pub(super) tool_request: Option<ToolLoadRequest>,
    pub(super) rerun_diagnostics: bool,
}

/// Off-thread-computed data-tab conflict result — the lightweight refresh that
/// `Message::DataTabConflictsLoaded` applies when the Data tab is opened.
#[derive(Debug, Clone)]
pub struct DataTabConflicts {
    pub conflicts: Vec<(String, Vec<String>)>,
    pub missing_store_mod_count: usize,
}

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

pub(super) fn prefill_wabbajack_game_dir(
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
