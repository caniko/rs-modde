use std::collections::HashMap;

use tempfile::TempDir;

use super::test_farm;
use crate::vfs::rollback;

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

#[tokio::test]
async fn test_deploy_matches_existing_target_tree_casing() {
    let tmp = TempDir::new().unwrap();
    let staging_dir = tmp.path().join("staging");
    let target_dir = tmp.path().join("game/mods");

    std::fs::create_dir_all(target_dir.join("textures")).unwrap();
    std::fs::write(target_dir.join("textures/sky.dds"), "old").unwrap();

    let source_file = tmp.path().join("source.dds");
    std::fs::write(&source_file, "new").unwrap();

    let mut links = HashMap::new();
    links.insert("Textures/SKY.DDS".to_string(), source_file.clone());

    let farm = test_farm(staging_dir.clone(), links);
    let farm = farm.materialize().await.unwrap();

    farm.deploy_to(&target_dir).await.unwrap();

    let deployed = target_dir.join("textures/sky.dds");
    assert!(
        deployed
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_link(&deployed).unwrap(),
        staging_dir.join("Textures/SKY.DDS")
    );
    assert_eq!(std::fs::read_to_string(&deployed).unwrap(), "new");
    assert!(!target_dir.join("Textures/SKY.DDS").exists());
}

// ========================================================================
// Tests for rollback()
// ========================================================================

#[tokio::test]
async fn test_rollback_no_backup() {
    let tmp = TempDir::new().unwrap();
    // Point XDG_DATA_HOME to our temp dir so dirs_path() resolves there
    // SAFETY: this test owns the process-global XDG_DATA_HOME mutation for
    // the duration of the assertion and uses a unique temp directory.
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
    // SAFETY: this ignored test mutates XDG_DATA_HOME only when explicitly
    // run, and points it at an isolated temp directory.
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
