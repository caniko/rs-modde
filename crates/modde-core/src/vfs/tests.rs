use super::*;
use crate::resolver::{ModId, ResolvedLoadOrder};
use std::collections::HashMap;
use tempfile::TempDir;

mod deploy;

fn make_resolved(order: Vec<&str>) -> ResolvedLoadOrder {
    ResolvedLoadOrder {
        order: order.into_iter().map(ModId::from).collect(),
    }
}

/// Helper: construct a `Built` farm directly for testing.
pub(super) fn test_farm(
    staging_dir: PathBuf,
    links: HashMap<String, PathBuf>,
) -> SymlinkFarm<Built> {
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
fn test_build_case_only_collision_uses_load_order_winner() {
    let resolved = make_resolved(vec!["mod_a", "mod_b"]);
    let source_a = PathBuf::from("/store/mod_a/Textures/Foo.dds");
    let source_b = PathBuf::from("/store/mod_b/textures/foo.dds");

    let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
    mod_files.insert(
        "mod_a".into(),
        vec![("Textures/Foo.dds".into(), source_a.clone())],
    );
    mod_files.insert(
        "mod_b".into(),
        vec![("textures/foo.dds".into(), source_b.clone())],
    );

    let farm = SymlinkFarm::build("test_profile", &resolved, &mod_files, None, None).unwrap();
    assert_eq!(farm.links.len(), 1);
    assert_eq!(farm.links.get("textures/foo.dds").unwrap(), &source_b);
    assert!(!farm.links.contains_key("Textures/Foo.dds"));
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
    let _farm = farm.materialize().await.unwrap();

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
