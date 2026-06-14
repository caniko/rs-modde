use super::manager::{
    MODDE_LIVE_STATE_DIR, MODDE_PROFILE_PARK_DIR, STEAM_CLOUD_MARKER, clear_active_save_dir,
    park_active_saves,
};

#[test]
fn park_active_saves_preserves_steam_cloud_marker() {
    let tmp = tempfile::tempdir().unwrap();
    let save_dir = tmp.path();
    std::fs::write(save_dir.join("Save1.ess"), b"save").unwrap();
    std::fs::write(save_dir.join(STEAM_CLOUD_MARKER), b"marker").unwrap();

    park_active_saves(save_dir, "vanilla profile").unwrap();

    assert!(!save_dir.join("Save1.ess").exists());
    assert_eq!(
        std::fs::read(save_dir.join(STEAM_CLOUD_MARKER)).unwrap(),
        b"marker"
    );
    assert!(
        save_dir
            .join(MODDE_LIVE_STATE_DIR)
            .join(MODDE_PROFILE_PARK_DIR)
            .join("vanilla-profile")
            .join("Save1.ess")
            .exists()
    );
}

#[test]
fn clear_active_save_dir_keeps_cloud_and_modde_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let save_dir = tmp.path();
    let modde_dir = save_dir.join(MODDE_LIVE_STATE_DIR);
    std::fs::create_dir_all(&modde_dir).unwrap();
    std::fs::write(save_dir.join("Save1.ess"), b"save").unwrap();
    std::fs::write(save_dir.join(STEAM_CLOUD_MARKER), b"marker").unwrap();
    std::fs::write(modde_dir.join("state"), b"state").unwrap();

    clear_active_save_dir(save_dir).unwrap();

    assert!(!save_dir.join("Save1.ess").exists());
    assert!(save_dir.join(STEAM_CLOUD_MARKER).exists());
    assert!(modde_dir.join("state").exists());
}
