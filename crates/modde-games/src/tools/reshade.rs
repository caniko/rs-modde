//! ReShade — shader injection for Wine/Proton games.
//!
//! ReShade works by placing a proxy DLL (typically `dxgi.dll` or `d3d11.dll`)
//! in the game directory. Wine needs `WINEDLLOVERRIDES` set to load the native
//! version instead of its built-in stub.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use smallvec::{SmallVec, smallvec};
use tracing::info;

use super::{
    AppliedFiles, GameTool, ToolAvailability, ToolCategory, ToolConfig,
};

pub static RESHADE: ReShade = ReShade;

pub struct ReShade;

impl GameTool for ReShade {
    fn tool_id(&self) -> &'static str {
        "reshade"
    }

    fn display_name(&self) -> &'static str {
        "ReShade"
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::PostProcess
    }

    fn detect_available(&self) -> ToolAvailability {
        // ReShade DLLs must be provided by the user (they're Windows binaries).
        // Check if a source path is configured.
        ToolAvailability::Available {
            version: Some("user-provided".into()),
        }
    }

    fn env_vars(&self, _config: &ToolConfig) -> SmallVec<[(String, String); 4]> {
        SmallVec::new()
    }

    fn wine_dll_overrides(&self, config: &ToolConfig) -> SmallVec<[String; 4]> {
        let dll_name = config.get_str("dll_name").unwrap_or("dxgi");
        smallvec![dll_name.to_string()]
    }

    fn apply(&self, game_dir: &Path, config: &ToolConfig) -> Result<AppliedFiles> {
        let source_dir = config
            .get_str("source_dir")
            .map(PathBuf::from)
            .context("reshade: 'source_dir' setting is required (path to ReShade DLLs)")?;

        let dll_name = config.get_str("dll_name").unwrap_or("dxgi.dll");
        let exe_subdir = config.get_str("exe_subdir").unwrap_or("");

        let target_dir = if exe_subdir.is_empty() {
            game_dir.to_path_buf()
        } else {
            game_dir.join(exe_subdir)
        };

        std::fs::create_dir_all(&target_dir)?;

        let mut applied = AppliedFiles::default();

        // Copy the main DLL
        let src_dll = source_dir.join(dll_name);
        if src_dll.exists() {
            let dest = target_dir.join(dll_name);
            std::fs::copy(&src_dll, &dest)
                .with_context(|| format!("failed to copy ReShade DLL to {}", dest.display()))?;
            let rel = dest.strip_prefix(game_dir).unwrap_or(&dest).to_path_buf();
            applied.files.push(rel);
            info!(dll = %dll_name, "applied ReShade DLL");
        }

        // Copy ReShade.ini if present
        let ini = source_dir.join("ReShade.ini");
        if ini.exists() {
            let dest = target_dir.join("ReShade.ini");
            std::fs::copy(&ini, &dest)?;
            let rel = dest.strip_prefix(game_dir).unwrap_or(&dest).to_path_buf();
            applied.files.push(rel);
        }

        // Copy shader directories if present
        for subdir in &["reshade-shaders", "reshade-presets"] {
            let src = source_dir.join(subdir);
            if src.is_dir() {
                let dest = target_dir.join(subdir);
                copy_dir_recursive(&src, &dest, game_dir, &mut applied)?;
            }
        }

        Ok(applied)
    }

    fn default_config(&self) -> ToolConfig {
        let mut config = ToolConfig::new("reshade");
        config.set("dll_name", serde_json::json!("dxgi.dll"));
        config.set("exe_subdir", serde_json::json!(""));
        config
    }
}

/// Recursively copy a directory, recording all files in [`AppliedFiles`].
fn copy_dir_recursive(
    src: &Path,
    dest: &Path,
    game_dir: &Path,
    applied: &mut AppliedFiles,
) -> Result<()> {
    std::fs::create_dir_all(dest)?;

    for entry in std::fs::read_dir(src)?.flatten() {
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dest_path, game_dir, applied)?;
        } else {
            std::fs::copy(&src_path, &dest_path)?;
            let rel = dest_path
                .strip_prefix(game_dir)
                .unwrap_or(&dest_path)
                .to_path_buf();
            applied.files.push(rel);
        }
    }

    Ok(())
}
