pub mod manifest;
pub mod redmod;

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::traits::GamePlugin;

pub struct Cyberpunk2077;

impl GamePlugin for Cyberpunk2077 {
    fn game_id(&self) -> &str {
        "cyberpunk2077"
    }

    fn display_name(&self) -> &str {
        "Cyberpunk 2077"
    }

    fn detect_install(&self) -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        let path = PathBuf::from(home)
            .join(".local/share/Steam/steamapps/common/Cyberpunk 2077");
        path.exists().then_some(path)
    }

    fn mod_directory(&self, install: &Path) -> PathBuf {
        install.join("mods")
    }

    fn deploy(&self, staging: &Path, target: &Path) -> Result<()> {
        if !target.exists() {
            std::fs::create_dir_all(target)?;
        }
        // Symlink each mod directory from staging into game mods dir
        for entry in std::fs::read_dir(staging)? {
            let entry = entry?;
            let dst = target.join(entry.file_name());
            if dst.exists() || dst.symlink_metadata().is_ok() {
                if dst.is_dir() {
                    std::fs::remove_dir_all(&dst)?;
                } else {
                    std::fs::remove_file(&dst)?;
                }
            }
            std::os::unix::fs::symlink(entry.path(), &dst)?;
        }
        Ok(())
    }

    fn post_deploy(&self, install: &Path) -> Result<()> {
        // Run REDmod deploy if available
        redmod::deploy_if_available(install)
    }
}
