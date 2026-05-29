use std::path::PathBuf;

use iced::widget::{container, opaque};
use iced::{Element, Length, Task};
use modde_core::profile::ProfileManager;
use modde_core::resolver::GameId;

use super::tool_ops::{load_executables_for_game, load_tools_state};
use super::tool_settings::{
    apply_derived_tool_settings, build_tool_derived_facts, current_tool_config,
    format_tool_availability, normalize_tool_settings_for_specs, patch_tool_setting_options,
    set_tool_options, sync_optiscaler_release_options, tool_apply_is_pending, tool_options,
};
use super::{
    Message, Modde, SettingsState, ToolHistoryUiEntry, ToolLoadRequest, ToolLoadSnapshot,
    ToolReleaseSupport, ToolUiEntry, View, WabbajackInstallerState, build_conflict_rows,
    build_default_download_meta, detected_game_ids, format_diagnostic_entry, load_active_plugins,
    load_hidden_files, settings_game_install_paths,
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

    pub(super) fn reload_profile(&mut self) {
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

    pub(super) fn switch_game_context(&mut self, game_id: &str) {
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

    pub(super) fn accept_game_selection(&mut self, game_id: String, previous_game: Option<String>) {
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

    pub(super) fn refresh_data_tab_conflicts(&mut self) {
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

    pub(super) fn run_diagnostics_now(&mut self) {
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
        Task::perform(load_tools_state(request), move |result| {
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
        Task::perform(load_executables_for_game(game_id), move |result| {
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

    pub(super) fn refresh_tools_state(&mut self) {
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
