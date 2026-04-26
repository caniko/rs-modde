use std::path::PathBuf;

use modde_games::GamePlugin;
use modde_games::generic::GenericGame;

// ── GenericGame::new with various parameters ────────────────────────

#[test]
fn test_generic_game_new_basic() {
    let game = GenericGame::new("mygame", "My Game", None, "mods");
    assert_eq!(game.game_id(), "mygame");
    assert_eq!(game.display_name(), "My Game");
}

#[test]
fn test_generic_game_new_with_install_path() {
    let tmp = tempfile::tempdir().unwrap();
    let game = GenericGame::new(
        "testgame",
        "Test Game",
        Some(tmp.path().to_path_buf()),
        "data/mods",
    );
    assert_eq!(game.game_id(), "testgame");
    assert_eq!(game.display_name(), "Test Game");
}

#[test]
fn test_generic_game_new_string_types() {
    // Verify Into<String> works for String as well as &str
    let game = GenericGame::new(
        String::from("game_id"),
        String::from("Game Name"),
        None,
        String::from("moddir"),
    );
    assert_eq!(game.game_id(), "game_id");
    assert_eq!(game.display_name(), "Game Name");
}

// ── GenericGame detect_install ──────────────────────────────────────

#[test]
fn test_generic_game_detect_install_returns_existing_path() {
    let tmp = tempfile::tempdir().unwrap();
    let game = GenericGame::new("game", "Game", Some(tmp.path().to_path_buf()), "mods");
    // tmp.path() exists, so detect_install should return it
    let detected = game.detect_install();
    assert!(detected.is_some());
    assert_eq!(detected.unwrap(), tmp.path());
}

#[test]
fn test_generic_game_detect_install_returns_none_when_not_configured() {
    let game = GenericGame::new("game", "Game", None, "mods");
    assert!(game.detect_install().is_none());
}

#[test]
fn test_generic_game_detect_install_returns_none_when_path_nonexistent() {
    let game = GenericGame::new(
        "game",
        "Game",
        Some(PathBuf::from("/nonexistent/path/that/should/not/exist")),
        "mods",
    );
    assert!(game.detect_install().is_none());
}

// ── GenericGame mod_directory ────────────────────────────────────────

#[test]
fn test_generic_game_mod_directory_simple() {
    let game = GenericGame::new("game", "Game", None, "mods");
    let install = std::path::Path::new("/game/install");
    assert_eq!(game.mod_directory(install), install.join("mods"));
}

#[test]
fn test_generic_game_mod_directory_nested() {
    let game = GenericGame::new("game", "Game", None, "data/mods/custom");
    let install = std::path::Path::new("/opt/games/mygame");
    assert_eq!(
        game.mod_directory(install),
        install.join("data/mods/custom")
    );
}

// ── GenericGame deploy with nested directories ──────────────────────

#[test]
fn test_generic_game_deploy_creates_symlinks() {
    let staging = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();

    // Create a file in staging
    std::fs::write(staging.path().join("mod_file.txt"), b"mod data").unwrap();

    let game = GenericGame::new("game", "Game", None, "mods");
    game.deploy(staging.path(), target.path()).unwrap();

    let deployed = target.path().join("mod_file.txt");
    assert!(deployed.exists());
    assert!(
        deployed
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn test_generic_game_deploy_nested_directories() {
    let staging = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();

    // Create nested structure in staging
    let nested = staging.path().join("sub1/sub2");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("deep_file.dat"), b"deep data").unwrap();
    std::fs::write(staging.path().join("top_file.dat"), b"top data").unwrap();

    let game = GenericGame::new("game", "Game", None, "mods");
    game.deploy(staging.path(), target.path()).unwrap();

    // Top-level file should be symlinked
    let top = target.path().join("top_file.dat");
    assert!(top.exists());
    assert!(top.symlink_metadata().unwrap().file_type().is_symlink());

    // Nested directory should be created (not symlinked)
    let nested_target = target.path().join("sub1/sub2");
    assert!(nested_target.exists());
    assert!(nested_target.is_dir());

    // Deep file should be symlinked
    let deep = nested_target.join("deep_file.dat");
    assert!(deep.exists());
    assert!(deep.symlink_metadata().unwrap().file_type().is_symlink());
}

// ── GenericGame deploy with empty staging ────────────────────────────

#[test]
fn test_generic_game_deploy_empty_staging() {
    let staging = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();

    let game = GenericGame::new("game", "Game", None, "mods");
    // Should succeed with nothing to do
    game.deploy(staging.path(), target.path()).unwrap();
}

// ── GenericGame deploy creates target directory ─────────────────────

#[test]
fn test_generic_game_deploy_creates_target_dir() {
    let staging = tempfile::tempdir().unwrap();
    let target_base = tempfile::tempdir().unwrap();
    let target = target_base.path().join("new/nested/dir");

    std::fs::write(staging.path().join("file.txt"), b"data").unwrap();

    let game = GenericGame::new("game", "Game", None, "mods");
    game.deploy(staging.path(), &target).unwrap();

    assert!(target.exists());
    assert!(target.join("file.txt").exists());
}

// ── GenericGame deploy replaces existing symlinks ───────────────────

#[test]
fn test_generic_game_deploy_replaces_existing_symlinks() {
    let staging = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();

    std::fs::write(staging.path().join("file.txt"), b"data").unwrap();

    let game = GenericGame::new("game", "Game", None, "mods");

    // Deploy twice -- second should replace first
    game.deploy(staging.path(), target.path()).unwrap();
    game.deploy(staging.path(), target.path()).unwrap();

    assert!(target.path().join("file.txt").exists());
}

// ── GenericGame post_deploy succeeds ────────────────────────────────

#[test]
fn test_generic_game_post_deploy_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let game = GenericGame::new("game", "Game", None, "mods");
    // post_deploy is a no-op for GenericGame
    game.post_deploy(tmp.path()).unwrap();
}

#[test]
fn test_generic_game_post_deploy_with_nonexistent_path() {
    let game = GenericGame::new("game", "Game", None, "mods");
    // Even with a non-existent path, post_deploy should succeed (it's a no-op)
    game.post_deploy(std::path::Path::new("/nonexistent"))
        .unwrap();
}
