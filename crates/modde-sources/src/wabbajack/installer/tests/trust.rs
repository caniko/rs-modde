use super::*;

#[tokio::test]
async fn archive_trust_sidecar_skips_second_hash_pass() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let staging = dir.path().join("staging");
    tokio::fs::create_dir_all(&store).await.unwrap();
    let bytes = b"trusted archive bytes";
    let hash = xxh64(bytes, 0);
    let path = archive_path(&store, &hash);
    tokio::fs::write(&path, bytes).await.unwrap();

    let inst = WabbajackInstaller::new(
        minimal_manifest(),
        dir.path().join("list.wabbajack"),
        store,
        staging,
    );
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    let (_archive, first) = inst.ensure_archive_trusted(hash, &tx).await.unwrap();
    assert_eq!(first.streamed_hash_bytes, bytes.len() as u64);
    assert!(!first.sidecar_hit);
    assert!(verified_sidecar_path(&path).exists());

    let (_archive, second) = inst.ensure_archive_trusted(hash, &tx).await.unwrap();
    assert!(second.sidecar_hit);
    assert_eq!(second.streamed_hash_bytes, 0);
}

#[tokio::test]
async fn stale_archive_trust_sidecar_forces_rehash() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let staging = dir.path().join("staging");
    tokio::fs::create_dir_all(&store).await.unwrap();
    let bytes = b"trusted archive bytes";
    let hash = xxh64(bytes, 0);
    let path = archive_path(&store, &hash);
    tokio::fs::write(&path, bytes).await.unwrap();

    let inst = WabbajackInstaller::new(
        minimal_manifest(),
        dir.path().join("list.wabbajack"),
        store,
        staging,
    );
    inst.write_verified_sidecar(&path, hash).await.unwrap();
    let mut sidecar: serde_json::Value =
        serde_json::from_slice(&tokio::fs::read(verified_sidecar_path(&path)).await.unwrap())
            .unwrap();
    sidecar["size_bytes"] = serde_json::json!(1);
    tokio::fs::write(
        verified_sidecar_path(&path),
        serde_json::to_vec_pretty(&sidecar).unwrap(),
    )
    .await
    .unwrap();

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let (_archive, stats) = inst.ensure_archive_trusted(hash, &tx).await.unwrap();
    assert!(!stats.sidecar_hit);
    assert_eq!(stats.streamed_hash_bytes, bytes.len() as u64);
}
