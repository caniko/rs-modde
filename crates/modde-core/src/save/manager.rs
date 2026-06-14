//! Git-backed save vault manager.

mod helpers;
mod history;
mod tracking;
mod transfer;
mod vault;

use crate::db::ModdeDb;

#[cfg(test)]
pub(super) use helpers::{
    MODDE_LIVE_STATE_DIR, MODDE_PROFILE_PARK_DIR, STEAM_CLOUD_MARKER, clear_active_save_dir,
    park_active_saves,
};

/// Manages save files using a git-backed vault per game.
///
/// Each game gets a git repository at `<modde_data>/saves/<game_id>/`.
/// Each profile is a branch in that repository. This gives us branching,
/// history, and stacking for free.
///
/// The caller is responsible for resolving the game's save directory
/// (e.g. via `GamePlugin::save_directory()`) and passing it in.
pub struct SaveManager<'a> {
    pub(super) db: &'a ModdeDb,
}

impl<'a> SaveManager<'a> {
    pub fn new(db: &'a ModdeDb) -> Self {
        Self { db }
    }
}
