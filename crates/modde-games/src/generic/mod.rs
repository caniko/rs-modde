use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::traits::GamePlugin;

/// A generic game with loose file drop support.
pub struct GenericGame {
    id: String,
    name: String,
    install_path: Option<PathBuf>,
    mod_dir: String,
}

impl GenericGame {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        install_path: Option<PathBuf>,
        mod_dir: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            install_path,
            mod_dir: mod_dir.into(),
        }
    }
}

impl GamePlugin for GenericGame {
    fn game_id(&self) -> &str {
        &self.id
    }

    fn display_name(&self) -> &str {
        &self.name
    }

    fn detect_install(&self) -> Option<PathBuf> {
        self.install_path.clone().filter(|p| p.exists())
    }

    fn mod_directory(&self, install: &Path) -> PathBuf {
        install.join(&self.mod_dir)
    }

    fn deploy(&self, staging: &Path, target: &Path) -> Result<()> {
        if !target.exists() {
            std::fs::create_dir_all(target)?;
        }
        // Loose file drop: symlink everything from staging into target
        copy_symlinks_recursive(staging, target)?;
        Ok(())
    }

    fn post_deploy(&self, _install: &Path) -> Result<()> {
        Ok(())
    }
}

fn copy_symlinks_recursive(src: &Path, dst: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path)?;
            copy_symlinks_recursive(&src_path, &dst_path)?;
        } else {
            if dst_path.exists() || dst_path.symlink_metadata().is_ok() {
                std::fs::remove_file(&dst_path)?;
            }
            std::os::unix::fs::symlink(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
