use std::path::PathBuf;

use iced::widget::{container, opaque};
use iced::{Element, Length, Task};
use modde_core::profile::ProfileManager;
use modde_core::resolver::GameId;

use super::state::{
    DataTabConflicts, ProfileContextRequest, ProfileContextSnapshot, ProfileLoadOutcome,
    ToolLoadRequest,
};
use super::tool_ops::{load_executables_for_game, load_tools_state, load_tools_state_blocking};
use super::{
    DiagnosticsComputed, Message, Modde, SettingsState, ToolLoadSnapshot, View,
    WabbajackInstallerState, build_conflict_rows, build_default_download_meta, detected_game_ids,
    format_diagnostic_entry, load_active_plugins, load_hidden_files, settings_game_install_paths,
};

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

    pub(super) fn refresh_nexus_api_key_state(&mut self) {
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

    pub(super) fn refresh_fomod_conflicts(&mut self) {
        if let Some(ref installer) = self.fomod_installer {
            self.fomod_conflicts = installer.detect_conflicts().into();
        }
    }

    pub(super) fn clear_game_scoped_state(&mut self) {
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

    pub(super) fn game_supports_save_profiles(game_id: &str) -> bool {
        modde_games::resolve_game_plugin(game_id)
            .is_some_and(modde_games::GamePlugin::supports_save_profiles)
    }

    pub(super) fn current_game_supports_save_profiles(&self) -> bool {
        self.loaded_profile
            .as_ref()
            .map(|p| p.game_id.as_str())
            .or(self.selected_game.as_deref())
            .is_some_and(Self::game_supports_save_profiles)
    }

    pub(super) fn resolve_save_dir(game_id: &str) -> Option<PathBuf> {
        let plugin = modde_games::resolve_game_plugin(game_id)?;
        plugin
            .supports_save_profiles()
            .then(|| plugin.save_directory())
            .flatten()
    }

    /// Reload the active profile, its data-tab conflicts, and the tool state
    /// off the render thread, returning the `Task` that resolves to
    /// `Message::ProfileContextLoaded`. Replaces the old synchronous
    /// `reload_profile` (which drove the whole multi-query reload via
    /// `block_on` on the iced thread).
    pub(super) fn reload_profile(&mut self) -> Task<Message> {
        let request = self.reload_request(false);
        self.dispatch_profile_context(request)
    }

    /// Like [`Self::reload_profile`], but re-runs diagnostics after the load
    /// resolves when the Diagnostics view is active. Used only by the
    /// profile-switch handler to preserve its previous synchronous behavior.
    pub(super) fn reload_profile_refresh_diagnostics(&mut self) -> Task<Message> {
        let request = self.reload_request(true);
        self.dispatch_profile_context(request)
    }

    /// Reload, recomputing the active profile from the DB (active pointer, then
    /// first listed profile). Used after deleting the active profile in a
    /// no-game context, where the previous active profile is gone.
    pub(super) fn reload_profile_recompute_active(&mut self) -> Task<Message> {
        let mut request = self.reload_request(false);
        request.recompute_active = true;
        self.dispatch_profile_context(request)
    }

    /// Build the request for an in-place reload (no scoped-state clear, keeps
    /// the current `selected_game`/`active_profile`).
    fn reload_request(&self, rerun_diagnostics: bool) -> ProfileContextRequest {
        ProfileContextRequest {
            selected_game: self.selected_game.clone(),
            active_profile: self.active_profile.clone(),
            recompute_active: false,
            fallback_profile: self.loaded_profile.clone(),
            tool_request: self.tool_load_request(),
            rerun_diagnostics,
        }
    }

    /// Bump the context generation, mark the tool state as loading (when a tool
    /// reload is folded in), and spawn the off-thread loader.
    fn dispatch_profile_context(&mut self, request: ProfileContextRequest) -> Task<Message> {
        self.context_generation = self.context_generation.wrapping_add(1);
        let generation = self.context_generation;
        self.diagnostics_generation = self.diagnostics_generation.wrapping_add(1);
        // A composite reload recomputes the data-tab conflicts authoritatively,
        // so any in-flight standalone data-tab refresh is now stale — invalidate
        // it so its older result can't clobber the new game's conflicts.
        self.data_tab_generation = self.data_tab_generation.wrapping_add(1);
        if request.tool_request.is_some() {
            self.tool_state.loading = true;
            self.tool_state.load_error = None;
        }
        let db = self.db.clone();
        Task::perform(load_profile_context(db, request), move |result| {
            Message::ProfileContextLoaded { generation, result }
        })
    }

    /// Apply a resolved [`ProfileContextSnapshot`] into `self` — the synchronous
    /// tail of the old `reload_profile`/`switch_game_context`/
    /// `refresh_data_tab_conflicts`/`refresh_tools_state` helpers.
    pub(super) fn apply_profile_context(&mut self, snapshot: ProfileContextSnapshot) {
        self.profiles = snapshot.profiles;
        self.active_profile = snapshot.active_profile;
        match snapshot.profile_outcome {
            ProfileLoadOutcome::Loaded {
                profile,
                experiment_depth,
                current_fingerprint,
                mod_id_filter_keys,
            } => {
                self.experiment_depth = experiment_depth;
                self.current_fingerprint = current_fingerprint;
                self.mod_id_filter_keys = mod_id_filter_keys;
                self.loaded_profile = Some(*profile);
            }
            ProfileLoadOutcome::Cleared => {
                self.loaded_profile = None;
                self.mod_id_filter_keys.clear();
            }
            ProfileLoadOutcome::KeepPrevious => {}
        }
        self.data_tab_conflicts = snapshot.data_tab_conflicts;
        self.data_tab_state.missing_store_mod_count = snapshot.missing_store_mod_count;
        self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Idle;
        if let Some(tools) = snapshot.tools {
            self.apply_tool_snapshot(tools);
        } else {
            // No game in scope — clear the tool state (mirrors the old
            // `refresh_tools_state` empty-game branch).
            self.tool_state.entries.clear();
            self.tool_state.active_tool_id = None;
            self.tool_state.game_label = None;
            self.tool_state.game_dir_configured = false;
            self.tool_state.loading = false;
            self.tool_state.load_error = None;
        }
    }

    /// Synchronously drive [`load_profile_context`] + [`Self::apply_profile_context`]
    /// for tests, which pump `update` but discard returned `Task`s.
    #[cfg(test)]
    pub(super) fn reload_profile_blocking(&mut self) {
        let request = self.reload_request(false);
        let snapshot = crate::app::block_on(load_profile_context(self.db.clone(), request))
            .expect("load profile context");
        self.apply_profile_context(snapshot);
    }

    /// Synchronously complete the game-context switch that a `SelectGame` /
    /// `GamePathDialogPathSelected` handler just kicked off (the returned
    /// `Task` is discarded by the test harness). Mirrors the request
    /// `switch_game_context` builds, using the already-set `selected_game`.
    #[cfg(test)]
    pub(super) fn finish_pending_switch_blocking(&mut self) {
        let game_id = self
            .selected_game
            .clone()
            .expect("selected_game set by the switch kickoff");
        let request = ProfileContextRequest {
            selected_game: Some(game_id.clone()),
            active_profile: None,
            recompute_active: true,
            fallback_profile: None,
            tool_request: Some(self.tool_load_request_for(&game_id)),
            rerun_diagnostics: false,
        };
        let snapshot = crate::app::block_on(load_profile_context(self.db.clone(), request))
            .expect("load profile context");
        self.apply_profile_context(snapshot);
    }

    /// Switch the game context: clear game-scoped state and the previously
    /// loaded profile (so the view repaints to a "loading" state immediately
    /// rather than showing the old game's data), then reload the new game's
    /// profile/conflicts/tools off-thread. Returns the resolving `Task`.
    pub(super) fn switch_game_context(&mut self, game_id: &str) -> Task<Message> {
        self.clear_game_scoped_state();
        self.loaded_profile = None;
        self.mod_id_filter_keys.clear();
        let request = ProfileContextRequest {
            selected_game: Some(game_id.to_string()),
            active_profile: None,
            recompute_active: true,
            fallback_profile: None,
            tool_request: Some(self.tool_load_request_for(game_id)),
            rerun_diagnostics: false,
        };
        self.dispatch_profile_context(request)
    }

    pub(super) fn accept_game_selection(
        &mut self,
        game_id: String,
        previous_game: Option<String>,
    ) -> Task<Message> {
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
                return Task::none();
            }
        }

        self.game_path_dialog_open = false;
        self.pending_game_path_game_id = None;
        self.previous_game_before_path_dialog = None;
        self.game_path_dialog_error = None;
        let task = self.switch_game_context(&game_id);
        self.sync_browse_game_to_current(true);
        self.save_settings();
        self.status_message = format!("Active game set to {game_id}");
        task
    }

    pub(super) fn save_settings(&self) {
        self.settings.save();
    }

    pub(super) fn refresh_available_games(&mut self) {
        self.available_games = modde_games::supported_games()
            .iter()
            .map(|(id, name)| (id.to_string(), name.to_string()))
            .collect();
        self.detected_games = detected_game_ids(&self.settings, self.available_games.as_slice());
    }

    pub(super) fn custom_games(&self) -> Vec<(String, String)> {
        self.available_games
            .iter()
            .filter(|(id, _)| !modde_games::SUPPORTED_GAME_IDS.contains(&id.as_str()))
            .cloned()
            .collect()
    }

    pub(super) fn current_game_id(&self) -> Option<&str> {
        self.loaded_profile
            .as_ref()
            .map(|profile| profile.game_id.as_str())
            .or(self.selected_game.as_deref())
    }

    pub(super) fn current_game_dir(&self) -> Option<PathBuf> {
        let game_id = self.current_game_id()?;
        self.settings
            .game_path(&GameId::from(game_id))
            .cloned()
            .or_else(|| {
                modde_games::resolve_game_plugin(game_id)
                    .and_then(modde_games::GamePlugin::detect_install)
            })
    }

    pub(super) fn add_custom_game_modal(&self) -> Element<'_, Message> {
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

    pub(super) fn manage_custom_games_modal(&self) -> Element<'_, Message> {
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

    pub(super) fn current_tool_game_context(&self) -> Option<modde_games::tools::ToolGameContext> {
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

    /// Refresh the Data tab's conflict rows off the render thread. Returns the
    /// `Task` resolving to `Message::DataTabConflictsLoaded`. The heavy
    /// `analyze_profile_state` pass and hidden-files DB read run on a blocking
    /// task rather than inline via `block_on`.
    pub(super) fn refresh_data_tab_conflicts(&mut self) -> Task<Message> {
        let Some(profile) = self.loaded_profile.clone() else {
            self.data_tab_conflicts.clear();
            self.data_tab_state.missing_store_mod_count = 0;
            return Task::none();
        };
        self.data_tab_generation = self.data_tab_generation.wrapping_add(1);
        let generation = self.data_tab_generation;
        let db = self.db.clone();
        Task::perform(load_data_tab_conflicts(db, profile), move |result| {
            Message::DataTabConflictsLoaded { generation, result }
        })
    }

    pub(super) fn start_diagnostics_load(&mut self) -> Task<Message> {
        let Some(profile) = self.loaded_profile.clone() else {
            self.status_message = "Select a profile before running diagnostics".to_string();
            self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Error(
                "Select a profile before running diagnostics.".to_string(),
            );
            return Task::none();
        };
        self.diagnostics_generation = self.diagnostics_generation.wrapping_add(1);
        let generation = self.diagnostics_generation;
        self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Running;
        self.status_message = "Running diagnostics...".to_string();
        Task::perform(load_diagnostics(self.db.clone(), profile), move |result| {
            Message::DiagnosticsComputed { generation, result }
        })
    }

    pub(super) fn apply_diagnostics_computed(&mut self, computed: DiagnosticsComputed) {
        let diagnostic_count = computed.report.entries.len();
        let broken_count = computed.report.integrity.broken_symlinks.len();
        self.data_tab_state.missing_store_mod_count = computed.missing_store_mod_count;
        self.data_tab_conflicts = computed.data_tab_conflicts;
        self.diagnostics_state =
            crate::views::diagnostics::DiagnosticsState::Complete(computed.report);
        self.status_message = if diagnostic_count == 0 && broken_count == 0 {
            "Diagnostics complete: no issues found".to_string()
        } else {
            format!(
                "Diagnostics complete: {diagnostic_count} issue(s), {broken_count} broken symlink(s)"
            )
        };
    }

    pub(super) fn verify_staging_integrity(
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

    pub(super) fn start_tools_load(&mut self) -> Task<Message> {
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
        let db = self.db.clone();
        Task::perform(load_tools_state(db, request), move |result| {
            Message::ToolsLoaded { generation, result }
        })
    }

    pub(super) fn tool_load_request(&self) -> Option<ToolLoadRequest> {
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

    /// Build a tool-load request for an explicit game id, rather than relying on
    /// `current_game_id()`. Used by `switch_game_context`, where `loaded_profile`
    /// has just been cleared so `current_game_id()` could not be trusted.
    pub(super) fn tool_load_request_for(&self, game_id: &str) -> ToolLoadRequest {
        let display_name = self
            .available_games
            .iter()
            .find(|(id, _)| id == game_id)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| game_id.to_string());
        ToolLoadRequest {
            game_id: game_id.to_string(),
            display_name,
            configured_game_dir: self.settings.game_path(&GameId::from(game_id)).cloned(),
            optiscaler_releases: self.tool_state.optiscaler_releases.clone(),
            tool_option_catalog: self.tool_state.tool_option_catalog.clone(),
            previous_active_tool_id: self.tool_state.active_tool_id.clone(),
        }
    }

    pub(super) fn apply_tool_snapshot(&mut self, snapshot: ToolLoadSnapshot) {
        self.tool_state.entries = snapshot.entries;
        self.tool_state.active_tool_id = snapshot.active_tool_id;
        self.tool_state.game_label = snapshot.game_label;
        self.tool_state.game_dir_configured = snapshot.game_dir_configured;
        self.tool_state.tool_option_catalog = snapshot.tool_option_catalog;
        self.tool_state.executables = snapshot.executables;
        self.tool_state.loading = false;
        self.tool_state.load_error = None;
    }

    pub(super) fn start_executables_load(&mut self) -> Task<Message> {
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
        let db = self.db.clone();
        Task::perform(load_executables_for_game(db, game_id), move |result| {
            Message::ExecutablesLoaded { generation, result }
        })
    }

    pub(super) fn refresh_executables_or_tools(&mut self) -> Task<Message> {
        if matches!(self.active_view, View::Executables) {
            self.start_executables_load()
        } else if self.tool_load_request().is_some() {
            self.start_tools_load()
        } else {
            Task::none()
        }
    }

    pub(super) fn track_download(&mut self, key: &str, name: &str) -> usize {
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

    pub(super) fn downloads_view_tasks(&self) -> Vec<crate::views::downloads::DownloadTask> {
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

    pub(super) fn nexus_domain_for_game(game_id: &str) -> Option<String> {
        let game = modde_games::resolve_game(game_id)?;
        game.nexus_game_id?;
        game.nexus_domain.map(str::to_string)
    }

    pub(super) fn first_supported_nexus_game(&self) -> Option<String> {
        self.available_games
            .iter()
            .map(|(id, _)| id)
            .find(|id| Self::nexus_domain_for_game(id).is_some())
            .cloned()
    }

    pub(super) fn default_browse_game_id(&self) -> Option<String> {
        self.current_game_id()
            .filter(|game_id| Self::nexus_domain_for_game(game_id).is_some())
            .map(str::to_string)
            .or_else(|| self.first_supported_nexus_game())
    }

    pub(super) fn browse_game_nexus_domain(&self) -> Option<String> {
        self.browse_nexus
            .selected_game_id
            .as_deref()
            .and_then(Self::nexus_domain_for_game)
    }

    pub(super) fn clear_browse_results(&mut self) {
        self.browse_nexus.mods.clear();
        self.browse_nexus.collections.clear();
        self.browse_nexus.error = None;
        self.browse_nexus.install_status = None;
    }

    pub(super) fn sync_browse_game_to_current(&mut self, force: bool) {
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

    pub(super) fn initialize_wabbajack_game_filter(&self, state: &mut WabbajackInstallerState) {
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

/// Off-thread loader for the profile + data-tab + tool context. Mirrors
/// [`super::tool_ops::load_tools_state`]: an async wrapper around a synchronous
/// body so every `sqlx` `block_on` runs on a blocking task, never on the iced
/// executor. Folds the three reads that must stay mutually consistent
/// (profile, data-tab conflicts, tools) into one job.
pub(super) async fn load_profile_context(
    db: modde_core::db::ModdeDb,
    request: ProfileContextRequest,
) -> Result<ProfileContextSnapshot, String> {
    tokio::task::spawn_blocking(move || load_profile_context_blocking(db, request))
        .await
        .map_err(|err| err.to_string())?
}

fn load_profile_context_blocking(
    db: modde_core::db::ModdeDb,
    request: ProfileContextRequest,
) -> Result<ProfileContextSnapshot, String> {
    let pm = ProfileManager::with_db(db.clone());
    let selected_game_id = request.selected_game.as_deref().map(GameId::from);

    // Profile list (scoped to the game when one is selected, else all).
    let profiles = match selected_game_id.as_ref() {
        Some(game_id) => crate::app::block_on(pm.list_for_game(game_id)).unwrap_or_default(),
        None => crate::app::block_on(pm.list()).unwrap_or_default(),
    };

    // Active profile: recompute from the DB for game switches; otherwise trust
    // the caller-provided name.
    let active_profile = if request.recompute_active {
        selected_game_id
            .as_ref()
            .and_then(|game_id| crate::app::block_on(pm.active(game_id)).ok().flatten())
            .map(|info| info.profile.name)
            .or_else(|| profiles.first().map(|profile| profile.name.clone()))
    } else {
        request.active_profile.clone()
    };

    // Load the active profile.
    let profile_outcome = match active_profile.as_deref() {
        Some(name) => match crate::app::block_on(pm.load(name, selected_game_id.as_ref())) {
            Ok(profile) => {
                let experiment_depth = crate::app::block_on(pm.active(&profile.game_id))
                    .ok()
                    .flatten()
                    .map_or(0, |info| info.experiment_depth);
                let current_fingerprint = compute_save_fingerprint(&profile);
                let mod_id_filter_keys = modde_core::filter::mod_id_filter_keys(&profile.mods);
                ProfileLoadOutcome::Loaded {
                    profile: Box::new(profile),
                    experiment_depth,
                    current_fingerprint,
                    mod_id_filter_keys,
                }
            }
            // Load failed — keep the previously-loaded profile (and its derived
            // fields) untouched, as the old synchronous helper did.
            Err(_) => ProfileLoadOutcome::KeepPrevious,
        },
        None => ProfileLoadOutcome::Cleared,
    };

    // Data-tab conflicts come from whichever profile will be displayed: the
    // freshly loaded one, the kept previous one, or none.
    let effective_profile = match &profile_outcome {
        ProfileLoadOutcome::Loaded { profile, .. } => Some(profile.as_ref()),
        ProfileLoadOutcome::KeepPrevious => request.fallback_profile.as_ref(),
        ProfileLoadOutcome::Cleared => None,
    };
    // On a data-tab analysis failure the conflicts are cleared (matching the
    // old behavior). The error is NOT surfaced via `status_message` here: every
    // composite-load caller set its own status after the old synchronous
    // `reload_profile`, so the data-tab error was never visible on this path.
    // The standalone `load_data_tab_conflicts` (Data-tab open) still surfaces it.
    let (data_tab_conflicts, missing_store_mod_count) = match effective_profile {
        Some(profile) => compute_data_tab_conflicts(&pm, profile).unwrap_or_default(),
        None => (Vec::new(), 0),
    };

    // Fold in the tool reload when a game is in scope.
    let tools = match request.tool_request {
        Some(tool_request) => Some(load_tools_state_blocking(db, tool_request)?),
        None => None,
    };

    Ok(ProfileContextSnapshot {
        profiles,
        active_profile,
        profile_outcome,
        data_tab_conflicts,
        missing_store_mod_count,
        tools,
        rerun_diagnostics: request.rerun_diagnostics,
    })
}

/// Compute the save fingerprint for a profile (lifted verbatim from the old
/// `reload_profile`). `None` when the game doesn't support save profiles.
fn compute_save_fingerprint(
    profile: &modde_core::Profile,
) -> Option<modde_core::save::SaveFingerprint> {
    let game_id = profile.game_id.as_str();
    let staging_dir = ProfileManager::staging_dir(&profile.name);
    modde_games::resolve_game_plugin(game_id)
        .filter(|plugin| plugin.supports_save_profiles())
        .map(|plugin| {
            modde_core::save::SaveFingerprint::compute(&profile.mods, |mod_id| {
                let mod_path = staging_dir.join(mod_id);
                plugin.classify_mod(&mod_path).affects_saves()
            })
        })
}

/// Compute data-tab conflict rows + missing-store count for a profile. Shared
/// by [`load_profile_context`] and [`load_data_tab_conflicts`].
fn compute_data_tab_conflicts(
    pm: &ProfileManager,
    profile: &modde_core::Profile,
) -> Result<(Vec<(String, Vec<String>)>, usize), String> {
    let hidden = load_hidden_files(pm, profile);
    let classifier = modde_games::resolve_collision_classifier(profile.game_id.as_str());
    match modde_core::diagnostics::analyze_profile_state(
        profile,
        &modde_core::paths::store_dir(),
        &hidden,
        classifier.as_deref(),
    ) {
        Ok(analysis) => Ok((
            build_conflict_rows(&analysis, &hidden),
            analysis.missing_store_mods.len(),
        )),
        Err(err) => Err(format!("Failed to load data tab: {err}")),
    }
}

/// Off-thread lightweight data-tab conflict refresh (fired when the Data tab is
/// opened).
pub(super) async fn load_data_tab_conflicts(
    db: modde_core::db::ModdeDb,
    profile: modde_core::Profile,
) -> Result<DataTabConflicts, String> {
    tokio::task::spawn_blocking(move || {
        let pm = ProfileManager::with_db(db);
        compute_data_tab_conflicts(&pm, &profile).map(|(conflicts, missing_store_mod_count)| {
            DataTabConflicts {
                conflicts,
                missing_store_mod_count,
            }
        })
    })
    .await
    .map_err(|err| err.to_string())?
}

pub(super) async fn load_diagnostics(
    db: modde_core::db::ModdeDb,
    profile: modde_core::Profile,
) -> Result<DiagnosticsComputed, String> {
    tokio::task::spawn_blocking(move || load_diagnostics_blocking(db, profile))
        .await
        .map_err(|err| err.to_string())?
}

fn load_diagnostics_blocking(
    db: modde_core::db::ModdeDb,
    profile: modde_core::Profile,
) -> Result<DiagnosticsComputed, String> {
    let pm = ProfileManager::with_db(db);
    let hidden = load_hidden_files(&pm, &profile);
    let active_plugins = load_active_plugins(&pm, &profile);
    let integrity = Modde::verify_staging_integrity(&ProfileManager::staging_dir(&profile.name));
    let engine = match profile.game_id.as_str() {
        "skyrim-se" | "skyrim-ae" | "fallout4" | "fallout76" => {
            modde_games::bethesda::diagnostics::bethesda_diagnostics()
        }
        _ => modde_core::diagnostics::base_diagnostics(),
    };
    let classifier = modde_games::resolve_collision_classifier(profile.game_id.as_str());
    let (diagnostics, analysis) = modde_core::diagnostics::run_profile_diagnostics(
        profile.game_id.as_str(),
        &profile,
        &active_plugins,
        &modde_core::paths::store_dir(),
        &ProfileManager::staging_dir(&profile.name),
        &hidden,
        classifier.as_deref(),
        &engine,
    )
    .map_err(|err| err.to_string())?;
    let entries = diagnostics.iter().map(format_diagnostic_entry).collect();
    Ok(DiagnosticsComputed {
        report: crate::views::diagnostics::DiagnosticsReport {
            profile_name: profile.name.clone(),
            game_id: profile.game_id.to_string(),
            entries,
            integrity,
        },
        data_tab_conflicts: build_conflict_rows(&analysis, &hidden),
        missing_store_mod_count: analysis.missing_store_mods.len(),
    })
}
