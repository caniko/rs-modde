use std::path::Path;

use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use xxhash_rust::xxh3::xxh3_64;
use xxhash_rust::xxh64::xxh64;

use crate::error::Result;
use crate::CoreError;

const BUF_SIZE: usize = 64 * 1024;

/// Return a `HashMismatch` error with hex-formatted u64 hashes.
fn hash_mismatch(path: &Path, expected: u64, actual: u64) -> CoreError {
    CoreError::HashMismatch {
        path: path.to_path_buf(),
        expected: format!("{expected:016x}"),
        actual: format!("{actual:016x}"),
    }
}

/// Compute xxHash (XXH3-64) of a file.
pub async fn hash_file_xxhash(path: &Path) -> Result<u64> {
    let data = tokio::fs::read(path).await?;
    Ok(xxh3_64(&data))
}

/// Verify a file's XXH3-64 hash matches the expected value.
pub async fn verify_xxhash(path: &Path, expected: u64) -> Result<()> {
    let actual = hash_file_xxhash(path).await?;
    if actual != expected {
        return Err(hash_mismatch(path, expected, actual));
    }
    Ok(())
}

/// Verify a file's classic xxHash64 matches the expected value (used by Wabbajack).
pub async fn verify_xxh64(path: &Path, expected: u64) -> Result<()> {
    let data = tokio::fs::read(path).await?;
    let actual = xxh64(&data, 0);
    if actual != expected {
        return Err(hash_mismatch(path, expected, actual));
    }
    Ok(())
}

/// Verify a file's hash using Wabbajack-compatible strategy: try xxHash64 first, fall back to XXH3.
///
/// This is the canonical entry point for download verification where the hash algorithm is ambiguous.
pub async fn verify_xxhash_compat(path: &Path, expected: u64) -> Result<u64> {
    let data = tokio::fs::read(path).await?;
    let h64 = xxh64(&data, 0);
    if h64 == expected || xxh3_64(&data) == expected {
        return Ok(expected);
    }
    Err(hash_mismatch(path, expected, h64))
}

/// Compute SHA-256 of a file using streaming reads.
pub async fn hash_file_sha256(path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; BUF_SIZE];

    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Verify a file's SHA-256 matches the expected hex string.
pub async fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let actual = hash_file_sha256(path).await?;
    if actual != expected {
        return Err(CoreError::HashMismatch {
            path: path.to_path_buf(),
            expected: expected.to_string(),
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_file(content: &[u8]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content).unwrap();
        f
    }

    #[tokio::test]
    async fn test_xxhash_roundtrip() {
        let f = create_temp_file(b"hello world");
        let hash = hash_file_xxhash(f.path()).await.unwrap();
        verify_xxhash(f.path(), hash).await.unwrap();
    }

    #[tokio::test]
    async fn test_xxhash_mismatch() {
        let f = create_temp_file(b"hello world");
        let result = verify_xxhash(f.path(), 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_sha256_known_value() {
        let f = create_temp_file(b"hello world");
        let hash = hash_file_sha256(f.path()).await.unwrap();
        // SHA-256 of "hello world"
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[tokio::test]
    async fn test_sha256_verify() {
        let f = create_temp_file(b"test data");
        let hash = hash_file_sha256(f.path()).await.unwrap();
        verify_sha256(f.path(), &hash).await.unwrap();
    }

    #[tokio::test]
    async fn test_sha256_mismatch() {
        let f = create_temp_file(b"test data");
        let result = verify_sha256(f.path(), "0000").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_xxhash_empty_file() {
        let f = create_temp_file(b"");
        let hash = hash_file_xxhash(f.path()).await.unwrap();
        let expected = xxh3_64(b"");
        assert_eq!(hash, expected);
    }

    #[tokio::test]
    async fn test_xxhash_large_file() {
        let data = vec![0xABu8; 1024 * 1024]; // 1MB
        let f = create_temp_file(&data);
        let hash = hash_file_xxhash(f.path()).await.unwrap();
        let expected = xxh3_64(&data);
        assert_eq!(hash, expected);
    }

    #[tokio::test]
    async fn test_sha256_empty_file() {
        let f = create_temp_file(b"");
        let hash = hash_file_sha256(f.path()).await.unwrap();
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[tokio::test]
    async fn test_xxhash_nonexistent_file() {
        let result = hash_file_xxhash(Path::new("/tmp/nonexistent_file_xxhash_test")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_sha256_nonexistent_file() {
        let result = hash_file_sha256(Path::new("/tmp/nonexistent_file_sha256_test")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_xxhash_different_content_different_hash() {
        let f1 = create_temp_file(b"content alpha");
        let f2 = create_temp_file(b"content beta");
        let h1 = hash_file_xxhash(f1.path()).await.unwrap();
        let h2 = hash_file_xxhash(f2.path()).await.unwrap();
        assert_ne!(h1, h2);
    }

    #[tokio::test]
    async fn test_xxhash_same_content_same_hash() {
        let f1 = create_temp_file(b"identical content");
        let f2 = create_temp_file(b"identical content");
        let h1 = hash_file_xxhash(f1.path()).await.unwrap();
        let h2 = hash_file_xxhash(f2.path()).await.unwrap();
        assert_eq!(h1, h2);
    }
}
