use std::path::{Path, PathBuf};

use anyhow::Result;

/// Trait implemented by each supported game.
pub trait GamePlugin: Send + Sync {
    /// Unique game identifier (e.g. "skyrim-se").
    fn game_id(&self) -> &str;

    /// Human-readable display name.
    fn display_name(&self) -> &str;

    /// Attempt to detect the game's install location.
    fn detect_install(&self) -> Option<PathBuf>;

    /// Return the mod directory relative to the install path.
    fn mod_directory(&self, install: &Path) -> PathBuf;

    /// Deploy staged mods into the game's mod directory.
    fn deploy(&self, staging: &Path, target: &Path) -> Result<()>;

    /// Run any post-deployment steps (e.g. REDmod deploy).
    fn post_deploy(&self, install: &Path) -> Result<()>;
}
