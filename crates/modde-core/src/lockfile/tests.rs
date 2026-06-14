use super::*;
use crate::profile::ProfileSource;

fn minimal_payload() -> LockPayload {
    LockPayload {
        modde_version: "test".to_string(),
        generated_at: "2026-01-01T00:00:00Z".to_string(),
        reproducible: true,
        incomplete_reasons: Vec::new(),
        profile: LockProfile {
            name: "main".to_string(),
            game_id: "skyrim-se".to_string(),
            source: ProfileSource::Manual,
            load_order_lock: None,
            mod_order: vec!["mod-a".to_string()],
        },
        mods: vec![LockedMod {
            order: 0,
            mod_id: "mod-a".to_string(),
            display_name: None,
            enabled: true,
            version: None,
            nexus: Some(NexusProvenance {
                game_domain: "skyrimspecialedition".to_string(),
                mod_id: 1,
                file_id: 2,
            }),
            source_archive_hash: Some("abc".to_string()),
            install_method: None,
            fomod_config: None,
            files: Vec::new(),
        }],
        plugin_order: Vec::new(),
        hidden_files: Vec::new(),
        patchers: Vec::new(),
        tool_outputs: Vec::new(),
        wabbajack_manifest: None,
    }
}

#[test]
fn rejects_path_traversal() {
    let mut payload = minimal_payload();
    payload.mods[0].files.push(LockedFile {
        rel_path: "../evil".to_string(),
        origin_rel_path: "evil".to_string(),
        size: 0,
        sha256: String::new(),
        xxh64: String::new(),
        xxh3: String::new(),
        merge_group: None,
    });

    assert!(validate_payload(&payload).is_err());
}

#[test]
fn rejects_duplicate_mod_ids() {
    let mut payload = minimal_payload();
    payload.mods.push(payload.mods[0].clone());

    assert!(validate_payload(&payload).is_err());
}

#[test]
fn sign_and_verify_roundtrip() {
    let (secret, _) = generate_keypair().unwrap();
    let signing_key = signing_key_from_secret_file(&to_pretty_json(&secret).unwrap()).unwrap();
    let mut lock = ModdeLock {
        kind: LOCK_KIND.to_string(),
        format_version: LOCK_FORMAT_VERSION,
        payload: minimal_payload(),
        signatures: Vec::new(),
    };

    sign_lock(&mut lock, &signing_key).unwrap();
    verify_signatures(&lock).unwrap();

    lock.payload.profile.name = "tampered".to_string();
    assert!(verify_signatures(&lock).is_err());
}

#[tokio::test]
async fn verify_locked_file_accepts_untouched_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file.bin");
    tokio::fs::write(&path, b"locked file contents")
        .await
        .unwrap();
    let locked = lock_file_at(&path, "file.bin", "file.bin", None)
        .await
        .unwrap();

    verify_locked_file(&path, &locked).await.unwrap();
}

#[tokio::test]
async fn verify_locked_file_rejects_corrupted_xxh3_digest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file.bin");
    tokio::fs::write(&path, b"locked file contents")
        .await
        .unwrap();
    let locked = lock_file_at(&path, "file.bin", "file.bin", None)
        .await
        .unwrap();

    // Size, sha256, and xxh64 stay correct; only the xxh3 digest is wrong.
    let mut corrupted = locked.clone();
    let flipped = u64::from_str_radix(&locked.xxh3, 16).unwrap() ^ 1;
    corrupted.xxh3 = format!("{flipped:016x}");

    let error = verify_locked_file(&path, &corrupted).await.unwrap_err();
    match error {
        CoreError::HashMismatch {
            expected, actual, ..
        } => {
            assert_eq!(expected, corrupted.xxh3, "the xxh3 check must have failed");
            assert_eq!(actual, locked.xxh3);
        }
        other => panic!("expected hash mismatch, got: {other}"),
    }
    let message = verify_locked_file(&path, &corrupted)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("hash mismatch"),
        "error should report a hash mismatch: {message}"
    );
}

#[tokio::test]
async fn verify_locked_file_checks_xxh_before_sha256() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file.bin");
    tokio::fs::write(&path, b"locked file contents")
        .await
        .unwrap();
    let locked = lock_file_at(&path, "file.bin", "file.bin", None)
        .await
        .unwrap();

    // Same-size corruption invalidates every digest; the xxh64 check
    // must trip first, so the reported expected digest is the 16-char
    // xxh64 value rather than the 64-char sha256.
    tokio::fs::write(&path, b"LOCKED FILE CONTENTS")
        .await
        .unwrap();
    let error = verify_locked_file(&path, &locked).await.unwrap_err();
    match error {
        CoreError::HashMismatch { expected, .. } => {
            assert_eq!(
                expected, locked.xxh64,
                "xxh64 must be verified before sha256"
            );
        }
        other => panic!("expected hash mismatch, got: {other}"),
    }
}

#[test]
fn rejects_future_lock_versions() {
    let lock = ModdeLock {
        kind: LOCK_KIND.to_string(),
        format_version: LOCK_FORMAT_VERSION + 1,
        payload: minimal_payload(),
        signatures: Vec::new(),
    };

    assert!(validate_lock(&lock).is_err());
}
