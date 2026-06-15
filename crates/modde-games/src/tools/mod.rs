//! Per-game tool/overlay management.
//!
//! Each tool (`MangoHud`, vkBasalt, `GameMode`, `ReShade`, `OptiScaler`, Proton) implements the
//! [`GameTool`] trait. Tools are registered via [`all_tools`] and resolved by ID
//! via [`resolve_tool`], following the same pattern as [`crate::resolve_game_plugin`].

#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
pub mod gamemode;
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
pub mod mangohud;
pub mod optiscaler;
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
pub mod proton;
pub mod release;
pub mod reshade;
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
pub mod vkbasalt;

mod config;
mod context;
mod settings;
mod types;

#[cfg(test)]
mod tests;

pub use config::ToolConfig;
pub use context::ToolGameContext;
pub use settings::{ToolSelectOption, ToolSettingKind, ToolSettingSpec};
pub use types::{
    AppliedFiles, GeneratedConfig, ToolApplyPreview, ToolAvailability, ToolCategory,
    ToolReleaseAsset, ToolReleaseInstallFuture, ToolReleaseListFuture, ToolReleaseSummary,
    WrapperEntry,
};

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use smallvec::SmallVec;

// ── Trait ───────────────────────────────────────────────────────────────────

/// A gaming tool/overlay that can be managed per-game.
pub trait GameTool: Send + Sync {
    /// Unique identifier (e.g. `"mangohud"`, `"gamemode"`).
    fn tool_id(&self) -> &'static str;

    /// Human-readable display name.
    fn display_name(&self) -> &'static str;

    /// Tool category.
    fn category(&self) -> ToolCategory;

    /// Short user-facing description for the tool tab.
    fn description(&self) -> &'static str {
        "Game-specific tool integration."
    }

    /// Declarative settings rendered by the UI.
    fn settings_schema(&self) -> Vec<ToolSettingSpec> {
        Vec::new()
    }

    /// Declarative settings rendered by the UI with game context.
    fn settings_schema_for(
        &self,
        _context: Option<&ToolGameContext>,
        _config: &ToolConfig,
    ) -> Vec<ToolSettingSpec> {
        self.settings_schema()
    }

    /// Check if the tool is installed on the system.
    fn detect_available(&self) -> ToolAvailability;

    /// Environment variables to set when launching the game.
    fn env_vars(&self, config: &ToolConfig) -> SmallVec<[(String, String); 4]>;

    /// Environment variables to set when launching the game with game context.
    fn env_vars_for(
        &self,
        _context: Option<&ToolGameContext>,
        config: &ToolConfig,
    ) -> SmallVec<[(String, String); 4]> {
        self.env_vars(config)
    }

    /// Wrapper command to chain before the game (e.g. `gamemoderun`).
    fn wrapper_command(&self, _config: &ToolConfig) -> Option<WrapperEntry> {
        None
    }

    /// Wine DLL overrides needed (e.g. `"dxgi"`, `"version"`).
    fn wine_dll_overrides(&self, _config: &ToolConfig) -> SmallVec<[String; 4]> {
        SmallVec::new()
    }

    /// Wine DLL overrides needed with game context.
    fn wine_dll_overrides_for(
        &self,
        _context: Option<&ToolGameContext>,
        config: &ToolConfig,
    ) -> SmallVec<[String; 4]> {
        self.wine_dll_overrides(config)
    }

    /// Apply/install files into the game directory (DLLs, shaders, etc.).
    /// Returns a manifest of files written for revert tracking.
    fn apply(&self, _game_dir: &Path, _config: &ToolConfig) -> Result<AppliedFiles> {
        Ok(AppliedFiles::default())
    }

    /// Apply/install files with game context.
    fn apply_for(
        &self,
        game_dir: &Path,
        _context: Option<&ToolGameContext>,
        config: &ToolConfig,
    ) -> Result<AppliedFiles> {
        self.apply(game_dir, config)
    }

    /// Preview an apply without mutating the game directory.
    fn preview_apply_for(
        &self,
        _game_dir: &Path,
        _context: Option<&ToolGameContext>,
        _config: &ToolConfig,
    ) -> Result<ToolApplyPreview> {
        Ok(ToolApplyPreview::default())
    }

    /// Revert files previously applied by [`Self::apply`].
    fn revert(&self, game_dir: &Path, applied: &AppliedFiles) -> Result<()> {
        for rel in &applied.files {
            let path = game_dir.join(rel);
            if path.exists() {
                std::fs::remove_file(&path)
                    .with_context(|| format!("failed to remove {}", path.display()))?;
            }
        }
        Ok(())
    }

    /// Generate a per-game config file (e.g. MangoHud.conf).
    fn generate_config(&self, _config: &ToolConfig) -> Option<GeneratedConfig> {
        None
    }

    /// Generate a per-game config file with game context.
    fn generate_config_for(
        &self,
        _context: Option<&ToolGameContext>,
        config: &ToolConfig,
    ) -> Option<GeneratedConfig> {
        self.generate_config(config)
    }

    /// Default configuration for a fresh enable.
    fn default_config(&self) -> ToolConfig;

    /// Default configuration for a fresh enable with game context.
    fn default_config_for(&self, _context: Option<&ToolGameContext>) -> ToolConfig {
        self.default_config()
    }

    /// Whether this tool supports explicit per-game release selection.
    fn supports_releases(&self) -> bool {
        false
    }

    /// List upstream releases for release-backed tools.
    fn list_releases(&self) -> ToolReleaseListFuture<'_> {
        Box::pin(async {
            anyhow::bail!("{} does not support release selection", self.display_name())
        })
    }

    /// Return installable asset names for a release.
    fn installable_release_assets(&self, _release: &ToolReleaseSummary) -> Vec<String> {
        Vec::new()
    }

    /// Install a selected release asset and return the updated per-game config.
    fn install_release<'a>(
        &'a self,
        _game_id: &'a str,
        _config: ToolConfig,
        _tag: &'a str,
        _asset: &'a str,
    ) -> ToolReleaseInstallFuture<'a> {
        Box::pin(async {
            anyhow::bail!(
                "{} does not support release installation",
                self.display_name()
            )
        })
    }

    /// Install a selected release asset from a local path and return the updated per-game config.
    fn install_release_from_path<'a>(
        &'a self,
        _game_id: &'a str,
        _config: ToolConfig,
        _tag: &'a str,
        _asset: &'a str,
        _path: PathBuf,
    ) -> ToolReleaseInstallFuture<'a> {
        Box::pin(async {
            anyhow::bail!("{} does not support release pinning", self.display_name())
        })
    }
}

// ── Registry ───────────────────────────────────────────────────────────────

/// All registered tools for Linux integration builds.
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
static ALL_TOOLS: [&dyn GameTool; 6] = [
    &mangohud::MANGOHUD,
    &vkbasalt::VKBASALT,
    &gamemode::GAMEMODE,
    &reshade::RESHADE,
    &optiscaler::OPTISCALER,
    &proton::PROTON,
];

/// All registered tools for Windows integration builds.
#[cfg(all(target_os = "windows", feature = "windows-integrations"))]
static ALL_TOOLS: [&dyn GameTool; 2] = [&reshade::RESHADE, &optiscaler::OPTISCALER];

/// macOS has no implemented per-game tool integration yet.
#[cfg(all(target_os = "macos", feature = "macos-integrations"))]
static ALL_TOOLS: [&dyn GameTool; 0] = [];

/// Fail closed when a platform integration feature is not enabled.
#[cfg(not(any(
    all(target_os = "linux", feature = "linux-integrations"),
    all(target_os = "windows", feature = "windows-integrations"),
    all(target_os = "macos", feature = "macos-integrations"),
)))]
static ALL_TOOLS: [&dyn GameTool; 0] = [];

#[must_use]
pub fn all_tools() -> &'static [&'static dyn GameTool] {
    &ALL_TOOLS
}

/// Resolve a tool by its ID string.
#[must_use]
pub fn resolve_tool(tool_id: &str) -> Option<&'static dyn GameTool> {
    all_tools().iter().find(|t| t.tool_id() == tool_id).copied()
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Directory where modde stores per-game tool configs.
#[must_use]
pub fn tool_config_dir(game_id: &str) -> PathBuf {
    modde_core::paths::modde_data_dir()
        .join("tools")
        .join(game_id)
}

/// Check if a binary is on `$PATH`.
///
/// Uses the `which` crate for cross-platform support (handles Windows
/// `%PATHEXT%` extensions like `.exe`, `.cmd`, `.bat` automatically).
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
pub(crate) fn which(binary: &str) -> Option<PathBuf> {
    which::which(binary).ok()
}
