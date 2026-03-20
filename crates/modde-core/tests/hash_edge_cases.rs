use std::io::Write;
use std::path::Path;

use modde_core::hash::{hash_file_sha256, hash_file_xxhash, verify_sha256, verify_xxhash};
use tempfile::NamedTempFile;

fn create_temp_file(content: &[u8]) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content).unwrap();
    f
}

// ── xxHash edge cases ───────────────────────────────────────────────

#[tokio::test]
async fn test_xxhash_binary_content() {
    // Test with all byte values 0x00-0xFF
    let data: Vec<u8> = (0..=255u8).collect();
    let f = create_temp_file(&data);
    let hash = hash_file_xxhash(f.path()).await.unwrap();
    // Hash should be deterministic
    let hash2 = hash_file_xxhash(f.path()).await.unwrap();
    assert_eq!(hash, hash2);
    assert_ne!(hash, 0);
}

#[tokio::test]
async fn test_xxhash_single_byte() {
    let f = create_temp_file(&[0x42]);
    let hash = hash_file_xxhash(f.path()).await.unwrap();
    assert_ne!(hash, 0);
}

#[tokio::test]
async fn test_xxhash_single_null_byte() {
    let f = create_temp_file(&[0x00]);
    let hash = hash_file_xxhash(f.path()).await.unwrap();
    // Null byte should still produce a valid hash
    let empty = create_temp_file(b"");
    let empty_hash = hash_file_xxhash(empty.path()).await.unwrap();
    assert_ne!(hash, empty_hash);
}

#[tokio::test]
async fn test_xxhash_verify_immediately_after_hash() {
    let f = create_temp_file(b"verify me");
    let hash = hash_file_xxhash(f.path()).await.unwrap();
    // Verification should always pass immediately after hashing
    verify_xxhash(f.path(), hash).await.unwrap();
}

#[tokio::test]
async fn test_xxhash_verify_wrong_hash_error_message() {
    let f = create_temp_file(b"test content");
    let result = verify_xxhash(f.path(), 12345).await;
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("hash mismatch"), "expected hash mismatch error, got: {err}");
}

// ── SHA-256 edge cases ──────────────────────────────────────────────

#[tokio::test]
async fn test_sha256_binary_content() {
    let data: Vec<u8> = (0..=255u8).collect();
    let f = create_temp_file(&data);
    let hash = hash_file_sha256(f.path()).await.unwrap();
    assert_eq!(hash.len(), 64); // SHA-256 hex is 64 chars
}

#[tokio::test]
async fn test_sha256_large_file_streaming() {
    // 5MB file to test streaming reads across buffer boundaries
    let data = vec![0xCDu8; 5 * 1024 * 1024];
    let f = create_temp_file(&data);
    let hash = hash_file_sha256(f.path()).await.unwrap();
    assert_eq!(hash.len(), 64);
    // Verify consistency
    let hash2 = hash_file_sha256(f.path()).await.unwrap();
    assert_eq!(hash, hash2);
}

#[tokio::test]
async fn test_sha256_verify_immediately_after_hash() {
    let f = create_temp_file(b"sha256 test data");
    let hash = hash_file_sha256(f.path()).await.unwrap();
    verify_sha256(f.path(), &hash).await.unwrap();
}

#[tokio::test]
async fn test_sha256_verify_wrong_hash_error_message() {
    let f = create_temp_file(b"test");
    let result = verify_sha256(f.path(), "0000000000000000000000000000000000000000000000000000000000000000").await;
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("hash mismatch"), "expected hash mismatch, got: {err}");
}

#[tokio::test]
async fn test_sha256_case_sensitive_hex() {
    let f = create_temp_file(b"case test");
    let hash = hash_file_sha256(f.path()).await.unwrap();
    // Our implementation returns lowercase hex
    assert_eq!(hash, hash.to_lowercase());
}

// ── Cross-algorithm tests ───────────────────────────────────────────

#[tokio::test]
async fn test_different_algorithms_different_hashes_same_content() {
    let f = create_temp_file(b"hello algorithms");
    let xxhash = hash_file_xxhash(f.path()).await.unwrap();
    let sha256 = hash_file_sha256(f.path()).await.unwrap();
    // They're different types so we just verify both succeed
    assert_ne!(xxhash, 0);
    assert_eq!(sha256.len(), 64);
}

#[tokio::test]
async fn test_both_algorithms_fail_on_nonexistent() {
    let bad_path = Path::new("/tmp/nonexistent_modde_hash_test_file");
    assert!(hash_file_xxhash(bad_path).await.is_err());
    assert!(hash_file_sha256(bad_path).await.is_err());
}

#[tokio::test]
async fn test_both_algorithms_handle_just_newline() {
    let f = create_temp_file(b"\n");
    let xxhash = hash_file_xxhash(f.path()).await.unwrap();
    let sha256 = hash_file_sha256(f.path()).await.unwrap();
    assert_ne!(xxhash, 0);
    assert_eq!(sha256.len(), 64);
}

// ── Verify mismatch details ─────────────────────────────────────────

#[tokio::test]
async fn test_xxhash_mismatch_includes_path() {
    let f = create_temp_file(b"mismatch test");
    let result = verify_xxhash(f.path(), 0).await;
    let err = format!("{}", result.unwrap_err());
    // Error should mention the file path
    assert!(err.contains(f.path().to_str().unwrap()), "error should contain path: {err}");
}

#[tokio::test]
async fn test_sha256_mismatch_includes_path() {
    let f = create_temp_file(b"mismatch sha256");
    let result = verify_sha256(f.path(), "bad_hash").await;
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains(f.path().to_str().unwrap()), "error should contain path: {err}");
}
