//! Tool loading, apply/revert, and executable helpers.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use modde_core::profile::ProfileManager;
use modde_core::resolver::GameId;

use super::state::ToolLoadRequest;
use super::tool_settings::{
    apply_derived_tool_settings, build_tool_derived_facts, current_tool_config_async,
    current_tool_config_blocking, format_tool_availability, get_tool_setting_value,
    normalize_tool_settings_for_specs, patch_tool_setting_options,
    save_tool_config_with_reason_async, sync_optiscaler_release_options, tool_apply_is_pending,
    tool_apply_signature,
};
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
use super::tool_settings::{set_tool_options, tool_options};
use super::{
    ChecklistStatus, ConfigChecklistItem, ExecutableDraft, ExecutableUiEntry, ToolApplyResult,
    ToolHistoryUiEntry, ToolLoadSnapshot, ToolOptionCatalog, ToolReleaseSupport, ToolRevertResult,
    ToolUiEntry,
};
#[path = "tool_ops_parts/load.rs"]
mod load;
#[path = "tool_ops_parts/operations.rs"]
mod operations;

pub(super) use self::load::*;
pub use self::operations::parse_executable_environment;
pub(super) use self::operations::*;
