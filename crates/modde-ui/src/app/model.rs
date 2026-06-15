use std::path::PathBuf;

use iced::widget::{container, opaque};
use iced::{Element, Length, Task};
use modde_core::installer::StagedFile;
use modde_core::profile::ProfileManager;
use modde_core::resolver::GameId;

use super::state::{
    DataTabConflicts, ProfileContextRequest, ProfileContextSnapshot, ProfileLoadOutcome,
    ToolLoadRequest,
};
use super::tool_ops::{load_executables_for_game, load_tools_state};
use super::{
    DiagnosticsComputed, Message, Modde, SettingsState, ToolLoadSnapshot, View,
    WabbajackInstallerState, build_conflict_rows, build_default_download_meta, detected_game_ids,
    format_diagnostic_entry, load_active_plugins_blocking, load_hidden_files_blocking,
    settings_game_install_paths,
};

#[path = "model_parts/context.rs"]
mod context;
#[path = "model_parts/core.rs"]
mod core;
#[path = "model_parts/diagnostics.rs"]
mod diagnostics;
#[path = "model_parts/tasks.rs"]
mod tasks;

pub(super) use context::load_profile_context;
pub(super) use diagnostics::{
    load_data_tab_conflicts, load_diagnostics, load_diagnostics_with_crash,
};
