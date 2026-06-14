use super::*;
use xxhash_rust::xxh3::xxh3_64;

fn test_policy(min_bytes: u64) -> StagingCompressionPolicy {
    StagingCompressionPolicy {
        min_bytes,
        level: 1,
        suffix: COMPRESSED_SUFFIX.to_string(),
    }
}

#[tokio::test]
async fn staging_store_resolves_plain_and_compressed_logical_paths() {
    let temp = tempfile::tempdir().unwrap();
    let store = StagingStore::with_policy(temp.path(), test_policy(1));
    store.prepare_fresh().await.unwrap();
    let rel = "mods/Test/textures/large.dds";
    let logical = temp.path().join(rel);
    tokio::fs::create_dir_all(logical.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&logical, b"plain bytes").await.unwrap();
    assert!(store.logical_exists(rel).await);

    store.compress_eligible_files(1).await.unwrap();
    assert!(!logical.exists());
    assert!(compressed_path(&logical).exists());
    assert!(store.logical_exists(rel).await);
    let mut reader = store.open_logical_reader(rel).unwrap();
    let mut data = Vec::new();
    reader.read_to_end(&mut data).unwrap();
    assert_eq!(data, b"plain bytes");
}

#[tokio::test]
async fn zstd_round_trip_preserves_hash() {
    let temp = tempfile::tempdir().unwrap();
    let store = StagingStore::with_policy(temp.path(), test_policy(1));
    store.prepare_fresh().await.unwrap();
    let rel = "mods/Test/meshes/blob.bin";
    let logical = temp.path().join(rel);
    tokio::fs::create_dir_all(logical.parent().unwrap())
        .await
        .unwrap();
    let data = vec![42_u8; 256 * 1024];
    tokio::fs::write(&logical, &data).await.unwrap();
    store.compress_eligible_files(1).await.unwrap();
    assert_eq!(store.hash_logical_file(rel).await.unwrap(), xxh3_64(&data));
}

#[tokio::test]
async fn compression_policy_skips_metadata_plugins_configs_and_tiny_files() {
    let temp = tempfile::tempdir().unwrap();
    let store = StagingStore::with_policy(temp.path(), test_policy(8));
    store.prepare_fresh().await.unwrap();
    for rel in [
        "_state/state.bin",
        "mods/Test/meta.ini",
        "bsa_temp_abc/large.dds",
        "mods/Test/plugin.esp",
        "mods/Test/skse.dll",
        "mods/Test/config.ini",
        "mods/Test/tiny.bin",
    ] {
        let path = temp.path().join(rel);
        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&path, b"1234").await.unwrap();
    }
    let compressible = temp.path().join("mods/Test/texture.dds");
    tokio::fs::write(&compressible, b"123456789").await.unwrap();

    let summary = store.compress_eligible_files(1).await.unwrap();
    assert_eq!(summary.compressed_files, 1);
    assert!(compressed_path(&compressible).exists());
    assert!(temp.path().join("mods/Test/plugin.esp").exists());
    assert!(temp.path().join("mods/Test/skse.dll").exists());
    assert!(temp.path().join("mods/Test/config.ini").exists());
    assert!(temp.path().join("bsa_temp_abc/large.dds").exists());
}

#[tokio::test]
async fn fresh_layout_removes_incompatible_staging_but_keeps_compatible() {
    let temp = tempfile::tempdir().unwrap();
    let store = StagingStore::with_policy(temp.path(), test_policy(1));
    tokio::fs::create_dir_all(temp.path()).await.unwrap();
    tokio::fs::write(temp.path().join("old.txt"), b"old")
        .await
        .unwrap();
    store.prepare_fresh().await.unwrap();
    assert!(!temp.path().join("old.txt").exists());

    tokio::fs::write(temp.path().join("kept.txt"), b"kept")
        .await
        .unwrap();
    store.prepare_fresh().await.unwrap();
    assert!(temp.path().join("kept.txt").exists());
}

#[tokio::test]
async fn resumable_layout_adopts_incompatible_staging_without_deleting_files() {
    let temp = tempfile::tempdir().unwrap();
    let store = StagingStore::with_policy(temp.path(), test_policy(1));
    tokio::fs::create_dir_all(temp.path()).await.unwrap();
    tokio::fs::write(temp.path().join("old.txt"), b"old")
        .await
        .unwrap();

    assert_eq!(
        store.prepare_resumable().await.unwrap(),
        StagingPrepareStatus::Adopted
    );
    assert!(temp.path().join("old.txt").exists());
    assert!(store.has_compatible_layout().await);

    assert_eq!(
        store.prepare_resumable().await.unwrap(),
        StagingPrepareStatus::Compatible
    );

    assert_eq!(
        store.reset_and_prepare().await.unwrap(),
        StagingPrepareStatus::Reset
    );
    assert!(!temp.path().join("old.txt").exists());
}
