use std::collections::HashMap;
use std::path::PathBuf;

use iced::widget::{column, container, row, text};
use iced::{Element, Length, Task, Theme};
use smallvec::SmallVec;

use modde_core::manifest::collection::CollectionManifest;
use modde_core::profile::ProfileManager;
use modde_core::resolver::{ConflictMap, ModId};
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
    pub mod_filter: String,
    pub theme_name: String,
    pub wabbajack_manifest: Option<modde_core::WabbajackManifest>,
    pub active_downloads: Vec<crate::views::collections::CollectionDownload>,
    // ── New state fields ──
    pub loaded_profile: Option<modde_core::Profile>,
    pub resolved_order: Vec<ModId>,
    pub conflict_map: ConflictMap,
    pub save_snapshots: Vec<SaveSnapshot>,
    pub current_fingerprint: Option<modde_core::save::SaveFingerprint>,
    pub experiment_depth: usize,
    pub nexus_status: Option<NexusAuthStatus>,
    pub verify: VerifyState,
    pub new_profile_name: String,
    pub new_profile_game: String,
    pub available_games: SmallVec<[(String, String); 6]>,
    pub selected_game: Option<String>,
    pub stock_snapshot_exists: bool,
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

impl Modde {
    pub fn settings_state(&self) -> SettingsState {
        SettingsState {
            nexus_api_key: self.settings.nexus_api_key.clone(),
            game_path: self
                .settings
                .game_paths
                .first()
                .map(|gp| gp.path.clone()),
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
                    match modde_core::resolver::resolve(&profile) {
                        Ok(resolved) => {
                            self.resolved_order = resolved.order;
                        }
                        Err(_) => {
                            self.resolved_order = profile
                                .mods
                                .iter()
                                .filter(|m| m.enabled)
                                .map(|m| ModId::from(m.mod_id.clone()))
                                .collect();
                        }
                    }
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
        }
    }

    fn save_settings(&self) {
        self.settings.save();
    }
}

/// Which view is currently displayed.
#[derive(Debug, Clone)]
pub enum View {
    ModList,
    LoadOrder,
    Collections,
    WabbajackInstaller(WabbajackInstallerState),
    FOMODWizard(FOMODWizardState),
    Settings,
    Saves,
    Verify,
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
            None => fomod_oxide::installer::InstallPlan {
                operations: vec![],
            },
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
            None => fomod_oxide::installer::InstallPlan {
                operations: vec![],
            },
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
    CreateProfile { name: String, game_id: String },
    DeleteProfile(String),
    ForkProfile { source: String, new_name: String },

    // Profile dialog
    NewProfileNameChanged(String),
    NewProfileGameChanged(String),

    // Game selection
    SelectGame(String),

    // Mod list
    ToggleMod { mod_id: String, enabled: bool },
    FilterChanged(String),
    AddMod,
    AddModFromPath(PathBuf),
    RemoveMod(usize),
    SelectMod(usize),
    Deploy,
    DeployComplete(Result<String, String>),

    // Load order
    ReorderMod { from: usize, to: usize },
    ApplyLoadOrder,

    // Collections
    SearchCollections(String),
    InstallCollection { slug: String, version: String },

    // Wabbajack
    OpenWabbajackFile,
    WabbajackFileSelected(PathBuf),
    WabbajackProgress(f32),
    WabbajackStartInstall,
    WabbajackLog(String),

    // FOMOD
    StartFOMOD { mod_path: PathBuf, dest_path: PathBuf },
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
    DownloadProgress { id: String, bytes: u64, total: u64 },
    DownloadComplete { id: String },
    DownloadFailed { id: String, error: String },

    // Settings
    SetNexusApiKey(String),
    SetGamePath { game_id: String, path: PathBuf },
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

    // Verification
    RunVerify,
    VerifyComplete(VerifyResults),

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

        let available_games: SmallVec<[(String, String); 6]> = smallvec::smallvec![
            ("skyrim-se".to_string(), "Skyrim SE".to_string()),
            ("skyrim-ae".to_string(), "Skyrim AE".to_string()),
            ("fallout4".to_string(), "Fallout 4".to_string()),
            ("fallout76".to_string(), "Fallout 76".to_string()),
            ("cyberpunk2077".to_string(), "Cyberpunk 2077".to_string()),
        ];

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
            mod_filter: String::new(),
            theme_name,
            wabbajack_manifest: None,
            active_downloads: Vec::new(),
            loaded_profile: None,
            resolved_order: Vec::new(),
            conflict_map: ConflictMap::default(),
            save_snapshots: Vec::new(),
            current_fingerprint: None,
            experiment_depth: 0,
            nexus_status: None,
            verify: VerifyState::Idle,
            new_profile_name: String::new(),
            new_profile_game: available_games.first().map(|(id, _)| id.clone()).unwrap_or_default(),
            available_games,
            selected_game,
            stock_snapshot_exists: false,
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
        (app, Task::none())
    }

    fn title(&self) -> String {
        "modde".to_string()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Navigation ───────────────────────────────────────
            Message::SwitchView(view) => {
                // Auto-load save history when switching to Saves view
                if matches!(view, View::Saves) {
                    self.active_view = view;
                    return self.update(Message::LoadSaveHistory);
                }
                self.active_view = view;
            }
            Message::SwitchProfile(name) => {
                self.active_profile = Some(name);
                self.reload_profile();
                self.status_message = "Profile switched".to_string();
            }
            Message::CreateProfile { name, game_id } => {
                match ProfileManager::open() {
                    Ok(pm) => {
                        let profile = modde_core::Profile {
                            id: None,
                            name: name.clone(),
                            game_id: modde_core::GameId::from(game_id),
                            source: modde_core::ProfileSource::Manual,
                            mods: Vec::new(),
                            overrides: PathBuf::from("overrides"),
                            load_order_rules: smallvec::SmallVec::new(),
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
                }
            }
            Message::DeleteProfile(name) => {
                match ProfileManager::open() {
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
                }
            }
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
            Message::NewProfileGameChanged(game) => self.new_profile_game = game,

            // ── Game selection ────────────────────────────────────
            Message::SelectGame(game_id) => {
                self.selected_game = Some(game_id.clone());
                self.settings.selected_game = Some(game_id);
                self.save_settings();
            }

            // ── Mod list ─────────────────────────────────────────
            Message::ToggleMod { mod_id, enabled } => {
                if let Some(ref profile_name) = self.active_profile {
                    if let Ok(pm) = ProfileManager::open() {
                        if let Ok(mut profile) = pm.load(profile_name, None) {
                            if let Some(m) = profile.mods.iter_mut().find(|m| m.mod_id == mod_id) {
                                m.enabled = enabled;
                            }
                            let _ = pm.create(&profile).or_else(|_| pm.update(&profile).map(|_| 0));
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
                                version: None,
                                fomod_config: None,
                            });
                            let _ = pm.create(&profile).or_else(|_| pm.update(&profile).map(|_| 0));
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
                                let _ = pm.create(&profile).or_else(|_| pm.update(&profile).map(|_| 0));
                                self.selected_mod_index = None;
                                self.status_message = format!("Removed mod: {}", removed.mod_id);
                                self.reload_profile();
                            }
                        }
                    }
                }
            }
            Message::SelectMod(index) => self.selected_mod_index = Some(index),
            Message::Deploy => {
                self.status_message = "Deploying mods...".to_string();
                if let Some(ref profile) = self.loaded_profile {
                    let profile_name = profile.name.clone();
                    let game_id = profile.game_id.clone();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || -> Result<String, String> {
                                let pm = ProfileManager::open().map_err(|e| e.to_string())?;
                                let profile = pm.load(&profile_name, Some(&game_id)).map_err(|e| e.to_string())?;
                                let resolved = modde_core::resolver::resolve(&profile).map_err(|e| e.to_string())?;
                                let game_plugin = modde_games::resolve_game_plugin(&game_id)
                                    .ok_or_else(|| format!("unsupported game: {game_id}"))?;
                                let install_path = game_plugin.detect_install()
                                    .ok_or_else(|| format!("could not detect install for {game_id}"))?;
                                let mod_dir = game_plugin.mod_directory(&install_path);
                                let staging_dir = ProfileManager::staging_dir(&profile.name);
                                game_plugin.deploy(&staging_dir, &mod_dir).map_err(|e| e.to_string())?;
                                game_plugin.post_deploy(&install_path).map_err(|e| e.to_string())?;
                                Ok(format!("Deployed {} mod(s) for {}", resolved.order.len(), game_id))
                            }).await.map_err(|e| e.to_string())?
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
            Message::ReorderMod { from, to } => {
                if let Some(ref profile_name) = self.active_profile {
                    if let Ok(pm) = ProfileManager::open() {
                        if let Ok(mut profile) = pm.load(profile_name, None) {
                            if from < profile.mods.len() && to < profile.mods.len() {
                                let item = profile.mods.remove(from);
                                profile.mods.insert(to, item);
                                let _ = pm.create(&profile).or_else(|_| pm.update(&profile).map(|_| 0));
                                self.status_message = "Mod reordered".to_string();
                                self.reload_profile();
                            }
                        }
                    }
                }
            }
            Message::ApplyLoadOrder => {
                if let Some(ref profile_name) = self.active_profile {
                    if let Ok(pm) = ProfileManager::open() {
                        if let Ok(profile) = pm.load(profile_name, None) {
                            let _ = pm.create(&profile).or_else(|_| pm.update(&profile).map(|_| 0));
                        }
                    }
                }
                self.status_message = "Load order saved".to_string();
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
                    async move {
                        Ok::<Vec<CollectionManifest>, anyhow::Error>(Vec::new())
                    },
                    |result| match result {
                        Ok(_) => Message::Noop,
                        Err(_) => Message::Noop,
                    },
                );
            }
            Message::InstallCollection { slug, version } => {
                self.status_message = format!("Installing collection {slug} v{version}...");
            }

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
                    let name = if archive.file_names().any(|n| n == "modlist.json") { "modlist.json" } else { "modlist" };
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
                                    let name = if archive.file_names().any(|n| n == "modlist.json") { "modlist.json" } else { "modlist" };
                                    let mut entry = archive.by_name(name)?;
                                    let mut buf = String::new();
                                    std::io::Read::read_to_string(&mut entry, &mut buf)?;
                                    serde_json::from_str(&buf)?
                                };
                                Ok::<String, anyhow::Error>(manifest.name)
                            },
                            |result: Result<String, anyhow::Error>| match result {
                                Ok(name) => Message::WabbajackLog(format!("Parsed manifest: {name}")),
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
            Message::StartFOMOD { mod_path, dest_path } => {
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
            Message::FOMODChoice { step, group, option, selected } => {
                if let Some(ref mut installer) = self.fomod_installer {
                    installer.checkpoint();
                    self.fomod_can_undo = true;
                    let group_type = installer.group_type_at(step, group);
                    let entry = self.fomod_selections.entry((step, group)).or_default();
                    match group_type {
                        Some(fomod_oxide::config::GroupType::SelectExactlyOne)
                        | Some(fomod_oxide::config::GroupType::SelectAtMostOne) => {
                            if selected { *entry = vec![option]; } else { entry.retain(|&o| o != option); }
                        }
                        Some(fomod_oxide::config::GroupType::SelectAll) => {}
                        _ => {
                            if selected { if !entry.contains(&option) { entry.push(option); } } else { entry.retain(|&o| o != option); }
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
                        let installer = self.fomod_installer.as_ref().ok_or("No active FOMOD installer")?;
                        let source = self.fomod_source_dir.as_ref().ok_or("No source directory")?;
                        let dest = self.fomod_dest_dir.as_ref().ok_or("No destination directory")?;
                        let plan = installer.resolve();
                        plan.execute(source, dest).map_err(|e| e.to_string())?;
                        Ok(())
                    })();
                    match &result {
                        Ok(()) => self.status_message = "FOMOD installation completed successfully".to_string(),
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
                if self.fomod_wizard_pos > 0 { self.fomod_wizard_pos -= 1; }
            }
            Message::FOMODCancel => {
                self.reset_fomod();
                self.active_view = View::ModList;
                self.status_message = "FOMOD installation cancelled".to_string();
            }
            Message::FOMODUndo => {
                let rolled_back = self.fomod_installer.as_mut().map(|i| i.rollback()).unwrap_or(false);
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
                let pct = if total > 0 { (bytes as f64 / total as f64) * 100.0 } else { 0.0 };
                self.status_message = format!("Downloading {id}: {pct:.0}%");
            }
            Message::DownloadComplete { id } => self.status_message = format!("Download complete: {id}"),
            Message::DownloadFailed { id, error } => self.status_message = format!("Download failed ({id}): {error}"),

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
                    async { rfd::AsyncFileDialog::new().set_title("Select Game Directory").pick_folder().await.map(|h| h.path().to_path_buf()) },
                    |path| match path { Some(p) => Message::SetGamePath { game_id: "default".to_string(), path: p }, None => Message::Noop },
                );
            }
            Message::BrowseDownloadDir => {
                return Task::perform(
                    async { rfd::AsyncFileDialog::new().set_title("Select Download Directory").pick_folder().await.map(|h| h.path().to_path_buf()) },
                    |path| match path { Some(p) => Message::SetDownloadDir(p), None => Message::Noop },
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
                            if api_key.is_empty() { return Err("No API key set".to_string()); }
                            let client = reqwest::blocking::Client::new();
                            let resp = client.get("https://api.nexusmods.com/v1/users/validate.json")
                                .header("apikey", &api_key).send().map_err(|e| e.to_string())?;
                            if !resp.status().is_success() { return Err(format!("HTTP {}", resp.status())); }
                            let body: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
                            let name = body["name"].as_str().unwrap_or("Unknown").to_string();
                            let is_premium = body["is_premium"].as_bool().unwrap_or(false);
                            Ok((name, is_premium))
                        }).await.map_err(|e| e.to_string())?
                    },
                    Message::NexusKeyValidated,
                );
            }
            Message::NexusKeyValidated(result) => match result {
                Ok((username, is_premium)) => {
                    self.nexus_status = Some(NexusAuthStatus::Valid { username: username.clone(), is_premium });
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
                                let install_path = game_plugin.detect_install()
                                    .ok_or_else(|| format!("could not detect install for {game_id}"))?;
                                let mgr = modde_core::stock::StockGameManager::new(modde_core::stock::StockGameManager::default_dir());
                                let rt = tokio::runtime::Handle::current();
                                rt.block_on(mgr.snapshot(&game_id, &install_path)).map_err(|e| e.to_string())?;
                                Ok(format!("Snapshot created for {game_id}"))
                            }).await.map_err(|e| e.to_string())?
                        },
                        Message::StockSnapshotCreated,
                    );
                } else {
                    self.status_message = "No active profile".to_string();
                }
            }
            Message::StockSnapshotCreated(result) => match result {
                Ok(msg) => { self.stock_snapshot_exists = true; self.status_message = msg; }
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
                                let _install_path = game_plugin.detect_install()
                                    .ok_or_else(|| format!("could not detect install for {game_id}"))?;
                                let mgr = modde_core::stock::StockGameManager::new(modde_core::stock::StockGameManager::default_dir());
                                let rt = tokio::runtime::Handle::current();
                                match rt.block_on(mgr.verify(&game_id)) {
                                    Ok(true) => Ok("Stock snapshot verified: OK".to_string()),
                                    Ok(false) => Ok("Stock snapshot MODIFIED".to_string()),
                                    Err(e) => Err(e.to_string()),
                                }
                            }).await.map_err(|e| e.to_string())?
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
                if let (Some(profile), Some(profile_name)) = (&self.loaded_profile, &self.active_profile) {
                    let game_id = profile.game_id.clone();
                    let name = profile_name.clone();
                    match ProfileManager::open() {
                        Ok(pm) => {
                            let save_dir = modde_games::resolve_game_plugin(&game_id).and_then(|g| g.save_directory());
                            match pm.try_profile(&name, &game_id, save_dir.as_deref()) {
                                Ok(()) => { self.experiment_depth += 1; self.status_message = format!("Experiment started (depth {})", self.experiment_depth); }
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
                            let save_dir = modde_games::resolve_game_plugin(&game_id).and_then(|g| g.save_directory());
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
                            Ok(()) => { self.experiment_depth = 0; self.status_message = "Experiment committed".to_string(); }
                            Err(e) => self.status_message = format!("Commit failed: {e}"),
                        },
                        Err(e) => self.status_message = format!("Error: {e}"),
                    }
                }
            }

            // ── Saves ────────────────────────────────────────────
            Message::LoadSaveHistory => {
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    let profile_name = profile.name.clone();
                    match modde_core::save::SaveManager::history(&game_id, &profile_name, 20) {
                        Ok(history) => self.save_snapshots = history,
                        Err(e) => { self.save_snapshots = Vec::new(); self.status_message = format!("Could not load save history: {e}"); }
                    }
                }
            }
            Message::RestoreSaveSnapshot(commit_id) => {
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    let profile_name = profile.name.clone();
                    let save_dir = modde_games::resolve_game_plugin(&game_id).and_then(|g| g.save_directory());
                    if let Some(save_dir) = save_dir {
                        match modde_core::save::SaveManager::restore(&game_id, &profile_name, &commit_id, &save_dir) {
                            Ok(count) => self.status_message = format!("Restored {count} save file(s)"),
                            Err(e) => self.status_message = format!("Restore failed: {e}"),
                        }
                    } else {
                        self.status_message = "Cannot detect save directory for this game".to_string();
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
                                let mut results = VerifyResults { missing_mods: SmallVec::new(), hash_mismatches: Vec::new(), broken_symlinks: SmallVec::new(), ok_count: 0 };
                                if !staging_dir.exists() { return results; }
                                fn walk(dir: &std::path::Path, results: &mut VerifyResults) {
                                    if let Ok(entries) = std::fs::read_dir(dir) {
                                        for entry in entries.flatten() {
                                            let path = entry.path();
                                            if path.is_dir() { walk(&path, results); }
                                            else if path.is_symlink() {
                                                match std::fs::read_link(&path) {
                                                    Ok(target) if target.exists() => results.ok_count += 1,
                                                    _ => results.broken_symlinks.push(path),
                                                }
                                            } else { results.ok_count += 1; }
                                        }
                                    }
                                }
                                walk(&staging_dir, &mut results);
                                results
                            }).await.unwrap_or(VerifyResults { missing_mods: SmallVec::new(), hash_mismatches: Vec::new(), broken_symlinks: SmallVec::new(), ok_count: 0 })
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

            Message::Noop => {}
        }
        Task::none()
    }

    // ─── View ────────────────────────────────────────────────────

    fn view(&self) -> Element<'_, Message> {
        let sidebar = crate::views::sidebar::view(
            &self.active_view,
            &self.profiles,
            &self.active_profile,
            self.experiment_depth,
            &self.new_profile_name,
            &self.new_profile_game,
            &self.available_games,
        );

        let mods = self.loaded_profile.as_ref().map(|p| p.mods.as_slice()).unwrap_or(&[]);
        let settings_state = self.settings_state();

        let content: Element<Message> = match &self.active_view {
            View::ModList => crate::views::mod_list::view(mods, &self.mod_filter, self.selected_mod_index),
            View::LoadOrder => crate::views::load_order::view(&self.resolved_order, &self.conflict_map),
            View::Collections => crate::views::collections::view(&self.collection_search, &self.collections, &self.active_downloads),
            View::FOMODWizard(_) => crate::views::fomod_wizard::view(self),
            View::Settings => crate::views::settings::view(settings_state),
            View::WabbajackInstaller(state) => crate::views::wabbajack::view(state, &self.wabbajack_manifest),
            View::Saves => crate::views::saves::view(
                &self.save_snapshots,
                self.loaded_profile.as_ref().map(|p| p.name.as_str()),
                self.current_fingerprint.as_ref(),
            ),
            View::Verify => crate::views::verify::view(&self.verify),
        };

        let status_bar = container(text(&self.status_message).size(12)).padding(5);

        let main_layout = column![
            row![sidebar, content].spacing(0).height(Length::Fill),
            status_bar,
        ]
        .spacing(0);

        container(main_layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
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
}

/// Run the iced application.
pub fn run() -> iced::Result {
    iced::application(Modde::new, Modde::update, Modde::view)
        .title(Modde::title)
        .theme(Modde::theme)
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
            mod_filter: String::new(),
            theme_name: "Dark".to_string(),
            wabbajack_manifest: None,
            active_downloads: Vec::new(),
            loaded_profile: None,
            resolved_order: Vec::new(),
            conflict_map: ConflictMap::default(),
            save_snapshots: Vec::new(),
            current_fingerprint: None,
            experiment_depth: 0,
            nexus_status: None,
            verify: VerifyState::Idle,
            new_profile_name: String::new(),
            new_profile_game: "skyrim-se".to_string(),
            available_games: smallvec::smallvec![("skyrim-se".to_string(), "Skyrim SE".to_string())],
            selected_game: None,
            stock_snapshot_exists: false,
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
        let results = VerifyResults { missing_mods: SmallVec::new(), hash_mismatches: vec![], broken_symlinks: smallvec::smallvec![PathBuf::from("/broken")], ok_count: 42 };
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
}
