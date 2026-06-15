//! Tool settings schema and persistence helpers.

use modde_core::resolver::GameId;

use super::ToolOptionCatalog;
use super::state::ToolSettingWriteResult;
#[path = "tool_settings_parts/persistence.rs"]
mod persistence;
#[path = "tool_settings_parts/schema.rs"]
mod schema;

pub(super) use self::persistence::*;
pub(super) use self::schema::*;
