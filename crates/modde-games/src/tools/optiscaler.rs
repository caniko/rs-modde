//! OptiScaler — DLSS/FSR/XeSS upscaling and frame generation replacement.
//!
//! OptiScaler hooks into a game via proxy DLLs (typically `dxgi.dll` or
//! `winmm.dll`). Some games need additional DLLs like `nvngx.dll`.
//!
//! This implementation also subsumes the old fgmod DLL restoration logic:
//! when fgmod deletes certain DLLs at launch time, the launch wrapper
//! restores them.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use smallvec::{SmallVec, smallvec};
use tracing::info;

use super::{AppliedFiles, GameTool, ToolAvailability, ToolCategory, ToolConfig};

pub static OPTISCALER: OptiScaler = OptiScaler;

pub struct OptiScaler;

/// DLLs that fgmod deletes at launch time. If any of these are deployed by
/// mods or by OptiScaler itself, the launch wrapper must restore them.
pub const FGMOD_DELETED_DLLS: &[&str] = &[
    "dxgi.dll",
    "winmm.dll",
    "nvngx.dll",
    "_nvngx.dll",
    "nvngx-wrapper.dll",
    "dlss-enabler.dll",
    "OptiScaler.dll",
];

impl GameTool for OptiScaler {
    fn tool_id(&self) -> &'static str {
        "optiscaler"
    }

    fn display_name(&self) -> &'static str {
        "OptiScaler"
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Upscaler
    }

    fn detect_available(&self) -> ToolAvailability {
        // Check common locations for OptiScaler
        let candidates = [
            dirs::data_dir()
                .unwrap_or_default()
                .join("goverlay/fgmod/OptiScaler.dll"),
            dirs::home_dir()
                .unwrap_or_default()
                .join(".local/share/goverlay/fgmod/OptiScaler.dll"),
        ];

        for path in &candidates {
            if path.exists() {
                return ToolAvailability::Available {
                    version: Some("fgmod".into()),
                };
            }
        }

        // User can provide a custom path via settings
        ToolAvailability::Available {
            version: Some("user-provided".into()),
        }
    }

    fn env_vars(&self, _config: &ToolConfig) -> SmallVec<[(String, String); 4]> {
        SmallVec::new()
    }

    fn wine_dll_overrides(&self, config: &ToolConfig) -> SmallVec<[String; 4]> {
        let primary = config.get_str("dll_name").unwrap_or("dxgi");
        let mut overrides: SmallVec<[String; 4]> = smallvec![primary.to_string()];

        // Some games also need winmm override
        if config.get_bool("needs_winmm") && primary != "winmm" {
            overrides.push("winmm".into());
        }

        overrides
    }

    fn apply(&self, game_dir: &Path, config: &ToolConfig) -> Result<AppliedFiles> {
        let source_dir = config
            .get_str("source_dir")
            .map(PathBuf::from)
            .or_else(|| {
                // Try fgmod default location
                let fgmod = dirs::home_dir()?.join(".local/share/goverlay/fgmod");
                fgmod.is_dir().then_some(fgmod)
            })
            .context("optiscaler: 'source_dir' setting is required, or install fgmod/goverlay")?;

        let dll_name = config.get_str("dll_name").unwrap_or("dxgi.dll");
        let exe_subdir = config.get_str("exe_subdir").unwrap_or("");

        let target_dir = if exe_subdir.is_empty() {
            game_dir.to_path_buf()
        } else {
            game_dir.join(exe_subdir)
        };

        std::fs::create_dir_all(&target_dir)?;

        let mut applied = AppliedFiles::default();

        // Copy OptiScaler as the requested DLL name
        let optiscaler_dll = source_dir.join("OptiScaler.dll");
        if optiscaler_dll.exists() {
            let dest = target_dir.join(dll_name);
            std::fs::copy(&optiscaler_dll, &dest)
                .with_context(|| format!("failed to copy OptiScaler to {}", dest.display()))?;
            let rel = dest.strip_prefix(game_dir).unwrap_or(&dest).to_path_buf();
            applied.files.push(rel);
            info!(as_dll = %dll_name, "applied OptiScaler DLL");
        }

        // Copy OptiScaler.ini if present (or generate default)
        let ini_src = source_dir.join("OptiScaler.ini");
        let ini_dest = target_dir.join("OptiScaler.ini");
        if ini_src.exists() && !ini_dest.exists() {
            std::fs::copy(&ini_src, &ini_dest)?;
            let rel = ini_dest
                .strip_prefix(game_dir)
                .unwrap_or(&ini_dest)
                .to_path_buf();
            applied.files.push(rel);
        }

        // Copy additional DLLs from source (fakenvapi, nvngx-wrapper, etc.)
        let extra_dlls = ["fakenvapi.dll", "nvngx-wrapper.dll"];
        for extra in &extra_dlls {
            let src = source_dir.join(extra);
            if src.exists() {
                let dest = target_dir.join(extra);
                std::fs::copy(&src, &dest)?;
                let rel = dest.strip_prefix(game_dir).unwrap_or(&dest).to_path_buf();
                applied.files.push(rel);
            }
        }

        Ok(applied)
    }

    fn default_config(&self) -> ToolConfig {
        let mut config = ToolConfig::new("optiscaler");
        config.set("dll_name", serde_json::json!("dxgi.dll"));
        config.set("exe_subdir", serde_json::json!(""));
        config.set("needs_winmm", serde_json::json!(false));
        config
    }
}

/// Build fgmod DLL restore commands for the launch wrapper.
///
/// Scans the staging mods directory for DLLs that fgmod will delete at launch,
/// and returns `(source, destination)` pairs for the wrapper to restore them.
pub fn fgmod_restore_commands(game_dir: &Path, staging_dir: &Path) -> Vec<(String, String)> {
    let exe_dir = game_dir.join("bin/x64");
    let mut restore = Vec::new();

    let mods_dir = staging_dir.join("mods");
    if !mods_dir.exists() {
        return restore;
    }

    for entry in std::fs::read_dir(&mods_dir).into_iter().flatten().flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }

        let mod_bin_x64 = entry.path().join("bin/x64");
        if !mod_bin_x64.exists() {
            continue;
        }

        for dll_entry in std::fs::read_dir(&mod_bin_x64)
            .into_iter()
            .flatten()
            .flatten()
        {
            let dll_name = dll_entry.file_name().to_string_lossy().to_lowercase();
            if FGMOD_DELETED_DLLS
                .iter()
                .any(|d| d.to_lowercase() == dll_name)
            {
                let src = dll_entry.path();
                let dest = exe_dir.join(&*dll_name);
                restore.push((
                    src.to_string_lossy().to_string(),
                    dest.to_string_lossy().to_string(),
                ));
            }
        }
    }

    restore
}

/// Shim for `dirs::data_dir()` / `dirs::home_dir()` — we use a minimal
/// vendored version to avoid adding the full `dirs` crate.
mod dirs {
    use std::path::PathBuf;

    pub fn data_dir() -> Option<PathBuf> {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|h| h.join(".local/share")))
    }

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}
