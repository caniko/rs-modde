use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use tracing::{info, warn};

use crate::error::{CoreError, Result};
use crate::fs::symlink_async;
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

        let mut links: HashMap<String, PathBuf> = HashMap::new();

        // Process mods in load order; later mods override earlier for the same path
        for mod_id in &resolved.order {
            if let Some(files) = mod_files.get(mod_id) {
                for (rel_path, source) in files {
                    // Skip hidden files
                    if let Some(hidden) = hidden {
                        if hidden.contains(&(mod_id.0.clone(), rel_path.clone())) {
                            continue;
                        }
                    }
                    links.insert(rel_path.clone(), source.clone());
                }
            }
        }

        // Profile-level overrides win over all mods
        if let Some(overrides) = overrides {
            for (rel_path, source) in overrides {
                links.insert(rel_path.clone(), source.clone());
            }
        }

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
            let dst = target.join(rel_path);

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
mod tests {
    use super::*;
    use crate::resolver::{ModId, ResolvedLoadOrder};
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn make_resolved(order: Vec<&str>) -> ResolvedLoadOrder {
        ResolvedLoadOrder {
            order: order.into_iter().map(ModId::from).collect(),
        }
    }

    /// Helper: construct a `Built` farm directly for testing.
    fn test_farm(staging_dir: PathBuf, links: HashMap<String, PathBuf>) -> SymlinkFarm<Built> {
        SymlinkFarm::from_links(staging_dir, links)
    }

    // ========================================================================
    // Unit tests for build()
    // ========================================================================

    #[test]
    fn test_build_empty_mod_files() {
        let resolved = make_resolved(vec![]);
        let mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();

        let farm = SymlinkFarm::build("test_profile", &resolved, &mod_files, None, None).unwrap();
        assert!(farm.links.is_empty());
    }

    #[test]
    fn test_build_single_mod() {
        let resolved = make_resolved(vec!["mod_a"]);
        let source = PathBuf::from("/store/mod_a/textures/sky.dds");
        let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
        mod_files.insert(
            "mod_a".into(),
            vec![("textures/sky.dds".into(), source.clone())],
        );

        let farm = SymlinkFarm::build("test_profile", &resolved, &mod_files, None, None).unwrap();
        assert_eq!(farm.links.len(), 1);
        assert_eq!(farm.links.get("textures/sky.dds").unwrap(), &source);
    }

    #[test]
    fn test_build_override_order() {
        // Later mod in load order should override earlier mod for the same relative path.
        let resolved = make_resolved(vec!["mod_a", "mod_b"]);
        let source_a = PathBuf::from("/store/mod_a/meshes/body.nif");
        let source_b = PathBuf::from("/store/mod_b/meshes/body.nif");

        let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
        mod_files.insert(
            "mod_a".into(),
            vec![("meshes/body.nif".into(), source_a.clone())],
        );
        mod_files.insert(
            "mod_b".into(),
            vec![("meshes/body.nif".into(), source_b.clone())],
        );

        let farm = SymlinkFarm::build("test_profile", &resolved, &mod_files, None, None).unwrap();
        assert_eq!(farm.links.len(), 1);
        // mod_b is later, so it wins
        assert_eq!(farm.links.get("meshes/body.nif").unwrap(), &source_b);
    }

    #[test]
    fn test_build_multiple_mods_different_files() {
        let resolved = make_resolved(vec!["mod_a", "mod_b"]);
        let source_a = PathBuf::from("/store/mod_a/textures/sky.dds");
        let source_b = PathBuf::from("/store/mod_b/meshes/tree.nif");

        let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
        mod_files.insert(
            "mod_a".into(),
            vec![("textures/sky.dds".into(), source_a.clone())],
        );
        mod_files.insert(
            "mod_b".into(),
            vec![("meshes/tree.nif".into(), source_b.clone())],
        );

        let farm = SymlinkFarm::build("test_profile", &resolved, &mod_files, None, None).unwrap();
        assert_eq!(farm.links.len(), 2);
        assert_eq!(farm.links.get("textures/sky.dds").unwrap(), &source_a);
        assert_eq!(farm.links.get("meshes/tree.nif").unwrap(), &source_b);
    }

    #[test]
    fn test_build_mod_not_in_mod_files() {
        // A mod in resolved.order that has no entry in mod_files should be silently skipped.
        let resolved = make_resolved(vec!["mod_a", "mod_missing", "mod_b"]);
        let source_a = PathBuf::from("/store/mod_a/file_a.txt");
        let source_b = PathBuf::from("/store/mod_b/file_b.txt");

        let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
        mod_files.insert(
            "mod_a".into(),
            vec![("file_a.txt".into(), source_a.clone())],
        );
        mod_files.insert(
            "mod_b".into(),
            vec![("file_b.txt".into(), source_b.clone())],
        );

        let farm = SymlinkFarm::build("test_profile", &resolved, &mod_files, None, None).unwrap();
        assert_eq!(farm.links.len(), 2);
        assert_eq!(farm.links.get("file_a.txt").unwrap(), &source_a);
        assert_eq!(farm.links.get("file_b.txt").unwrap(), &source_b);
    }

    #[test]
    fn test_build_deep_nested_paths() {
        let resolved = make_resolved(vec!["mod_a"]);
        let source = PathBuf::from("/store/mod_a/a/b/c/d/e/deep_file.esp");

        let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
        mod_files.insert(
            "mod_a".into(),
            vec![("a/b/c/d/e/deep_file.esp".into(), source.clone())],
        );

        let farm = SymlinkFarm::build("test_profile", &resolved, &mod_files, None, None).unwrap();
        assert_eq!(farm.links.len(), 1);
        assert_eq!(farm.links.get("a/b/c/d/e/deep_file.esp").unwrap(), &source);
    }

    #[test]
    fn test_build_hidden_files() {
        let resolved = make_resolved(vec!["mod_a", "mod_b"]);
        let source_a1 = PathBuf::from("/store/mod_a/textures/sky.dds");
        let source_a2 = PathBuf::from("/store/mod_a/meshes/tree.nif");
        let source_b = PathBuf::from("/store/mod_b/textures/sky.dds");

        let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
        mod_files.insert(
            "mod_a".into(),
            vec![
                ("textures/sky.dds".into(), source_a1.clone()),
                ("meshes/tree.nif".into(), source_a2.clone()),
            ],
        );
        mod_files.insert(
            "mod_b".into(),
            vec![("textures/sky.dds".into(), source_b.clone())],
        );

        // Hide mod_b's sky.dds — mod_a's version should win
        let mut hidden = HashSet::new();
        hidden.insert(("mod_b".to_string(), "textures/sky.dds".to_string()));

        let farm =
            SymlinkFarm::build("test_profile", &resolved, &mod_files, None, Some(&hidden)).unwrap();
        assert_eq!(farm.links.len(), 2);
        // mod_a's sky.dds should win since mod_b's is hidden
        assert_eq!(farm.links.get("textures/sky.dds").unwrap(), &source_a1);
        assert_eq!(farm.links.get("meshes/tree.nif").unwrap(), &source_a2);
    }

    // ========================================================================
    // Integration tests for materialize()
    // ========================================================================

    #[tokio::test]
    async fn test_materialize_creates_symlinks() {
        let tmp = TempDir::new().unwrap();

        // Create a real source file so the symlink target exists
        let source_file = tmp.path().join("source.txt");
        std::fs::write(&source_file, "hello").unwrap();

        let staging_dir = tmp.path().join("staging");
        let mut links = HashMap::new();
        links.insert("data/source.txt".to_string(), source_file.clone());

        let farm = test_farm(staging_dir.clone(), links);
        let farm = farm.materialize().await.unwrap();

        let link_path = staging_dir.join("data/source.txt");
        assert!(
            link_path
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_link(&link_path).unwrap(), source_file);
        assert_eq!(std::fs::read_to_string(&link_path).unwrap(), "hello");
    }

    #[tokio::test]
    async fn test_materialize_empty_links() {
        let tmp = TempDir::new().unwrap();
        let staging_dir = tmp.path().join("staging");

        let farm = test_farm(staging_dir.clone(), HashMap::new());
        farm.materialize().await.unwrap();

        assert!(staging_dir.exists());
        // Directory should be empty
        let entries: Vec<_> = std::fs::read_dir(&staging_dir).unwrap().collect();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn test_materialize_deep_subdirectories() {
        let tmp = TempDir::new().unwrap();
        let source_file = tmp.path().join("original.dds");
        std::fs::write(&source_file, "texture data").unwrap();

        let staging_dir = tmp.path().join("staging");
        let mut links = HashMap::new();
        links.insert(
            "textures/landscape/snow/detail.dds".to_string(),
            source_file.clone(),
        );

        let farm = test_farm(staging_dir.clone(), links);
        farm.materialize().await.unwrap();

        let link_path = staging_dir.join("textures/landscape/snow/detail.dds");
        assert!(
            link_path
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&link_path).unwrap(), "texture data");
    }

    #[tokio::test]
    async fn test_materialize_cleans_existing_staging() {
        let tmp = TempDir::new().unwrap();
        let staging_dir = tmp.path().join("staging");

        // Create a pre-existing staging dir with some leftover file
        std::fs::create_dir_all(&staging_dir).unwrap();
        std::fs::write(staging_dir.join("old_file.txt"), "stale").unwrap();

        let source_file = tmp.path().join("new_source.txt");
        std::fs::write(&source_file, "fresh").unwrap();

        let mut links = HashMap::new();
        links.insert("new_file.txt".to_string(), source_file.clone());

        let farm = test_farm(staging_dir.clone(), links);
        farm.materialize().await.unwrap();

        // Old file should be gone
        assert!(!staging_dir.join("old_file.txt").exists());
        // New symlink should exist
        assert!(
            staging_dir
                .join("new_file.txt")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    // ========================================================================
    // Integration tests for deploy()
    // ========================================================================

    #[tokio::test]
    async fn test_deploy_creates_target_dir() {
        let tmp = TempDir::new().unwrap();
        let staging_dir = tmp.path().join("staging");
        let target_dir = tmp.path().join("game/mods");

        let source_file = tmp.path().join("source.esp");
        std::fs::write(&source_file, "plugin data").unwrap();

        let mut links = HashMap::new();
        links.insert("mod.esp".to_string(), source_file.clone());

        let farm = test_farm(staging_dir.clone(), links);
        let farm = farm.materialize().await.unwrap();

        assert!(!target_dir.exists());
        farm.deploy_to(&target_dir).await.unwrap();
        assert!(target_dir.exists());
        assert!(target_dir.is_dir());
    }

    #[tokio::test]
    async fn test_deploy_creates_symlinks_in_target() {
        let tmp = TempDir::new().unwrap();
        let staging_dir = tmp.path().join("staging");
        let target_dir = tmp.path().join("game/mods");

        let source_file = tmp.path().join("source.esp");
        std::fs::write(&source_file, "plugin").unwrap();

        let mut links = HashMap::new();
        links.insert("plugin.esp".to_string(), source_file.clone());

        let farm = test_farm(staging_dir.clone(), links);
        let farm = farm.materialize().await.unwrap();

        farm.deploy_to(&target_dir).await.unwrap();

        let deployed = target_dir.join("plugin.esp");
        assert!(
            deployed
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let link_target = std::fs::read_link(&deployed).unwrap();
        assert_eq!(link_target, staging_dir.join("plugin.esp"));
    }

    #[tokio::test]
    async fn test_deploy_overwrites_existing() {
        let tmp = TempDir::new().unwrap();
        let staging_dir = tmp.path().join("staging");
        let target_dir = tmp.path().join("game/mods");

        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(target_dir.join("replaceme.txt"), "old content").unwrap();

        let source_file = tmp.path().join("new_source.txt");
        std::fs::write(&source_file, "new content").unwrap();

        let mut links = HashMap::new();
        links.insert("replaceme.txt".to_string(), source_file.clone());

        let farm = test_farm(staging_dir.clone(), links);
        let farm = farm.materialize().await.unwrap();

        farm.deploy_to(&target_dir).await.unwrap();

        let deployed = target_dir.join("replaceme.txt");
        assert!(
            deployed
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&deployed).unwrap(), "new content");
    }

    #[tokio::test]
    async fn test_deploy_nested_structure() {
        let tmp = TempDir::new().unwrap();
        let staging_dir = tmp.path().join("staging");
        let target_dir = tmp.path().join("game/mods");

        let src1 = tmp.path().join("src1.dds");
        let src2 = tmp.path().join("src2.nif");
        std::fs::write(&src1, "texture").unwrap();
        std::fs::write(&src2, "mesh").unwrap();

        let mut links = HashMap::new();
        links.insert("textures/landscape/dirt.dds".to_string(), src1.clone());
        links.insert("meshes/architecture/wall.nif".to_string(), src2.clone());

        let farm = test_farm(staging_dir.clone(), links);
        let farm = farm.materialize().await.unwrap();

        farm.deploy_to(&target_dir).await.unwrap();

        assert!(
            target_dir
                .join("textures/landscape/dirt.dds")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            target_dir
                .join("meshes/architecture/wall.nif")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    // ========================================================================
    // Tests for rollback()
    // ========================================================================

    #[tokio::test]
    async fn test_rollback_no_backup() {
        let tmp = TempDir::new().unwrap();
        // Point XDG_DATA_HOME to our temp dir so dirs_path() resolves there
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path());
        }

        let profile_dir = tmp.path().join("modde/profiles/rollback_test_no_bak");
        std::fs::create_dir_all(profile_dir.join("staging")).unwrap();

        let result = rollback("rollback_test_no_bak").await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("no backup staging found"),
            "unexpected error: {err_msg}"
        );
    }

    #[tokio::test]
    #[ignore = "env var race: XDG_DATA_HOME set_var is not thread-safe across parallel tests"]
    async fn test_rollback_swaps_dirs() {
        let tmp = TempDir::new().unwrap();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path());
        }

        let profile_dir = tmp.path().join("modde/profiles/rollback_test_swap");
        let staging = profile_dir.join("staging");
        let backup = profile_dir.join("staging.bak");

        std::fs::create_dir_all(&staging).unwrap();
        std::fs::create_dir_all(&backup).unwrap();

        // Write marker files so we can verify the swap
        std::fs::write(staging.join("current.txt"), "I am current").unwrap();
        std::fs::write(backup.join("backup.txt"), "I am backup").unwrap();

        rollback("rollback_test_swap").await.unwrap();

        // After rollback, staging should contain the backup's content
        assert!(staging.join("backup.txt").exists());
        assert_eq!(
            std::fs::read_to_string(staging.join("backup.txt")).unwrap(),
            "I am backup"
        );
        // The old staging (with current.txt) should have been removed
        assert!(!staging.join("current.txt").exists());
        // staging.bak should no longer exist (it was renamed to staging)
        assert!(!backup.exists());
    }
}
