use std::sync::atomic::{AtomicU64, Ordering};

use crate::app::{Message, ReorderDirection, SidebarGroup, View, WabbajackTab};
use crate::views::browse_nexus::BrowseTab;
use crate::views::mod_details::ModDetailsTab;
use iced::Element;
use iced::widget::{Button, mouse_area};
use modde_core::filter::FilterKind;

static NEXT_BUTTON_HOVER_ID: AtomicU64 = AtomicU64::new(1);

/// Trait-enforced hover copy for GUI button actions.
pub trait ButtonActionDescription {
    fn button_description(&self) -> &'static str;
}

#[derive(Debug, Clone)]
pub enum ButtonAction {
    SwitchView(View),
    ToggleSidebarGroup(SidebarGroup),
    DeleteProfile(String),
    OpenNewProfileDialog,
    ForkProfile {
        source: String,
        new_name: String,
    },
    RollbackExperiment,
    CommitExperiment,
    TryProfile,
    OpenModPage,
    ModGalleryNext,
    ModDetailsTabChanged(ModDetailsTab),
    RunModConflictMerge {
        merge_group: String,
        driver_id: Option<String>,
    },
    AcceptModConflictWinner {
        merge_group: String,
        winner_mod_id: String,
    },
    ToggleModConflictHidden {
        mod_id: String,
        rel_path: String,
        hide: bool,
    },
    LoadMerges,
    ResolveMerge {
        merge_group: String,
    },
    OpenMergeDossier {
        merge_group: String,
    },
    ModEndorseToggle,
    ModTrackToggle,
    RestoreSaveSnapshot(String),
    AddMod,
    RemoveMod(usize),
    Deploy,
    ToggleFilterMode,
    CycleFilter(FilterKind),
    ClearFilters,
    ToggleCompactModList,
    ToggleSeparator(Option<i64>),
    ReorderMod {
        mod_id: String,
        direction: ReorderDirection,
    },
    SelectMod(usize),
    SearchCollections(String),
    InstallCollection {
        slug: String,
        version: String,
    },
    BrowseTabSwitched(BrowseTab),
    BrowseInstallMod {
        game_domain: String,
        mod_id: u64,
    },
    LoadWabbajackCatalog,
    WabbajackTabChanged(WabbajackTab),
    OpenWabbajackFile,
    WabbajackDownloadSelected,
    WabbajackStartInstall,
    WabbajackSelectEntry(usize),
    WabbajackOpenUrl(String),
    WabbajackGenerateHmSnippet,
    WabbajackCopyHmSnippet,
    WabbajackSaveHmSnippet,
    FomodCancel,
    FomodUndo,
    FomodBack,
    FomodNext,
    PauseDownload(usize),
    ResumeDownload(usize),
    CancelDownload(usize),
    RunDiagnostics,
    ClearOverwrite,
    MoveOverwriteToMod(String),
    LoadSaveHistory,
    ValidateNexusKey,
    ToggleNexusApiKeyVisibility,
    ReplaceNexusApiKey,
    RemoveNexusConfigKey,
    BrowseGamePath,
    BrowseDownloadDir,
    CreateStockSnapshot,
    VerifyStockSnapshot,
    RefreshTools,
    SelectToolTab(String),
    UpdateToolSetting {
        tool_id: String,
        key: String,
        value: serde_json::Value,
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
    RefreshOptiScalerReleases,
    InstallOptiScalerRelease,
    RefreshProtonVersions,
    InstallProtonVersion,
    OpenExecutableEditor,
    RefreshExecutables,
    ClearExecutableDraft,
    EditExecutable(String),
    SaveExecutable,
    RemoveExecutable(String),
    RunExecutable(String),
    BrowseExecutablePath,
    BrowseExecutableWorkingDir,
    WindowMinimize,
    WindowToggleMaximize,
    WindowClose,
    CancelNewProfileDialog,
    SubmitNewProfileDialog,
    GamePathDialogBrowse,
    CancelGamePathDialog,
    OpenAddCustomGame,
    BrowseAddCustomGameInstallPath,
    AddCustomGameSubmit,
    AddCustomGameCancel,
    OpenManageCustomGames,
    CloseManageCustomGames,
    RemoveCustomGame(String),
    OpenUpdateReleasePage,
    DismissUpdateBanner,
}

impl ButtonActionDescription for ButtonAction {
    fn button_description(&self) -> &'static str {
        match self {
            ButtonAction::SwitchView(_) => "Switch the main workspace to this section.",
            ButtonAction::ToggleSidebarGroup(_) => {
                "Expand or collapse this sidebar navigation group."
            }
            ButtonAction::DeleteProfile(_) => {
                "Delete the active profile and remove it from the profile list."
            }
            ButtonAction::OpenNewProfileDialog => {
                "Open a dialog for creating a new profile for the selected game."
            }
            ButtonAction::ForkProfile { .. } => {
                "Create a copy of the active profile that can be changed independently."
            }
            ButtonAction::RollbackExperiment => {
                "Discard the current profile experiment and return to the previous state."
            }
            ButtonAction::CommitExperiment => {
                "Keep the current experiment changes and make them the active profile state."
            }
            ButtonAction::TryProfile => {
                "Start an experimental profile layer so changes can be tested before committing."
            }
            ButtonAction::OpenModPage => "Open the selected mod's Nexus Mods page in a browser.",
            ButtonAction::ModGalleryNext => {
                "Show the next image from the selected mod's Nexus gallery."
            }
            ButtonAction::ModDetailsTabChanged(_) => {
                "Switch the selected mod details panel to this tab."
            }
            ButtonAction::RunModConflictMerge { driver_id, .. } => {
                if driver_id.as_deref() == Some("claude-code") {
                    "Resolve this collision with the Claude Code merge driver."
                } else {
                    "Resolve this collision with the default available merge driver."
                }
            }
            ButtonAction::AcceptModConflictWinner { .. } => {
                "Resolve this collision by accepting the current winning file."
            }
            ButtonAction::ToggleModConflictHidden { hide, .. } => {
                if *hide {
                    "Hide the losing file from deployment for this mod."
                } else {
                    "Unhide the losing file so it participates in deployment again."
                }
            }
            ButtonAction::LoadMerges => "Refresh merge sessions for the active profile.",
            ButtonAction::ResolveMerge { .. } => {
                "Open this merge session with the selected merge driver."
            }
            ButtonAction::OpenMergeDossier { .. } => {
                "Open the merge session dossier for manual inspection."
            }
            ButtonAction::ModEndorseToggle => "Toggle your Nexus endorsement for the selected mod.",
            ButtonAction::ModTrackToggle => {
                "Toggle whether Nexus tracks updates for the selected mod."
            }
            ButtonAction::RestoreSaveSnapshot(_) => {
                "Restore this save snapshot and its captured save files."
            }
            ButtonAction::AddMod => "Choose a mod archive or folder to add to the active profile.",
            ButtonAction::RemoveMod(_) => "Remove the selected mod from the active profile.",
            ButtonAction::Deploy => "Deploy the active profile's enabled mods to the game folder.",
            ButtonAction::ToggleFilterMode => {
                "Switch whether mod list filters must all match or any one can match."
            }
            ButtonAction::CycleFilter(_) => {
                "Cycle this filter between ignored, required, and excluded."
            }
            ButtonAction::ClearFilters => "Clear all active mod list filters.",
            ButtonAction::ToggleCompactModList => {
                "Toggle between compact and normal spacing in the mod list."
            }
            ButtonAction::ToggleSeparator(_) => {
                "Expand or collapse this category group in the mod list."
            }
            ButtonAction::ReorderMod { .. } => {
                "Move this mod one position in the active profile load order."
            }
            ButtonAction::SelectMod(_) => "Select this mod and show its details in the sidebar.",
            ButtonAction::SearchCollections(_) => {
                "Run a Nexus Collections search using the current search text."
            }
            ButtonAction::InstallCollection { .. } => {
                "Start downloading and installing this Nexus collection."
            }
            ButtonAction::BrowseTabSwitched(_) => {
                "Switch the Nexus browser to this feed and load its results if needed."
            }
            ButtonAction::BrowseInstallMod { .. } => {
                "Install this Nexus mod into the active profile."
            }
            ButtonAction::LoadWabbajackCatalog => {
                "Refresh the Wabbajack catalog and authored file lists."
            }
            ButtonAction::WabbajackTabChanged(_) => "Switch the Wabbajack explorer to this tab.",
            ButtonAction::OpenWabbajackFile => "Choose a local .wabbajack file from disk.",
            ButtonAction::WabbajackDownloadSelected => {
                "Download the selected or entered Wabbajack modlist file."
            }
            ButtonAction::WabbajackStartInstall => {
                "Install the currently selected local Wabbajack file."
            }
            ButtonAction::WabbajackSelectEntry(_) => {
                "Select this Wabbajack entry and show its details."
            }
            ButtonAction::WabbajackOpenUrl(_) => "Open this Wabbajack page in your browser.",
            ButtonAction::WabbajackGenerateHmSnippet => {
                "Generate a Home Manager configuration snippet for this Wabbajack setup."
            }
            ButtonAction::WabbajackCopyHmSnippet => {
                "Copy the generated Home Manager snippet to the clipboard."
            }
            ButtonAction::WabbajackSaveHmSnippet => {
                "Save the generated Home Manager snippet to disk."
            }
            ButtonAction::FomodCancel => "Cancel the FOMOD installer and close the wizard.",
            ButtonAction::FomodUndo => "Undo the most recent FOMOD wizard selection change.",
            ButtonAction::FomodBack => "Return to the previous FOMOD installer step.",
            ButtonAction::FomodNext => {
                "Continue to the next FOMOD step or install when all required choices are ready."
            }
            ButtonAction::PauseDownload(_) => "Pause this active download.",
            ButtonAction::ResumeDownload(_) => "Resume or retry this download.",
            ButtonAction::CancelDownload(_) => "Cancel this queued or active download.",
            ButtonAction::RunDiagnostics => {
                "Scan the active profile for game-specific modding and integrity issues."
            }
            ButtonAction::ClearOverwrite => {
                "Delete all files currently stored in the profile override area."
            }
            ButtonAction::MoveOverwriteToMod(_) => {
                "Create a regular mod from the files currently in the override area."
            }
            ButtonAction::LoadSaveHistory => {
                "Refresh the list of captured save snapshots for the active profile."
            }
            ButtonAction::ValidateNexusKey => {
                "Validate the configured Nexus Mods API key and show account status."
            }
            ButtonAction::ToggleNexusApiKeyVisibility => {
                "Show or hide the Nexus Mods API key in the settings field."
            }
            ButtonAction::ReplaceNexusApiKey => {
                "Save this key to modde's own Nexus API key config file."
            }
            ButtonAction::RemoveNexusConfigKey => {
                "Remove only modde's own Nexus API key config file."
            }
            ButtonAction::BrowseGamePath => "Choose the game installation directory.",
            ButtonAction::BrowseDownloadDir => "Choose where downloaded mod archives are stored.",
            ButtonAction::CreateStockSnapshot => {
                "Capture a clean stock game snapshot for later deployment checks."
            }
            ButtonAction::VerifyStockSnapshot => {
                "Compare the current game installation with the saved stock snapshot."
            }
            ButtonAction::RefreshTools => {
                "Refresh detected gaming tools and overlay integration status."
            }
            ButtonAction::SelectToolTab(_) => "Switch to this tool's game-specific settings tab.",
            ButtonAction::UpdateToolSetting { .. } => {
                "Apply this value to the selected tool setting."
            }
            ButtonAction::ToggleToolAdvancedSettings => {
                "Show or hide advanced tool settings for the active tool."
            }
            ButtonAction::ApplyTool(_) => {
                "Apply this tool's required files or configuration to the game directory."
            }
            ButtonAction::RevertTool(_) => {
                "Remove this tool's applied files from the game directory."
            }
            ButtonAction::ActivateOptiScaler => {
                "Apply OptiScaler files and enable its launch integration."
            }
            ButtonAction::DeactivateOptiScaler => {
                "Revert OptiScaler files and disable its launch integration."
            }
            ButtonAction::AdoptOptiScaler => {
                "Record the detected OptiScaler files as managed for this game."
            }
            ButtonAction::RestoreOptiScalerBackup => {
                "Restore the latest backed-up OptiScaler files for this game."
            }
            ButtonAction::ResetOptiScalerConfig => {
                "Clear OptiScaler INI overrides so the selected release defaults are used."
            }
            ButtonAction::RestoreToolSettings { .. } => {
                "Restore this settings version without applying or reverting game files."
            }
            ButtonAction::RefreshOptiScalerReleases => {
                "Load OptiScaler release tags and assets from the official GitHub repository."
            }
            ButtonAction::InstallOptiScalerRelease => {
                "Download and cache the selected OptiScaler release for this game."
            }
            ButtonAction::RefreshProtonVersions => {
                "Load GE-Proton release versions from the official GitHub repository."
            }
            ButtonAction::InstallProtonVersion => {
                "Install the selected GEProton version through protonup-rs."
            }
            ButtonAction::OpenExecutableEditor => {
                "Open the editor for adding a new executable launch target."
            }
            ButtonAction::RefreshExecutables => {
                "Refresh executable launch targets for the selected game."
            }
            ButtonAction::ClearExecutableDraft => {
                "Close the executable editor and discard unsaved field values."
            }
            ButtonAction::EditExecutable(_) => {
                "Load this executable into the editor so its settings can be updated."
            }
            ButtonAction::SaveExecutable => {
                "Save the executable launch target for the selected game."
            }
            ButtonAction::RemoveExecutable(_) => {
                "Remove this executable launch target from the selected game."
            }
            ButtonAction::RunExecutable(_) => {
                "Run this executable through the active profile with overwrite capture."
            }
            ButtonAction::BrowseExecutablePath => "Choose the executable file to launch.",
            ButtonAction::BrowseExecutableWorkingDir => {
                "Choose the working directory for this executable."
            }
            ButtonAction::WindowMinimize => "Minimize the modde window.",
            ButtonAction::WindowToggleMaximize => {
                "Toggle the modde window between maximized and restored size."
            }
            ButtonAction::WindowClose => "Close the modde window.",
            ButtonAction::CancelNewProfileDialog => {
                "Close the new profile dialog without creating a profile."
            }
            ButtonAction::SubmitNewProfileDialog => {
                "Create the profile using the entered name and selected game."
            }
            ButtonAction::GamePathDialogBrowse => {
                "Choose the selected game's installation directory."
            }
            ButtonAction::CancelGamePathDialog => {
                "Cancel setting the game path and return to the previous game selection."
            }
            ButtonAction::OpenAddCustomGame => "Open a dialog to register a new custom game.",
            ButtonAction::BrowseAddCustomGameInstallPath => {
                "Choose the custom game's install directory and scan it for executables."
            }
            ButtonAction::AddCustomGameSubmit => {
                "Save the custom game, reload the registry, and select it."
            }
            ButtonAction::AddCustomGameCancel => "Close the custom game dialog without saving.",
            ButtonAction::OpenManageCustomGames => {
                "Open the list of user-defined games and remove existing entries."
            }
            ButtonAction::CloseManageCustomGames => "Close the custom game manager.",
            ButtonAction::RemoveCustomGame(_) => {
                "Remove this user-defined game from the runtime registry."
            }
            ButtonAction::OpenUpdateReleasePage => {
                "Open the latest modde release page in your browser."
            }
            ButtonAction::DismissUpdateBanner => {
                "Hide this update notification for the current session."
            }
        }
    }
}

impl From<ButtonAction> for Message {
    fn from(action: ButtonAction) -> Self {
        match action {
            ButtonAction::SwitchView(view) => Message::SwitchView(view),
            ButtonAction::ToggleSidebarGroup(group) => Message::ToggleSidebarGroup(group),
            ButtonAction::DeleteProfile(name) => Message::DeleteProfile(name),
            ButtonAction::OpenNewProfileDialog => Message::OpenNewProfileDialog,
            ButtonAction::ForkProfile { source, new_name } => {
                Message::ForkProfile { source, new_name }
            }
            ButtonAction::RollbackExperiment => Message::RollbackExperiment,
            ButtonAction::CommitExperiment => Message::CommitExperiment,
            ButtonAction::TryProfile => Message::TryProfile,
            ButtonAction::OpenModPage => Message::OpenModPage,
            ButtonAction::ModGalleryNext => Message::ModGalleryNext,
            ButtonAction::ModDetailsTabChanged(tab) => Message::ModDetailsTabChanged(tab),
            ButtonAction::RunModConflictMerge {
                merge_group,
                driver_id,
            } => Message::RunModConflictMerge {
                merge_group,
                driver_id,
            },
            ButtonAction::AcceptModConflictWinner {
                merge_group,
                winner_mod_id,
            } => Message::AcceptModConflictWinner {
                merge_group,
                winner_mod_id,
            },
            ButtonAction::ToggleModConflictHidden {
                mod_id,
                rel_path,
                hide,
            } => Message::ToggleModConflictHidden {
                mod_id,
                rel_path,
                hide,
            },
            ButtonAction::LoadMerges => Message::LoadMerges,
            ButtonAction::ResolveMerge { merge_group } => Message::ResolveMerge { merge_group },
            ButtonAction::OpenMergeDossier { merge_group } => {
                Message::OpenMergeDossier { merge_group }
            }
            ButtonAction::ModEndorseToggle => Message::ModEndorseToggle,
            ButtonAction::ModTrackToggle => Message::ModTrackToggle,
            ButtonAction::RestoreSaveSnapshot(id) => Message::RestoreSaveSnapshot(id),
            ButtonAction::AddMod => Message::AddMod,
            ButtonAction::RemoveMod(index) => Message::RemoveMod(index),
            ButtonAction::Deploy => Message::Deploy,
            ButtonAction::ToggleFilterMode => Message::ToggleFilterMode,
            ButtonAction::CycleFilter(kind) => Message::CycleFilter(kind),
            ButtonAction::ClearFilters => Message::ClearFilters,
            ButtonAction::ToggleCompactModList => Message::ToggleCompactModList,
            ButtonAction::ToggleSeparator(cat_id) => Message::ToggleSeparator(cat_id),
            ButtonAction::ReorderMod { mod_id, direction } => {
                Message::ReorderMod { mod_id, direction }
            }
            ButtonAction::SelectMod(index) => Message::SelectMod(index),
            ButtonAction::SearchCollections(query) => Message::SearchCollections(query),
            ButtonAction::InstallCollection { slug, version } => {
                Message::InstallCollection { slug, version }
            }
            ButtonAction::BrowseTabSwitched(tab) => Message::BrowseTabSwitched(tab),
            ButtonAction::BrowseInstallMod {
                game_domain,
                mod_id,
            } => Message::BrowseInstallMod {
                game_domain,
                mod_id,
            },
            ButtonAction::LoadWabbajackCatalog => Message::LoadWabbajackCatalog,
            ButtonAction::WabbajackTabChanged(tab) => Message::WabbajackTabChanged(tab),
            ButtonAction::OpenWabbajackFile => Message::OpenWabbajackFile,
            ButtonAction::WabbajackDownloadSelected => Message::WabbajackDownloadSelected,
            ButtonAction::WabbajackStartInstall => Message::WabbajackStartInstall,
            ButtonAction::WabbajackSelectEntry(index) => Message::WabbajackSelectEntry(index),
            ButtonAction::WabbajackOpenUrl(url) => Message::WabbajackOpenUrl(url),
            ButtonAction::WabbajackGenerateHmSnippet => Message::WabbajackGenerateHmSnippet,
            ButtonAction::WabbajackCopyHmSnippet => Message::WabbajackCopyHmSnippet,
            ButtonAction::WabbajackSaveHmSnippet => Message::WabbajackSaveHmSnippet,
            ButtonAction::FomodCancel => Message::FOMODCancel,
            ButtonAction::FomodUndo => Message::FOMODUndo,
            ButtonAction::FomodBack => Message::FOMODBack,
            ButtonAction::FomodNext => Message::FOMODNext,
            ButtonAction::PauseDownload(id) => Message::PauseDownload(id),
            ButtonAction::ResumeDownload(id) => Message::ResumeDownload(id),
            ButtonAction::CancelDownload(id) => Message::CancelDownload(id),
            ButtonAction::RunDiagnostics => Message::RunDiagnostics,
            ButtonAction::ClearOverwrite => Message::ClearOverwrite,
            ButtonAction::MoveOverwriteToMod(mod_id) => Message::MoveOverwriteToMod(mod_id),
            ButtonAction::LoadSaveHistory => Message::LoadSaveHistory,
            ButtonAction::ValidateNexusKey => Message::ValidateNexusKey,
            ButtonAction::ToggleNexusApiKeyVisibility => Message::ToggleNexusApiKeyVisibility,
            ButtonAction::ReplaceNexusApiKey => Message::ReplaceNexusApiKey,
            ButtonAction::RemoveNexusConfigKey => Message::RemoveNexusConfigKey,
            ButtonAction::BrowseGamePath => Message::BrowseGamePath,
            ButtonAction::BrowseDownloadDir => Message::BrowseDownloadDir,
            ButtonAction::CreateStockSnapshot => Message::CreateStockSnapshot,
            ButtonAction::VerifyStockSnapshot => Message::VerifyStockSnapshot,
            ButtonAction::RefreshTools => Message::RefreshTools,
            ButtonAction::SelectToolTab(tool_id) => Message::SelectToolTab(tool_id),
            ButtonAction::UpdateToolSetting {
                tool_id,
                key,
                value,
            } => Message::UpdateToolSetting {
                tool_id,
                key,
                value,
            },
            ButtonAction::ToggleToolAdvancedSettings => Message::ToggleToolAdvancedSettings,
            ButtonAction::ApplyTool(tool_id) => Message::ApplyTool(tool_id),
            ButtonAction::RevertTool(tool_id) => Message::RevertTool(tool_id),
            ButtonAction::ActivateOptiScaler => Message::ActivateOptiScaler,
            ButtonAction::DeactivateOptiScaler => Message::DeactivateOptiScaler,
            ButtonAction::AdoptOptiScaler => Message::AdoptOptiScaler,
            ButtonAction::RestoreOptiScalerBackup => Message::RestoreOptiScalerBackup,
            ButtonAction::ResetOptiScalerConfig => Message::ResetOptiScalerConfig,
            ButtonAction::RestoreToolSettings { tool_id, node_id } => {
                Message::RestoreToolSettings { tool_id, node_id }
            }
            ButtonAction::RefreshOptiScalerReleases => Message::RefreshOptiScalerReleases,
            ButtonAction::InstallOptiScalerRelease => Message::InstallOptiScalerRelease,
            ButtonAction::RefreshProtonVersions => Message::RefreshProtonVersions,
            ButtonAction::InstallProtonVersion => Message::InstallProtonVersion,
            ButtonAction::OpenExecutableEditor => Message::OpenExecutableEditor,
            ButtonAction::RefreshExecutables => Message::RefreshExecutables,
            ButtonAction::ClearExecutableDraft => Message::ClearExecutableDraft,
            ButtonAction::EditExecutable(name) => Message::EditExecutable(name),
            ButtonAction::SaveExecutable => Message::SaveExecutable,
            ButtonAction::RemoveExecutable(name) => Message::RemoveExecutable(name),
            ButtonAction::RunExecutable(name) => Message::RunExecutable(name),
            ButtonAction::BrowseExecutablePath => Message::BrowseExecutablePath,
            ButtonAction::BrowseExecutableWorkingDir => Message::BrowseExecutableWorkingDir,
            ButtonAction::WindowMinimize => Message::WindowMinimize,
            ButtonAction::WindowToggleMaximize => Message::WindowToggleMaximize,
            ButtonAction::WindowClose => Message::WindowClose,
            ButtonAction::CancelNewProfileDialog => Message::CancelNewProfileDialog,
            ButtonAction::SubmitNewProfileDialog => Message::SubmitNewProfileDialog,
            ButtonAction::GamePathDialogBrowse => Message::GamePathDialogBrowse,
            ButtonAction::CancelGamePathDialog => Message::CancelGamePathDialog,
            ButtonAction::OpenAddCustomGame => Message::OpenAddCustomGame,
            ButtonAction::BrowseAddCustomGameInstallPath => Message::BrowseAddCustomGameInstallPath,
            ButtonAction::AddCustomGameSubmit => Message::AddCustomGameSubmit,
            ButtonAction::AddCustomGameCancel => Message::AddCustomGameCancel,
            ButtonAction::OpenManageCustomGames => Message::OpenManageCustomGames,
            ButtonAction::CloseManageCustomGames => Message::CloseManageCustomGames,
            ButtonAction::RemoveCustomGame(id) => Message::RemoveCustomGame(id),
            ButtonAction::OpenUpdateReleasePage => Message::OpenUpdateReleasePage,
            ButtonAction::DismissUpdateBanner => Message::DismissUpdateBanner,
        }
    }
}

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
                    mod_id: 1,
                },
            ),
            ("Refresh", ButtonAction::LoadWabbajackCatalog),
            (
                "Catalog",
                ButtonAction::WabbajackTabChanged(WabbajackTab::Catalog),
            ),
            ("Select File", ButtonAction::OpenWabbajackFile),
            ("Download", ButtonAction::WabbajackDownloadSelected),
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
