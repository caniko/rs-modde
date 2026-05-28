//! DB-backed vanilla merge-base cache configuration.

use std::path::PathBuf;

use crate::db::ModdeDb;

/// Resolve the configured vanilla cache directory for `game_id` from the
/// current global modde database.
#[must_use]
pub fn resolve_cache_dir(game_id: &str) -> Option<PathBuf> {
    ModdeDb::open()
        .ok()
        .and_then(|db| db.get_vanilla_dir(game_id).ok().flatten())
}
