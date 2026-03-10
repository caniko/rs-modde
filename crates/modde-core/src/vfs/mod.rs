use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tracing::{info, warn};

use crate::error::{CoreError, Result};
use crate::resolver::{ModId, ResolvedLoadOrder};

/// A symlink farm mapping relative file paths to their source locations.
#[derive(Debug, Clone)]
pub struct SymlinkFarm {
    /// The staging directory for this profile.
    pub staging_dir: PathBuf,
    /// Map of relative path -> absolute source path (winner after priority resolution).
    pub links: HashMap<String, PathBuf>,
}

impl SymlinkFarm {
    /// Build a symlink farm from a resolved load order and the content-addressed store.
    ///
    /// `store_path` is the root of `~/.local/share/modde/store/`.
    /// `mod_files` maps each mod ID to its list of (relative_path, store_entry_path) pairs.
    pub fn build(
        profile_name: &str,
        resolved: &ResolvedLoadOrder,
        mod_files: &HashMap<ModId, Vec<(String, PathBuf)>>,
    ) -> Result<Self> {
        let staging_dir = dirs_path().join("profiles").join(profile_name).join("staging");

        let mut links: HashMap<String, PathBuf> = HashMap::new();

        // Process mods in load order; later mods override earlier for the same path
        for mod_id in &resolved.order {
            if let Some(files) = mod_files.get(mod_id) {
                for (rel_path, source) in files {
                    links.insert(rel_path.clone(), source.clone());
                }
            }
        }

        Ok(Self { staging_dir, links })
    }

    /// Materialize the symlink farm on disk.
    pub async fn materialize(&self) -> Result<()> {
        // Clean existing staging dir
        if self.staging_dir.exists() {
            tokio::fs::remove_dir_all(&self.staging_dir).await?;
        }
        tokio::fs::create_dir_all(&self.staging_dir).await?;

        for (rel_path, source) in &self.links {
            let target = self.staging_dir.join(rel_path);
            if let Some(parent) = target.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::symlink(source, &target).await?;
        }

        info!(
            staging_dir = %self.staging_dir.display(),
            link_count = self.links.len(),
            "symlink farm materialized"
        );

        Ok(())
    }
}

/// Deploy a staging directory into the game's mod directory via recursive symlinks.
pub async fn deploy(farm: &SymlinkFarm, target: &Path) -> Result<()> {
    if !target.exists() {
        tokio::fs::create_dir_all(target).await?;
    }

    for (rel_path, _) in &farm.links {
        let src = farm.staging_dir.join(rel_path);
        let dst = target.join(rel_path);

        if let Some(parent) = dst.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Remove existing file/symlink at destination
        if dst.exists() || dst.symlink_metadata().is_ok() {
            tokio::fs::remove_file(&dst).await.ok();
        }

        tokio::fs::symlink(&src, &dst).await?;
    }

    info!(
        target = %target.display(),
        "deployment complete"
    );

    Ok(())
}

/// Atomically swap the active staging directory pointer for a profile.
pub async fn rollback(profile_name: &str) -> Result<()> {
    let profile_dir = dirs_path().join("profiles").join(profile_name);
    let staging = profile_dir.join("staging");
    let backup = profile_dir.join("staging.bak");

    if !backup.exists() {
        return Err(CoreError::Other(format!(
            "no backup staging found for profile '{profile_name}'"
        )));
    }

    if staging.exists() {
        let tmp = profile_dir.join("staging.old");
        tokio::fs::rename(&staging, &tmp).await?;
        tokio::fs::rename(&backup, &staging).await?;
        tokio::fs::remove_dir_all(&tmp).await?;
    } else {
        tokio::fs::rename(&backup, &staging).await?;
    }

    warn!(profile = profile_name, "rolled back to previous staging");

    Ok(())
}

/// Base data directory for modde.
fn dirs_path() -> PathBuf {
    dirs_data_local().join("modde")
}

fn dirs_data_local() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".local/share")
        })
}
