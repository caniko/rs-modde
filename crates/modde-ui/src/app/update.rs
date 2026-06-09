use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use iced::{Task, window};
use modde_core::filter::{FilterCriterion, FilterKind, FilterMode};
use modde_core::manifest::collection::CollectionManifest;
use modde_core::profile::ProfileManager;
use modde_core::resolver::GameId;
use modde_core::settings::AppSettings;
use smallvec::SmallVec;

use super::install_ops::{
    download_wabbajack_source, format_anyhow_error, run_browse_install,
    run_wabbajack_install_for_ui, slugify_profile_name,
};
use super::profile_ops::{
    add_mod_to_profile, create_profile, delete_profile, fork_profile, remove_mod_from_profile,
    reorder_mod, run_experiment_write, set_mod_lock, toggle_mod_enabled,
};
use super::state::{empty_to_none, prefill_wabbajack_game_dir};
use super::tool_ops::{
    apply_tool_for_game, deactivate_optiscaler_for_game, executable_draft_to_row,
    install_selected_proton_version, install_selected_tool_release, load_proton_versions,
    load_tool_releases, remove_executable_for_game, revert_tool_for_game,
    run_saved_executable_for_game, save_executable_for_game,
};
use super::tool_settings::{
    adopt_optiscaler_for_game, reset_optiscaler_config_for_game, restore_tool_settings_for_game,
    save_optiscaler_release_selection_for_game, save_proton_selected_version_for_game,
    save_tool_setting_for_game, set_tool_options, toggle_tool_for_game, tool_options,
};
use super::{
    AddCustomGameDraftField, AddCustomGameState, BUTTON_HOVER_TOAST_DELAY, ButtonHoverToast,
    ButtonHoverToastState, ExecutableDraft, ExecutableDraftField, ExperimentWriteKind,
    FOMODWizardState, Message, Modde, NexusAuthStatus, ProfileWriteKind, SidebarGroup, View,
    detected_game_ids, resize_thumbnail_bytes,
};

impl Modde {
    pub(super) fn new() -> (Self, Task<Message>) {
        let settings = AppSettings::load();
        let theme_name = if settings.theme.is_empty() {
            "Dark".to_string()
        } else {
            settings.theme.clone()
        };
        let selected_game = settings.selected_game.clone();

        // TODO(phase-02): surface startup database failures as a Task error state.
        let db = crate::app::block_on(modde_core::db::ModdeDb::open())
            .expect("failed to open modde database at startup");
        let all_profiles =
            crate::app::block_on(ProfileManager::with_db(db.clone()).list()).unwrap_or_default();

        let available_games: SmallVec<[(String, String); 8]> = modde_games::supported_games()
            .iter()
            .map(|(id, name)| (id.to_string(), name.to_string()))
            .collect();
        let detected_games = detected_game_ids(&settings, available_games.as_slice());

        let mut app = Self {
            db,
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
            context_generation: 0,
            data_tab_generation: 0,
            diagnostics_generation: 0,
        };
        app.refresh_nexus_api_key_state();

        // Auto-detect: if no game is selected but profiles exist, pick the first profile's game
        if app.selected_game.is_none()
            && let Some(first) = all_profiles.first()
        {
            app.selected_game = Some(first.game_id.to_string());
            app.settings.selected_game = Some(first.game_id.to_string());
        }

        // Kick off the first profile/game-context load asynchronously. The
        // heavy multi-query reload (profile + conflicts + tools) resolves off
        // the render thread via `Message::ProfileContextLoaded`, so the first
        // paint is immediate. The one-time `list()` above is only used for the
        // auto-detect above and is not stored in `app.profiles` (which fills in
        // from the async load).
        let initial_context_task = if let Some(game_id) = app.selected_game.clone() {
            app.accept_game_selection(game_id, None)
        } else {
            Task::none()
        };

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
                initial_context_task,
            ]),
        )
    }

    pub(super) fn title(&self) -> String {
        "modde".to_string()
    }

    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // External process pushed a refresh signal (CLI install,
            // update apply, uninstall, …). Re-read the profile from
            // the shared sqlite DB so the user sees the change without
            // restarting or switching profiles.
            Message::ExternalRefresh => {
                let task = self.reload_profile();
                self.status_message = "Refreshed from external change".to_string();
                return task;
            }
            Message::ProfileContextLoaded { generation, result } => {
                // Drop stale loads: a newer kickoff bumped the generation, so
                // this result is for a game/profile the user has moved on from.
                if generation != self.context_generation {
                    return Task::none();
                }
                match result {
                    Ok(snapshot) => {
                        let rerun_diagnostics = snapshot.rerun_diagnostics;
                        self.apply_profile_context(snapshot);
                        if rerun_diagnostics && matches!(self.active_view, View::Diagnostics) {
                            return self.update(Message::RunDiagnostics);
                        }
                    }
                    Err(err) => {
                        self.tool_state.loading = false;
                        self.status_message = format!("Failed to load profile: {err}");
                    }
                }
            }
            Message::DataTabConflictsLoaded { generation, result } => {
                if generation != self.data_tab_generation {
                    return Task::none();
                }
                match result {
                    Ok(conflicts) => {
                        self.data_tab_conflicts = conflicts.conflicts;
                        self.data_tab_state.missing_store_mod_count =
                            conflicts.missing_store_mod_count;
                    }
                    Err(err) => {
                        self.data_tab_conflicts.clear();
                        self.data_tab_state.missing_store_mod_count = 0;
                        self.status_message = err;
                    }
                }
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
                    return self.refresh_data_tab_conflicts();
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
                // The reload re-runs diagnostics once it resolves if the
                // Diagnostics view is active (it needs the freshly loaded
                // profile), preserving the old synchronous behavior.
                let task = self.reload_profile_refresh_diagnostics();
                self.sync_browse_game_to_current(true);
                self.selected_save_details = None;
                self.status_message = "Profile switched".to_string();
                return task;
            }
            Message::CreateProfile { name, game_id } => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    self.status_message = "Profile name is required".to_string();
                    return Task::none();
                }
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
                self.context_generation = self.context_generation.wrapping_add(1);
                let generation = self.context_generation;
                self.status_message = format!("Creating profile '{name}'...");
                return Task::perform(create_profile(self.db.clone(), profile), move |result| {
                    Message::ProfileWriteDone {
                        generation,
                        kind: ProfileWriteKind::Create {
                            name: name.clone(),
                            game_id: game_id.clone(),
                        },
                        result,
                    }
                });
            }
            Message::DeleteProfile(name) => {
                self.context_generation = self.context_generation.wrapping_add(1);
                let generation = self.context_generation;
                self.status_message = format!("Deleting profile '{name}'...");
                return Task::perform(
                    delete_profile(self.db.clone(), name.clone()),
                    move |result| Message::ProfileWriteDone {
                        generation,
                        kind: ProfileWriteKind::Delete { name: name.clone() },
                        result,
                    },
                );
            }
            Message::ForkProfile { source, new_name } => {
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    self.context_generation = self.context_generation.wrapping_add(1);
                    let generation = self.context_generation;
                    self.status_message = format!("Forking profile as '{new_name}'...");
                    return Task::perform(
                        fork_profile(self.db.clone(), source, new_name.clone(), game_id),
                        move |result| Message::ProfileWriteDone {
                            generation,
                            kind: ProfileWriteKind::Fork {
                                new_name: new_name.clone(),
                            },
                            result,
                        },
                    );
                }
            }
            Message::ProfileWriteDone {
                generation,
                kind,
                result,
            } => {
                if generation != self.context_generation {
                    return Task::none();
                }
                match result {
                    Ok(outcome) => {
                        match &kind {
                            ProfileWriteKind::Create { name, game_id } => {
                                self.active_profile = Some(name.clone());
                                self.selected_game = Some(game_id.clone());
                                self.settings.selected_game = Some(game_id.clone());
                                self.save_settings();
                                self.new_profile_name.clear();
                                self.new_profile_dialog_open = false;
                            }
                            ProfileWriteKind::Fork { new_name } => {
                                self.active_profile = Some(new_name.clone());
                            }
                            ProfileWriteKind::RemoveMod => {
                                self.selected_mod_index = None;
                            }
                            _ => {}
                        }
                        if let Some(status) = outcome.status_message {
                            self.status_message = status;
                        }
                        if outcome.reload {
                            return match kind {
                                ProfileWriteKind::Delete { name } => {
                                    if let Some(game_id) = self.selected_game.clone() {
                                        self.switch_game_context(&game_id)
                                    } else if self.active_profile.as_deref() == Some(&name) {
                                        self.reload_profile_recompute_active()
                                    } else {
                                        self.reload_profile()
                                    }
                                }
                                _ => self.reload_profile(),
                            };
                        }
                    }
                    Err(err) => {
                        self.status_message = match kind {
                            ProfileWriteKind::Create { .. } => {
                                format!("Failed to create profile: {err}")
                            }
                            ProfileWriteKind::Delete { .. } => {
                                format!("Failed to delete profile: {err}")
                            }
                            ProfileWriteKind::Fork { .. } => format!("Fork failed: {err}"),
                            ProfileWriteKind::AddMod { .. } => format!("Failed to add mod: {err}"),
                            ProfileWriteKind::RemoveMod => {
                                format!("Failed to remove mod: {err}")
                            }
                            ProfileWriteKind::ToggleMod { .. } => {
                                format!("Failed to update mod: {err}")
                            }
                            ProfileWriteKind::Reorder { .. } => {
                                format!("Failed to reorder mod: {err}")
                            }
                            ProfileWriteKind::Lock { .. } => {
                                format!("Failed to pin mod: {err}")
                            }
                            ProfileWriteKind::Unlock { .. } => {
                                format!("Failed to unpin mod: {err}")
                            }
                        };
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
                return self.accept_game_selection(game_id, previous_game);
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
                let task = self.switch_game_context(&game_id);
                self.save_settings();
                self.status_message = format!("Active game set to {game_id}");
                return task;
            }
            Message::CancelGamePathDialog => {
                let previous = self.previous_game_before_path_dialog.clone();
                self.game_path_dialog_open = false;
                self.pending_game_path_game_id = None;
                self.previous_game_before_path_dialog = None;
                self.game_path_dialog_error = None;
                self.selected_game = previous.clone();
                self.settings.selected_game = previous.clone();
                let task = if let Some(game_id) = previous {
                    let task = self.switch_game_context(&game_id);
                    self.status_message = format!("Active game remains {game_id}");
                    task
                } else {
                    self.clear_game_scoped_state();
                    self.profiles.clear();
                    self.active_profile = None;
                    self.loaded_profile = None;
                    self.mod_id_filter_keys.clear();
                    self.status_message = "Game selection cancelled".to_string();
                    Task::none()
                };
                self.save_settings();
                return task;
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
                        let previous_game = self.selected_game.clone();
                        let task = self.accept_game_selection(spec.id.clone(), previous_game);
                        self.status_message =
                            format!("Registered custom game '{}'", spec.display_name);
                        return task;
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
                    let task = if self.selected_game.is_none()
                        && let Some((game_id, _)) = self.available_games.first().cloned()
                    {
                        self.accept_game_selection(game_id, None)
                    } else {
                        Task::none()
                    };
                    self.save_settings();
                    self.status_message = format!("Removed custom game '{id}'");
                    return task;
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
                if let Some(ref profile_name) = self.active_profile {
                    self.context_generation = self.context_generation.wrapping_add(1);
                    let generation = self.context_generation;
                    let profile_name = profile_name.clone();
                    self.status_message = format!(
                        "{} '{mod_id}'...",
                        if enabled { "Enabling" } else { "Disabling" }
                    );
                    return Task::perform(
                        toggle_mod_enabled(self.db.clone(), profile_name, mod_id.clone(), enabled),
                        move |result| Message::ProfileWriteDone {
                            generation,
                            kind: ProfileWriteKind::ToggleMod {
                                mod_id: mod_id.clone(),
                                enabled,
                            },
                            result,
                        },
                    );
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
                    self.context_generation = self.context_generation.wrapping_add(1);
                    let generation = self.context_generation;
                    let profile_name = profile_name.clone();
                    self.status_message = format!("Adding mod: {mod_name}...");
                    return Task::perform(
                        add_mod_to_profile(self.db.clone(), profile_name, mod_name.clone()),
                        move |result| Message::ProfileWriteDone {
                            generation,
                            kind: ProfileWriteKind::AddMod {
                                mod_id: mod_name.clone(),
                            },
                            result,
                        },
                    );
                }
                self.status_message = "No active profile — create one first".to_string();
            }
            Message::RemoveMod(index) => {
                if let Some(ref profile_name) = self.active_profile {
                    self.context_generation = self.context_generation.wrapping_add(1);
                    let generation = self.context_generation;
                    let profile_name = profile_name.clone();
                    self.status_message = "Removing mod...".to_string();
                    return Task::perform(
                        remove_mod_from_profile(self.db.clone(), profile_name, index),
                        move |result| Message::ProfileWriteDone {
                            generation,
                            kind: ProfileWriteKind::RemoveMod,
                            result,
                        },
                    );
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
                    let db = self.db.clone();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                deploy_profile_blocking(db, profile_name, game_id)
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
                self.context_generation = self.context_generation.wrapping_add(1);
                let generation = self.context_generation;
                let profile_name = profile_name.clone();
                return Task::perform(
                    reorder_mod(self.db.clone(), profile_name, mod_id.clone(), direction),
                    move |result| Message::ProfileWriteDone {
                        generation,
                        kind: ProfileWriteKind::Reorder {
                            mod_id: mod_id.clone(),
                            direction,
                        },
                        result,
                    },
                );
            }

            Message::LockMod { mod_id } => {
                let Some(ref profile_name) = self.active_profile else {
                    return Task::none();
                };
                self.context_generation = self.context_generation.wrapping_add(1);
                let generation = self.context_generation;
                let profile_name = profile_name.clone();
                self.status_message = format!("Pinning '{mod_id}'...");
                return Task::perform(
                    set_mod_lock(self.db.clone(), profile_name, mod_id.clone(), true),
                    move |result| Message::ProfileWriteDone {
                        generation,
                        kind: ProfileWriteKind::Lock {
                            mod_id: mod_id.clone(),
                        },
                        result,
                    },
                );
            }
            Message::UnlockMod { mod_id } => {
                let Some(ref profile_name) = self.active_profile else {
                    return Task::none();
                };
                self.context_generation = self.context_generation.wrapping_add(1);
                let generation = self.context_generation;
                let profile_name = profile_name.clone();
                self.status_message = format!("Unpinning '{mod_id}'...");
                return Task::perform(
                    set_mod_lock(self.db.clone(), profile_name, mod_id.clone(), false),
                    move |result| Message::ProfileWriteDone {
                        generation,
                        kind: ProfileWriteKind::Unlock {
                            mod_id: mod_id.clone(),
                        },
                        result,
                    },
                );
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
                let db = self.db.clone();
                return Task::perform(
                    async move { run_browse_install(db, game_domain, mod_id).await },
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
                    return self.reload_profile();
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
                let db = self.db.clone();
                return Task::perform(
                    async move { run_wabbajack_install_for_ui(db, path, profile_name, game_dir).await },
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
                            return self.reload_profile();
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
                    let save_dir = Self::resolve_save_dir(game_id.as_str());
                    self.context_generation = self.context_generation.wrapping_add(1);
                    let generation = self.context_generation;
                    let current_depth = self.experiment_depth;
                    self.status_message = "Starting experiment...".to_string();
                    return Task::perform(
                        run_experiment_write(
                            self.db.clone(),
                            ExperimentWriteKind::Try,
                            Some(name),
                            game_id,
                            save_dir,
                            current_depth,
                        ),
                        move |result| Message::ExperimentWriteDone {
                            generation,
                            kind: ExperimentWriteKind::Try,
                            result,
                        },
                    );
                }
            }
            Message::RollbackExperiment => {
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    let save_dir = Self::resolve_save_dir(game_id.as_str());
                    self.context_generation = self.context_generation.wrapping_add(1);
                    let generation = self.context_generation;
                    let current_depth = self.experiment_depth;
                    self.status_message = "Rolling back experiment...".to_string();
                    return Task::perform(
                        run_experiment_write(
                            self.db.clone(),
                            ExperimentWriteKind::Rollback,
                            None,
                            game_id,
                            save_dir,
                            current_depth,
                        ),
                        move |result| Message::ExperimentWriteDone {
                            generation,
                            kind: ExperimentWriteKind::Rollback,
                            result,
                        },
                    );
                }
            }
            Message::CommitExperiment => {
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    self.context_generation = self.context_generation.wrapping_add(1);
                    let generation = self.context_generation;
                    let current_depth = self.experiment_depth;
                    self.status_message = "Committing experiment...".to_string();
                    return Task::perform(
                        run_experiment_write(
                            self.db.clone(),
                            ExperimentWriteKind::Commit,
                            None,
                            game_id,
                            None,
                            current_depth,
                        ),
                        move |result| Message::ExperimentWriteDone {
                            generation,
                            kind: ExperimentWriteKind::Commit,
                            result,
                        },
                    );
                }
            }
            Message::ExperimentWriteDone {
                generation,
                kind,
                result,
            } => {
                if generation != self.context_generation {
                    return Task::none();
                }
                match result {
                    Ok(outcome) => {
                        if let Some(previous_profile) = outcome.previous_profile {
                            self.active_profile = Some(previous_profile);
                        }
                        if matches!(kind, ExperimentWriteKind::Try) {
                            self.experiment_depth = self.experiment_depth.saturating_add(1);
                        } else if matches!(kind, ExperimentWriteKind::Commit) {
                            self.experiment_depth = 0;
                        }
                        self.status_message = outcome.status_message;
                        if outcome.reload {
                            return self.reload_profile();
                        }
                    }
                    Err(err) => {
                        self.status_message = match kind {
                            ExperimentWriteKind::Try => format!("Try failed: {err}"),
                            ExperimentWriteKind::Rollback => format!("Rollback failed: {err}"),
                            ExperimentWriteKind::Commit => format!("Commit failed: {err}"),
                        };
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
                return self.start_diagnostics_load();
            }
            Message::DiagnosticsComputed { generation, result } => {
                if generation != self.diagnostics_generation {
                    return Task::none();
                }
                match result {
                    Ok(computed) => self.apply_diagnostics_computed(computed),
                    Err(err) => {
                        self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Error(
                            format!("Diagnostics failed: {err}"),
                        );
                        self.status_message = format!("Diagnostics failed: {err}");
                    }
                }
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
            Message::ToolSettingWritten { tool_id, result } => match result {
                Ok(result) => {
                    if let Some(option_catalog) = result.tool_option_catalog {
                        self.tool_state.tool_option_catalog = option_catalog;
                    }
                    self.tool_state.active_tool_id = Some(tool_id);
                    self.status_message = result.status_message;
                    self.pending_tools_load_status_message = Some(self.status_message.clone());
                    return self.start_tools_load();
                }
                Err(err) => {
                    self.status_message = format!("Failed to update tool setting: {err}");
                }
            },
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
                        self.tool_state.optiscaler_releases = releases;
                        self.tool_state.active_tool_id = Some("optiscaler".to_string());
                        self.status_message = "Loading OptiScaler release settings...".to_string();
                        return Task::perform(
                            save_optiscaler_release_selection_for_game(
                                self.db.clone(),
                                game_id,
                                self.tool_state.optiscaler_releases.clone(),
                                self.tool_state.tool_option_catalog.clone(),
                                self.current_tool_game_context(),
                            ),
                            |result| Message::ToolSettingWritten {
                                tool_id: "optiscaler".to_string(),
                                result,
                            },
                        );
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
                    install_selected_tool_release(
                        self.db.clone(),
                        game_id,
                        "optiscaler".to_string(),
                    ),
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
                        self.tool_state.active_tool_id = Some("proton".to_string());
                        self.status_message = "Loading Proton version settings...".to_string();
                        return Task::perform(
                            save_proton_selected_version_for_game(
                                self.db.clone(),
                                game_id,
                                versions,
                                self.tool_state.tool_option_catalog.clone(),
                            ),
                            |result| Message::ToolSettingWritten {
                                tool_id: "proton".to_string(),
                                result,
                            },
                        );
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
                    install_selected_proton_version(self.db.clone(), game_id),
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
                let display_name = tool.display_name().to_string();
                self.tool_state.active_tool_id = Some(tool_id.clone());
                self.status_message = format!("Updating {display_name} setting...");
                return Task::perform(
                    save_tool_setting_for_game(
                        self.db.clone(),
                        game_id,
                        tool_id.clone(),
                        key,
                        value,
                        self.current_tool_game_context(),
                        self.tool_state.optiscaler_releases.clone(),
                        self.tool_state.tool_option_catalog.clone(),
                    ),
                    move |result| Message::ToolSettingWritten {
                        tool_id: tool_id.clone(),
                        result,
                    },
                );
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
                let display_name = tool.display_name().to_string();
                self.tool_state.active_tool_id = Some(tool_id.clone());
                self.status_message = format!(
                    "{} {}...",
                    if enabled { "Enabling" } else { "Disabling" },
                    display_name
                );
                return Task::perform(
                    toggle_tool_for_game(
                        self.db.clone(),
                        game_id,
                        tool_id.clone(),
                        enabled,
                        self.current_tool_game_context(),
                    ),
                    move |result| Message::ToolSettingWritten {
                        tool_id: tool_id.clone(),
                        result,
                    },
                );
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
                let db = self.db.clone();
                return Task::perform(
                    apply_tool_for_game(db, game_id, game_dir, id.clone(), context),
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
                let db = self.db.clone();
                return Task::perform(
                    deactivate_optiscaler_for_game(db, game_id, game_dir),
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
                let db = self.db.clone();
                return Task::perform(
                    restore_tool_settings_for_game(db, game_id, tool_id.clone(), node_id),
                    move |result| Message::ToolSettingWritten {
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
                let db = self.db.clone();
                return Task::perform(
                    apply_tool_for_game(db, game_id, game_dir, id.clone(), context),
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
                let db = self.db.clone();
                return Task::perform(
                    revert_tool_for_game(db, game_id, game_dir, id.clone()),
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
                return Task::perform(
                    save_executable_for_game(self.db.clone(), row),
                    Message::ExecutableSaved,
                );
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
                let db = self.db.clone();
                return Task::perform(
                    remove_executable_for_game(db, game_id, name.clone()),
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
                let db = self.db.clone();
                return Task::perform(
                    run_saved_executable_for_game(db, game_id, name.clone(), profile),
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
                let context = self.current_tool_game_context();
                self.tool_state.active_tool_id = Some("optiscaler".to_string());
                self.status_message = "Adopting OptiScaler...".to_string();
                return Task::perform(
                    adopt_optiscaler_for_game(self.db.clone(), game_id, game_dir, context),
                    |result| Message::ToolSettingWritten {
                        tool_id: "optiscaler".to_string(),
                        result,
                    },
                );
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
                self.tool_state.active_tool_id = Some("optiscaler".to_string());
                self.status_message = "Resetting OptiScaler config...".to_string();
                return Task::perform(
                    reset_optiscaler_config_for_game(self.db.clone(), game_id),
                    |result| Message::ToolSettingWritten {
                        tool_id: "optiscaler".to_string(),
                        result,
                    },
                );
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
}

fn deploy_profile_blocking(
    db: modde_core::db::ModdeDb,
    profile_name: String,
    game_id: GameId,
) -> Result<String, String> {
    let pm = ProfileManager::with_db(db);
    let profile =
        crate::app::block_on(pm.load(&profile_name, Some(&game_id))).map_err(|e| e.to_string())?;
    let resolved = modde_core::resolver::resolve(&profile).map_err(|e| e.to_string())?;
    let game_plugin = modde_games::resolve_game_plugin(game_id.as_str())
        .ok_or_else(|| format!("unsupported game: {game_id}"))?;
    let install_path = game_plugin
        .detect_install()
        .ok_or_else(|| format!("could not detect install for {game_id}"))?;
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
}
