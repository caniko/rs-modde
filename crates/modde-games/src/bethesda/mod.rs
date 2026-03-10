pub mod archives;
pub mod fomod;
pub mod ini;
pub mod plugins_txt;

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::traits::GamePlugin;

fn steam_common() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".local/share/Steam/steamapps/common")
}

fn detect_steam_game(dir_name: &str) -> Option<PathBuf> {
    let path = steam_common().join(dir_name);
    path.exists().then_some(path)
}

fn deploy_symlinks(staging: &Path, target: &Path) -> Result<()> {
    if !target.exists() {
        std::fs::create_dir_all(target)?;
    }
    for entry in walkdir(staging)? {
        let rel = entry.strip_prefix(staging)?;
        let dst = target.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&dst)?;
        } else {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if dst.exists() || dst.symlink_metadata().is_ok() {
                std::fs::remove_file(&dst)?;
            }
            std::os::unix::fs::symlink(&entry, &dst)?;
        }
    }
    Ok(())
}

fn walkdir(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(walkdir(&path)?);
        } else {
            files.push(path);
        }
    }
    Ok(files)
}

// --- Game Implementations ---

pub struct SkyrimSE;

impl GamePlugin for SkyrimSE {
    fn game_id(&self) -> &str {
        "skyrim-se"
    }
    fn display_name(&self) -> &str {
        "The Elder Scrolls V: Skyrim Special Edition"
    }
    fn detect_install(&self) -> Option<PathBuf> {
        detect_steam_game("Skyrim Special Edition")
    }
    fn mod_directory(&self, install: &Path) -> PathBuf {
        install.join("Data")
    }
    fn deploy(&self, staging: &Path, target: &Path) -> Result<()> {
        deploy_symlinks(staging, target)
    }
    fn post_deploy(&self, _install: &Path) -> Result<()> {
        Ok(())
    }
}

pub struct SkyrimAE;

impl GamePlugin for SkyrimAE {
    fn game_id(&self) -> &str {
        "skyrim-ae"
    }
    fn display_name(&self) -> &str {
        "The Elder Scrolls V: Skyrim Anniversary Edition"
    }
    fn detect_install(&self) -> Option<PathBuf> {
        // AE shares the same directory as SE on Steam
        detect_steam_game("Skyrim Special Edition")
    }
    fn mod_directory(&self, install: &Path) -> PathBuf {
        install.join("Data")
    }
    fn deploy(&self, staging: &Path, target: &Path) -> Result<()> {
        deploy_symlinks(staging, target)
    }
    fn post_deploy(&self, _install: &Path) -> Result<()> {
        Ok(())
    }
}

pub struct Fallout4;

impl GamePlugin for Fallout4 {
    fn game_id(&self) -> &str {
        "fallout4"
    }
    fn display_name(&self) -> &str {
        "Fallout 4"
    }
    fn detect_install(&self) -> Option<PathBuf> {
        detect_steam_game("Fallout 4")
    }
    fn mod_directory(&self, install: &Path) -> PathBuf {
        install.join("Data")
    }
    fn deploy(&self, staging: &Path, target: &Path) -> Result<()> {
        deploy_symlinks(staging, target)
    }
    fn post_deploy(&self, _install: &Path) -> Result<()> {
        Ok(())
    }
}

pub struct Fallout76;

impl GamePlugin for Fallout76 {
    fn game_id(&self) -> &str {
        "fallout76"
    }
    fn display_name(&self) -> &str {
        "Fallout 76"
    }
    fn detect_install(&self) -> Option<PathBuf> {
        detect_steam_game("Fallout76")
    }
    fn mod_directory(&self, install: &Path) -> PathBuf {
        install.join("Data")
    }
    fn deploy(&self, staging: &Path, target: &Path) -> Result<()> {
        deploy_symlinks(staging, target)
    }
    fn post_deploy(&self, _install: &Path) -> Result<()> {
        Ok(())
    }
}
