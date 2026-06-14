use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use tracing::{info, warn};

use crate::error::{CoreError, Result};
use crate::fs::{case_match_path, symlink_async};
use crate::paths;
use crate::resolver::{ModId, ResolvedLoadOrder};

// ── Typestate markers ────────────────────────────────────────────

/// Typestate: farm has been built but not yet written to disk.
pub struct Built;
/// Typestate: farm has been materialized on disk and is ready for deployment.
pub struct Materialized;

/// A symlink farm mapping relative file paths to their source locations.
///
/// Uses a typestate pattern (`Built` → `Materialized`) to enforce at
/// compile time that `materialize()` must be called before `deploy_to()`.
/// The `PhantomData<S>` marker is zero-sized and erased at runtime.
#[derive(Debug, Clone)]
pub struct SymlinkFarm<S = Materialized> {
    /// The staging directory for this profile.
    pub staging_dir: PathBuf,
    /// Map of relative path -> absolute source path (winner after priority resolution).
    pub links: HashMap<String, PathBuf>,
    _state: PhantomData<S>,
}

impl SymlinkFarm<Built> {
    /// Construct a `Built` farm directly from a staging directory and link map.
    #[must_use]
    pub fn from_links(staging_dir: PathBuf, links: HashMap<String, PathBuf>) -> Self {
        Self {
            staging_dir,
            links,
            _state: PhantomData,
        }
    }

    /// Build a symlink farm from a resolved load order and the content-addressed store.
    ///
    /// `mod_files` maps each mod ID to its list of `(relative_path, store_entry_path)` pairs.
    /// `overrides` (if provided) are layered on top — override files win over all mods.
    /// `hidden` is a set of `(mod_id, rel_path)` pairs to exclude from the farm.
    pub fn build(
        profile_name: &str,
        resolved: &ResolvedLoadOrder,
        mod_files: &HashMap<ModId, Vec<(String, PathBuf)>>,
        overrides: Option<&[(String, PathBuf)]>,
        hidden: Option<&HashSet<(String, String)>>,
    ) -> Result<Self> {
        let staging_dir = paths::profiles_dir().join(profile_name).join("staging");

        let mut folded_links: HashMap<String, (String, PathBuf)> = HashMap::new();

        // Process mods in load order; later mods override earlier for the same path
        for mod_id in &resolved.order {
            if let Some(files) = mod_files.get(mod_id) {
                for (rel_path, source) in files {
                    // Skip hidden files
                    if let Some(hidden) = hidden
                        && hidden.contains(&(mod_id.0.clone(), rel_path.clone()))
                    {
                        continue;
                    }
                    insert_case_folded_link(
                        &mut folded_links,
                        rel_path,
                        source,
                        Some(mod_id.as_str()),
                    );
                }
            }
        }

        // Profile-level overrides win over all mods
        if let Some(overrides) = overrides {
            for (rel_path, source) in overrides {
                insert_case_folded_link(&mut folded_links, rel_path, source, None);
            }
        }
        let links = folded_links.into_values().collect();

        Ok(Self {
            staging_dir,
            links,
            _state: PhantomData,
        })
    }

    /// Materialize the symlink farm on disk, transitioning to `Materialized` state.
    pub async fn materialize(self) -> Result<SymlinkFarm<Materialized>> {
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
            symlink_async(source, &target).await?;
        }

        info!(
            staging_dir = %self.staging_dir.display(),
            link_count = self.links.len(),
            "symlink farm materialized"
        );

        Ok(SymlinkFarm {
            staging_dir: self.staging_dir,
            links: self.links,
            _state: PhantomData,
        })
    }
}

fn insert_case_folded_link(
    links: &mut HashMap<String, (String, PathBuf)>,
    rel_path: &str,
    source: &Path,
    mod_id: Option<&str>,
) {
    let folded = rel_path.to_ascii_lowercase();
    if let Some((previous_path, previous_source)) =
        links.insert(folded, (rel_path.to_string(), source.to_path_buf()))
        && previous_path != rel_path
    {
        warn!(
            previous_path,
            replacement_path = rel_path,
            previous_source = %previous_source.display(),
            replacement_source = %source.display(),
            mod_id = mod_id.unwrap_or("<profile-overrides>"),
            "case-only VFS path collision resolved by load-order precedence"
        );
    }
}

impl SymlinkFarm<Materialized> {
    /// Deploy the materialized staging directory into the game's mod directory.
    ///
    /// Creates symlinks in `target` pointing to the corresponding files in `staging_dir`.
    pub async fn deploy_to(&self, target: &Path) -> Result<()> {
        if !target.exists() {
            tokio::fs::create_dir_all(target).await?;
        }

        for rel_path in self.links.keys() {
            let src = self.staging_dir.join(rel_path);
            let dst = case_match_path(target, Path::new(rel_path))?;

            if let Some(parent) = dst.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            if dst.symlink_metadata().is_ok() {
                tokio::fs::remove_file(&dst).await?;
            }

            symlink_async(&src, &dst).await?;
        }

        info!(target = %target.display(), "deployment complete");
        Ok(())
    }
}

/// Atomically swap the active staging directory pointer for a profile.
pub async fn rollback(profile_name: &str) -> Result<()> {
    let profile_dir = paths::profiles_dir().join(profile_name);
    let staging = profile_dir.join("staging");
    let backup = profile_dir.join("staging.bak");

    if !backup.exists() {
        return Err(CoreError::Other(
            format!("no backup staging found for profile '{profile_name}'").into(),
        ));
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

#[cfg(test)]
mod tests;
