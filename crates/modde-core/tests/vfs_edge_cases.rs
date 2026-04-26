use std::collections::HashMap;
use std::path::PathBuf;

use modde_core::resolver::{ModId, ResolvedLoadOrder};
use modde_core::vfs::SymlinkFarm;
use tempfile::TempDir;

fn make_resolved(order: Vec<&str>) -> ResolvedLoadOrder {
    ResolvedLoadOrder {
        order: order.into_iter().map(ModId::from).collect(),
    }
}

type ModFiles = HashMap<ModId, Vec<(String, PathBuf)>>;

// ── Build edge cases ────────────────────────────────────────────────

#[test]
fn test_build_many_mods_many_files() {
    let resolved = make_resolved(
        (0..50)
            .map(|i| Box::leak(format!("mod_{i}").into_boxed_str()) as &str)
            .collect(),
    );
    let mut mod_files: ModFiles = HashMap::new();
    for i in 0..50 {
        let mod_id = format!("mod_{i}");
        let files: Vec<(String, PathBuf)> = (0..10)
            .map(|j| {
                (
                    format!("textures/file_{j}.dds"),
                    PathBuf::from(format!("/store/{mod_id}/file_{j}.dds")),
                )
            })
            .collect();
        mod_files.insert(ModId::from(mod_id.as_str()), files);
    }

    let farm = SymlinkFarm::build("stress_test", &resolved, &mod_files, None, None).unwrap();
    // Last mod wins for each file path, so 10 unique files
    assert_eq!(farm.links.len(), 10);
    // All links should point to mod_49 (last in order)
    for source in farm.links.values() {
        assert!(source.to_string_lossy().contains("mod_49"));
    }
}

#[test]
fn test_build_override_chain_last_wins() {
    let resolved = make_resolved(vec!["mod_a", "mod_b", "mod_c", "mod_d"]);
    let mut mod_files: ModFiles = HashMap::new();
    let shared_file = "shared/texture.dds".to_string();

    for mod_id in &["mod_a", "mod_b", "mod_c", "mod_d"] {
        mod_files.insert(
            ModId::from(*mod_id),
            vec![(
                shared_file.clone(),
                PathBuf::from(format!("/store/{mod_id}/texture.dds")),
            )],
        );
    }

    let farm = SymlinkFarm::build("override_chain", &resolved, &mod_files, None, None).unwrap();
    assert_eq!(farm.links.len(), 1);
    assert!(farm.links[&shared_file].to_string_lossy().contains("mod_d"));
}

#[test]
fn test_build_partial_overlap() {
    // mod_a provides file1, file2; mod_b provides file2, file3
    let resolved = make_resolved(vec!["mod_a", "mod_b"]);
    let mut mod_files: ModFiles = HashMap::new();
    mod_files.insert(
        ModId::from("mod_a"),
        vec![
            ("file1.txt".into(), PathBuf::from("/store/mod_a/file1.txt")),
            ("file2.txt".into(), PathBuf::from("/store/mod_a/file2.txt")),
        ],
    );
    mod_files.insert(
        ModId::from("mod_b"),
        vec![
            ("file2.txt".into(), PathBuf::from("/store/mod_b/file2.txt")),
            ("file3.txt".into(), PathBuf::from("/store/mod_b/file3.txt")),
        ],
    );

    let farm = SymlinkFarm::build("partial_overlap", &resolved, &mod_files, None, None).unwrap();
    assert_eq!(farm.links.len(), 3);
    // file1 from mod_a
    assert!(farm.links["file1.txt"].to_string_lossy().contains("mod_a"));
    // file2 from mod_b (overrides mod_a)
    assert!(farm.links["file2.txt"].to_string_lossy().contains("mod_b"));
    // file3 from mod_b
    assert!(farm.links["file3.txt"].to_string_lossy().contains("mod_b"));
}

#[test]
fn test_build_empty_order_with_nonempty_mod_files() {
    let resolved = make_resolved(vec![]);
    let mut mod_files: ModFiles = HashMap::new();
    mod_files.insert(
        ModId::from("orphan_mod"),
        vec![("file.txt".into(), PathBuf::from("/store/orphan/file.txt"))],
    );

    let farm = SymlinkFarm::build("empty_order", &resolved, &mod_files, None, None).unwrap();
    assert!(farm.links.is_empty(), "mods not in order should not appear");
}

#[test]
fn test_build_staging_dir_path_format() {
    let resolved = make_resolved(vec![]);
    let mod_files: ModFiles = HashMap::new();

    let farm = SymlinkFarm::build("my-profile", &resolved, &mod_files, None, None).unwrap();
    assert!(farm.staging_dir.to_string_lossy().contains("my-profile"));
    assert!(farm.staging_dir.to_string_lossy().contains("staging"));
}

#[test]
fn test_build_mod_with_many_files() {
    let resolved = make_resolved(vec!["big_mod"]);
    let files: Vec<(String, PathBuf)> = (0..1000)
        .map(|i| {
            (
                format!("data/file_{i}.esp"),
                PathBuf::from(format!("/store/big_mod/file_{i}.esp")),
            )
        })
        .collect();
    let mut mod_files: ModFiles = HashMap::new();
    mod_files.insert(ModId::from("big_mod"), files);

    let farm = SymlinkFarm::build("big_mod_test", &resolved, &mod_files, None, None).unwrap();
    assert_eq!(farm.links.len(), 1000);
}

// ── Materialize edge cases ──────────────────────────────────────────

#[tokio::test]
async fn test_materialize_many_files() {
    let tmp = TempDir::new().unwrap();
    let staging_dir = tmp.path().join("staging");

    // Create source files
    let src_dir = tmp.path().join("sources");
    std::fs::create_dir_all(&src_dir).unwrap();

    let mut links = HashMap::new();
    for i in 0..100 {
        let src = src_dir.join(format!("file_{i}.txt"));
        std::fs::write(&src, format!("content_{i}")).unwrap();
        links.insert(format!("data/file_{i}.txt"), src);
    }

    let farm = SymlinkFarm::from_links(staging_dir.clone(), links);
    farm.materialize().await.unwrap();

    // Verify all symlinks exist and are readable
    for i in 0..100 {
        let link = staging_dir.join(format!("data/file_{i}.txt"));
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(
            std::fs::read_to_string(&link).unwrap(),
            format!("content_{i}")
        );
    }
}

#[tokio::test]
async fn test_materialize_idempotent() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("src.txt");
    std::fs::write(&source, "data").unwrap();

    let staging_dir = tmp.path().join("staging");
    let mut links = HashMap::new();
    links.insert("file.txt".to_string(), source.clone());

    let farm = SymlinkFarm::from_links(staging_dir.clone(), links.clone());

    // Materialize twice should work (second call cleans and recreates)
    farm.materialize().await.unwrap();
    let farm2 = SymlinkFarm::from_links(staging_dir.clone(), links);
    farm2.materialize().await.unwrap();

    let link = staging_dir.join("file.txt");
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read_to_string(&link).unwrap(), "data");
}

// ── Deploy edge cases ───────────────────────────────────────────────

#[tokio::test]
async fn test_deploy_empty_farm() {
    let tmp = TempDir::new().unwrap();
    let staging_dir = tmp.path().join("staging");
    let target_dir = tmp.path().join("game");

    let farm = SymlinkFarm::from_links(staging_dir.clone(), HashMap::new());
    std::fs::create_dir_all(&staging_dir).unwrap();

    let farm = farm.materialize().await.unwrap();
    farm.deploy_to(&target_dir).await.unwrap();
    assert!(target_dir.exists());
    let entries: Vec<_> = std::fs::read_dir(&target_dir).unwrap().collect();
    assert!(entries.is_empty());
}

#[tokio::test]
async fn test_deploy_replaces_symlink_with_new_symlink() {
    let tmp = TempDir::new().unwrap();
    let staging_dir = tmp.path().join("staging");
    let target_dir = tmp.path().join("game");

    // Create first deployment
    let src1 = tmp.path().join("src1.txt");
    std::fs::write(&src1, "version1").unwrap();
    let mut links = HashMap::new();
    links.insert("mod.esp".to_string(), src1.clone());

    let farm1 = SymlinkFarm::from_links(staging_dir.clone(), links);
    let farm1 = farm1.materialize().await.unwrap();
    farm1.deploy_to(&target_dir).await.unwrap();

    // Create second deployment with different source
    let src2 = tmp.path().join("src2.txt");
    std::fs::write(&src2, "version2").unwrap();
    let mut links2 = HashMap::new();
    links2.insert("mod.esp".to_string(), src2.clone());

    let farm2 = SymlinkFarm::from_links(staging_dir.clone(), links2);
    let farm2 = farm2.materialize().await.unwrap();
    farm2.deploy_to(&target_dir).await.unwrap();

    // Should read the new content through symlink chain
    let deployed = target_dir.join("mod.esp");
    assert!(
        deployed
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_to_string(&deployed).unwrap(), "version2");
}

// ── Rollback edge cases ─────────────────────────────────────────────

#[tokio::test]
async fn test_rollback_when_staging_does_not_exist_but_backup_does() {
    let tmp = TempDir::new().unwrap();
    unsafe {
        std::env::set_var("XDG_DATA_HOME", tmp.path());
    }

    let profile_dir = tmp.path().join("modde/profiles/rollback_no_staging");
    let backup = profile_dir.join("staging.bak");

    std::fs::create_dir_all(&backup).unwrap();
    std::fs::write(backup.join("restored.txt"), "from backup").unwrap();

    modde_core::vfs::rollback("rollback_no_staging")
        .await
        .unwrap();

    let staging = profile_dir.join("staging");
    assert!(staging.join("restored.txt").exists());
    assert_eq!(
        std::fs::read_to_string(staging.join("restored.txt")).unwrap(),
        "from backup"
    );
    assert!(!backup.exists());
}
