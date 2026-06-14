use super::*;
use crate::wabbajack::staging::{StagingCompressionPolicy, StagingStore, compressed_path};

#[tokio::test]
async fn deploy_decodes_compressed_files_and_links_plain_files() {
    let temp = tempfile::tempdir().unwrap();
    let staging = temp.path().join("staging");
    let game = temp.path().join("game");
    let store = StagingStore::with_policy(
        &staging,
        StagingCompressionPolicy {
            min_bytes: 1,
            level: 1,
            suffix: ".modde-zst".to_string(),
        },
    );
    store.prepare_fresh().await.unwrap();

    let compressed_rel = "mods/TextureMod/textures/landscape/snow.dds";
    let plain_rel = "mods/PluginMod/plugin.esp";
    let compressed_path_plain = staging.join(compressed_rel);
    let plain_path = staging.join(plain_rel);
    tokio::fs::create_dir_all(compressed_path_plain.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::create_dir_all(plain_path.parent().unwrap())
        .await
        .unwrap();
    let compressed_bytes = vec![3_u8; 128 * 1024];
    tokio::fs::write(&compressed_path_plain, &compressed_bytes)
        .await
        .unwrap();
    tokio::fs::write(&plain_path, b"plugin").await.unwrap();
    store.compress_eligible_files(1).await.unwrap();

    assert!(compressed_path(&compressed_path_plain).exists());
    assert!(!compressed_path_plain.exists());

    deploy_mo2_to_game(&staging, &game, false).await.unwrap();

    assert_eq!(
        tokio::fs::read(game.join("textures/landscape/snow.dds"))
            .await
            .unwrap(),
        compressed_bytes
    );
    assert_eq!(
        tokio::fs::read(game.join("plugin.esp")).await.unwrap(),
        b"plugin"
    );
}
