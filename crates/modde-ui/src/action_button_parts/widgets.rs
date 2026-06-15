#![allow(clippy::wildcard_imports)]
use super::*;

pub trait DescribedButtonExt<'a> {
    fn on_action(self, action: ButtonAction) -> Element<'a, Message>;
    fn on_action_maybe(
        self,
        action: Option<ButtonAction>,
        disabled_description: &'static str,
    ) -> Element<'a, Message>;
    fn described_disabled(self, description: &'static str) -> Element<'a, Message>;
}

impl<'a> DescribedButtonExt<'a> for Button<'a, Message> {
    fn on_action(self, action: ButtonAction) -> Element<'a, Message> {
        let description = action.button_description();
        described(self.on_press(action.into()), description)
    }

    fn on_action_maybe(
        self,
        action: Option<ButtonAction>,
        disabled_description: &'static str,
    ) -> Element<'a, Message> {
        match action {
            Some(action) => self.on_action(action),
            None => self.described_disabled(disabled_description),
        }
    }

    fn described_disabled(self, description: &'static str) -> Element<'a, Message> {
        described(self, description)
    }
}

fn described<'a>(button: Button<'a, Message>, description: &'static str) -> Element<'a, Message> {
    let id = NEXT_BUTTON_HOVER_ID.fetch_add(1, Ordering::Relaxed);

    mouse_area(button)
        .on_enter(Message::ButtonHoverStarted { id, description })
        .on_exit(Message::ButtonHoverEnded { id })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_actions() -> Vec<(&'static str, ButtonAction)> {
        vec![
            ("Mod List", ButtonAction::SwitchView(View::ModList)),
            ("Game", ButtonAction::ToggleSidebarGroup(SidebarGroup::Game)),
            ("Del", ButtonAction::DeleteProfile("Default".to_string())),
            ("New", ButtonAction::OpenNewProfileDialog),
            (
                "Fork",
                ButtonAction::ForkProfile {
                    source: "Default".to_string(),
                    new_name: "Default-fork".to_string(),
                },
            ),
            ("Rollback", ButtonAction::RollbackExperiment),
            ("Commit", ButtonAction::CommitExperiment),
            ("Try Profile", ButtonAction::TryProfile),
            ("Open in Nexus", ButtonAction::OpenModPage),
            ("Next image", ButtonAction::ModGalleryNext),
            ("Endorse", ButtonAction::ModEndorseToggle),
            ("Track", ButtonAction::ModTrackToggle),
            (
                "Restore",
                ButtonAction::RestoreSaveSnapshot("abc123".to_string()),
            ),
            ("Add Mod", ButtonAction::AddMod),
            ("Remove", ButtonAction::RemoveMod(0)),
            ("Deploy", ButtonAction::Deploy),
            ("AND", ButtonAction::ToggleFilterMode),
            ("Enabled", ButtonAction::CycleFilter(FilterKind::Enabled)),
            ("Clear", ButtonAction::ClearFilters),
            ("Compact", ButtonAction::ToggleCompactModList),
            ("Category", ButtonAction::ToggleSeparator(None)),
            (
                "^",
                ButtonAction::ReorderMod {
                    mod_id: "mod".to_string(),
                    direction: ReorderDirection::Up,
                },
            ),
            ("Mod", ButtonAction::SelectMod(0)),
            (
                "Search",
                ButtonAction::SearchCollections("query".to_string()),
            ),
            (
                "Install",
                ButtonAction::InstallCollection {
                    slug: "collection".to_string(),
                    version: "1.0".to_string(),
                },
            ),
            ("Top", ButtonAction::BrowseTabSwitched(BrowseTab::Top)),
            (
                "Install",
                ButtonAction::BrowseInstallMod {
                    game_domain: "skyrimspecialedition".to_string(),
                    mod_id: 1.into(),
                },
            ),
            ("Refresh", ButtonAction::LoadWabbajackCatalog),
            (
                "Catalog",
                ButtonAction::WabbajackTabChanged(WabbajackTab::Catalog),
            ),
            ("Select File", ButtonAction::OpenWabbajackFile),
            ("Download", ButtonAction::WabbajackDownloadSelected),
            ("Recheck", ButtonAction::WabbajackCheckReadiness),
            ("Import archives", ButtonAction::WabbajackImportArchives),
            ("Install", ButtonAction::WabbajackStartInstall),
            ("Entry", ButtonAction::WabbajackSelectEntry(0)),
            (
                "Open Readme",
                ButtonAction::WabbajackOpenUrl("https://example.test".to_string()),
            ),
            ("Generate", ButtonAction::WabbajackGenerateHmSnippet),
            ("Copy", ButtonAction::WabbajackCopyHmSnippet),
            ("Save", ButtonAction::WabbajackSaveHmSnippet),
            ("Cancel", ButtonAction::FomodCancel),
            ("Undo", ButtonAction::FomodUndo),
            ("Back", ButtonAction::FomodBack),
            ("Next", ButtonAction::FomodNext),
            ("Pause", ButtonAction::PauseDownload(0)),
            ("Resume", ButtonAction::ResumeDownload(0)),
            ("Cancel", ButtonAction::CancelDownload(0)),
            ("Run Diagnostics", ButtonAction::RunDiagnostics),
            ("Clear All", ButtonAction::ClearOverwrite),
            (
                "Create Mod",
                ButtonAction::MoveOverwriteToMod("__from_overrides__".to_string()),
            ),
            ("Refresh", ButtonAction::LoadSaveHistory),
            ("Validate", ButtonAction::ValidateNexusKey),
            ("Show", ButtonAction::ToggleNexusApiKeyVisibility),
            ("Replace", ButtonAction::ReplaceNexusApiKey),
            ("Remove modde config", ButtonAction::RemoveNexusConfigKey),
            ("Browse", ButtonAction::BrowseGamePath),
            ("Browse", ButtonAction::BrowseDownloadDir),
            ("Create Snapshot", ButtonAction::CreateStockSnapshot),
            ("Verify Snapshot", ButtonAction::VerifyStockSnapshot),
            ("Refresh", ButtonAction::RefreshTools),
            ("Tool", ButtonAction::SelectToolTab("mangohud".to_string())),
            (
                "Tool Setting",
                ButtonAction::UpdateToolSetting {
                    tool_id: "tool".to_string(),
                    key: "setting".to_string(),
                    value: serde_json::json!(true),
                },
            ),
            (
                "Advanced Settings",
                ButtonAction::ToggleToolAdvancedSettings,
            ),
            ("Apply", ButtonAction::ApplyTool("tool".to_string())),
            ("Revert", ButtonAction::RevertTool("tool".to_string())),
            ("Adopt OptiScaler", ButtonAction::AdoptOptiScaler),
            ("Restore OptiScaler", ButtonAction::RestoreOptiScalerBackup),
            ("Reset OptiScaler", ButtonAction::ResetOptiScalerConfig),
            ("Releases", ButtonAction::RefreshOptiScalerReleases),
            ("Install OptiScaler", ButtonAction::InstallOptiScalerRelease),
            ("Proton Versions", ButtonAction::RefreshProtonVersions),
            ("Install Proton", ButtonAction::InstallProtonVersion),
            ("Add executable", ButtonAction::OpenExecutableEditor),
            ("Refresh", ButtonAction::RefreshExecutables),
            ("Clear", ButtonAction::ClearExecutableDraft),
            ("Edit", ButtonAction::EditExecutable("xEdit".to_string())),
            ("Save", ButtonAction::SaveExecutable),
            (
                "Remove",
                ButtonAction::RemoveExecutable("xEdit".to_string()),
            ),
            ("Run", ButtonAction::RunExecutable("xEdit".to_string())),
            ("Browse", ButtonAction::BrowseExecutablePath),
            ("Browse", ButtonAction::BrowseExecutableWorkingDir),
            ("-", ButtonAction::WindowMinimize),
            ("Maximize", ButtonAction::WindowToggleMaximize),
            ("Close", ButtonAction::WindowClose),
            ("Cancel", ButtonAction::CancelNewProfileDialog),
            ("Create", ButtonAction::SubmitNewProfileDialog),
            ("Browse", ButtonAction::GamePathDialogBrowse),
            ("Cancel", ButtonAction::CancelGamePathDialog),
        ]
    }

    #[test]
    fn action_descriptions_are_present_and_more_specific_than_labels() {
        for (label, action) in sample_actions() {
            let description = action.button_description();
            assert!(
                !description.trim().is_empty(),
                "missing description for {action:?}"
            );
            assert!(
                description.len() > label.len(),
                "description for {action:?} is not more detailed than {label:?}"
            );
        }
    }

    #[test]
    fn optiscaler_state_actions_have_descriptions() {
        for action in [
            ButtonAction::AdoptOptiScaler,
            ButtonAction::RestoreOptiScalerBackup,
            ButtonAction::ResetOptiScalerConfig,
        ] {
            assert!(
                !action.button_description().trim().is_empty(),
                "missing OptiScaler description for {action:?}"
            );
        }
    }
}
