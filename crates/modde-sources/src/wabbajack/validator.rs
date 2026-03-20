use std::path::Path;

use anyhow::Result;
use tracing::{info, warn};
use xxhash_rust::xxh3::xxh3_64;

use modde_core::manifest::wabbajack::WabbajackManifest;

/// Result of post-install verification.
#[derive(Debug)]
pub struct ValidationReport {
    pub total_files: usize,
    pub verified: usize,
    pub missing: Vec<String>,
    pub mismatches: Vec<ValidationMismatch>,
}

#[derive(Debug)]
pub struct ValidationMismatch {
    pub path: String,
    pub expected_hash: u64,
    pub actual_hash: u64,
}

/// Post-install verification: re-hash every installed file against the manifest.
///
/// Walks the expected files from the manifest's install directives, checks each
/// file exists in the staging directory, and verifies its xxHash matches.
pub async fn validate_install(
    manifest: &WabbajackManifest,
    staging_dir: &Path,
) -> Result<ValidationReport> {
    // Build expected file map from install directives and archives
    let expected_files = collect_expected_files(manifest);

    let total_files = expected_files.len();
    let mut verified = 0usize;
    let mut missing = Vec::new();
    let mut mismatches = Vec::new();

    for (rel_path, expected_hash) in &expected_files {
        let full_path = staging_dir.join(rel_path);

        if !full_path.exists() {
            warn!(path = %rel_path, "expected file missing from staging directory");
            missing.push(rel_path.clone());
            continue;
        }

        let data = tokio::fs::read(&full_path).await?;
        let actual_hash = xxh3_64(&data);

        if actual_hash != *expected_hash {
            warn!(
                path = %rel_path,
                expected = format!("{expected_hash:016x}"),
                actual = format!("{actual_hash:016x}"),
                "hash mismatch"
            );
            mismatches.push(ValidationMismatch {
                path: rel_path.clone(),
                expected_hash: *expected_hash,
                actual_hash,
            });
        } else {
            verified += 1;
        }
    }

    info!(
        total_files,
        verified,
        missing = missing.len(),
        mismatches = mismatches.len(),
        "post-install validation complete"
    );

    Ok(ValidationReport {
        total_files,
        verified,
        missing,
        mismatches,
    })
}

/// Collect expected (relative_path, hash) pairs from the manifest.
///
/// Uses archive entries as the source of truth for expected hashes. Install
/// directives reference archives by hash, so we build a lookup from archive
/// hash -> archive entry, then map install directive targets to expected hashes.
fn collect_expected_files(manifest: &WabbajackManifest) -> Vec<(String, u64)> {
    use modde_core::manifest::wabbajack::RawDirective;

    let mut files = Vec::new();

    // Build archive hash -> archive entry lookup
    let archive_map: std::collections::HashMap<u64, &modde_core::manifest::wabbajack::ArchiveEntry> =
        manifest.archives.iter().map(|a| (a.hash, a)).collect();

    for directive in &manifest.directives {
        match directive {
            RawDirective::FromArchive {
                archive_hash_path,
                to,
            } => {
                // The archive hash is the first element of archive_hash_path
                let archive_hash = archive_hash_path
                    .first()
                    .and_then(|v| v.as_str())
                    .and_then(modde_core::manifest::wabbajack::parse_b64_hash)
                    .or_else(|| archive_hash_path.first().and_then(|v| v.as_u64()))
                    .unwrap_or(0);

                // Use the archive's hash as a proxy for the expected output hash
                // In a real implementation, individual file hashes would be tracked
                if let Some(archive) = archive_map.get(&archive_hash) {
                    files.push((to.clone(), archive.hash));
                }
            }
            RawDirective::PatchedFromArchive {
                to, hash, ..
            } => {
                // The hash field on PatchedFromArchive is the expected output hash
                files.push((to.clone(), *hash));
            }
            RawDirective::InlineFile { to, hash, .. } => {
                files.push((to.clone(), *hash));
            }
            RawDirective::CreateBSA { .. } | RawDirective::Unknown => {}
        }
    }

    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use modde_core::manifest::wabbajack::WabbajackManifest;

    fn empty_manifest() -> WabbajackManifest {
        WabbajackManifest {
            name: "test".to_string(),
            author: "test".to_string(),
            description: "test".to_string(),
            game: "skyrimse".to_string(),
            version: "1.0".to_string(),
            archives: vec![],
            directives: vec![],
        }
    }

    #[tokio::test]
    async fn test_validate_empty_manifest() {
        let staging = tempfile::tempdir().unwrap();
        let manifest = empty_manifest();
        let report = validate_install(&manifest, staging.path()).await.unwrap();
        assert_eq!(report.total_files, 0);
        assert_eq!(report.verified, 0);
        assert!(report.missing.is_empty());
        assert!(report.mismatches.is_empty());
    }

    #[tokio::test]
    async fn test_validate_missing_file() {
        let staging = tempfile::tempdir().unwrap();
        let manifest = WabbajackManifest {
            name: "test".to_string(),
            author: "test".to_string(),
            description: "test".to_string(),
            game: "skyrimse".to_string(),
            version: "1.0".to_string(),
            archives: vec![modde_core::manifest::wabbajack::ArchiveEntry {
                hash: 12345,
                name: "test.zip".to_string(),
                size: 100,
                state: None,
            }],
            directives: vec![modde_core::manifest::wabbajack::RawDirective::FromArchive {
                archive_hash_path: vec![
                    serde_json::Value::Number(12345.into()),
                    serde_json::Value::String("inner.txt".to_string()),
                ],
                to: "output/inner.txt".to_string(),
            }],
        };

        let report = validate_install(&manifest, staging.path()).await.unwrap();
        assert_eq!(report.total_files, 1);
        assert_eq!(report.missing.len(), 1);
        assert_eq!(report.missing[0], "output/inner.txt");
    }

    #[tokio::test]
    async fn test_validate_hash_mismatch() {
        let staging = tempfile::tempdir().unwrap();
        let file_path = staging.path().join("test.txt");
        tokio::fs::write(&file_path, b"wrong content").await.unwrap();

        let manifest = WabbajackManifest {
            name: "test".to_string(),
            author: "test".to_string(),
            description: "test".to_string(),
            game: "skyrimse".to_string(),
            version: "1.0".to_string(),
            archives: vec![],
            directives: vec![
                modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                    archive_hash_path: vec![serde_json::Value::Number(0.into())],
                    patch_id: String::new(),
                    to: "test.txt".to_string(),
                    hash: 99999, // wrong hash
                },
            ],
        };

        let report = validate_install(&manifest, staging.path()).await.unwrap();
        assert_eq!(report.total_files, 1);
        assert_eq!(report.mismatches.len(), 1);
    }

    #[tokio::test]
    async fn test_validate_correct_file() {
        let staging = tempfile::tempdir().unwrap();
        let content = b"correct file content";
        let file_path = staging.path().join("test.txt");
        tokio::fs::write(&file_path, content).await.unwrap();

        let expected_hash = xxh3_64(content);

        let manifest = WabbajackManifest {
            name: "test".to_string(),
            author: "test".to_string(),
            description: "test".to_string(),
            game: "skyrimse".to_string(),
            version: "1.0".to_string(),
            archives: vec![],
            directives: vec![
                modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                    archive_hash_path: vec![serde_json::Value::Number(0.into())],
                    patch_id: String::new(),
                    to: "test.txt".to_string(),
                    hash: expected_hash,
                },
            ],
        };

        let report = validate_install(&manifest, staging.path()).await.unwrap();
        assert_eq!(report.total_files, 1);
        assert_eq!(report.verified, 1);
        assert!(report.missing.is_empty());
        assert!(report.mismatches.is_empty());
    }

    #[tokio::test]
    async fn test_validate_multiple_missing() {
        let staging = tempfile::tempdir().unwrap();

        let manifest = WabbajackManifest {
            name: "test".to_string(),
            author: "test".to_string(),
            description: "test".to_string(),
            game: "skyrimse".to_string(),
            version: "1.0".to_string(),
            archives: vec![],
            directives: vec![
                modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                    archive_hash_path: vec![serde_json::Value::Number(0.into())],
                    patch_id: String::new(),
                    to: "file_a.txt".to_string(),
                    hash: 111,
                },
                modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                    archive_hash_path: vec![serde_json::Value::Number(0.into())],
                    patch_id: String::new(),
                    to: "file_b.txt".to_string(),
                    hash: 222,
                },
                modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                    archive_hash_path: vec![serde_json::Value::Number(0.into())],
                    patch_id: String::new(),
                    to: "file_c.txt".to_string(),
                    hash: 333,
                },
            ],
        };

        let report = validate_install(&manifest, staging.path()).await.unwrap();
        assert_eq!(report.total_files, 3);
        assert_eq!(report.verified, 0);
        assert_eq!(report.missing.len(), 3);
        assert!(report.mismatches.is_empty());
    }

    #[tokio::test]
    async fn test_validate_mixed_results() {
        let staging = tempfile::tempdir().unwrap();

        // File with correct hash
        let correct_content = b"correct data";
        let correct_hash = xxh3_64(correct_content);
        tokio::fs::write(staging.path().join("correct.txt"), correct_content)
            .await
            .unwrap();

        // File with wrong hash
        tokio::fs::write(staging.path().join("wrong.txt"), b"actual data")
            .await
            .unwrap();

        // missing.txt is not created

        let manifest = WabbajackManifest {
            name: "test".to_string(),
            author: "test".to_string(),
            description: "test".to_string(),
            game: "skyrimse".to_string(),
            version: "1.0".to_string(),
            archives: vec![],
            directives: vec![
                modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                    archive_hash_path: vec![serde_json::Value::Number(0.into())],
                    patch_id: String::new(),
                    to: "correct.txt".to_string(),
                    hash: correct_hash,
                },
                modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                    archive_hash_path: vec![serde_json::Value::Number(0.into())],
                    patch_id: String::new(),
                    to: "wrong.txt".to_string(),
                    hash: 99999,
                },
                modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                    archive_hash_path: vec![serde_json::Value::Number(0.into())],
                    patch_id: String::new(),
                    to: "missing.txt".to_string(),
                    hash: 55555,
                },
            ],
        };

        let report = validate_install(&manifest, staging.path()).await.unwrap();
        assert_eq!(report.total_files, 3);
        assert_eq!(report.verified, 1);
        assert_eq!(report.missing.len(), 1);
        assert_eq!(report.missing[0], "missing.txt");
        assert_eq!(report.mismatches.len(), 1);
        assert_eq!(report.mismatches[0].path, "wrong.txt");
    }

    #[tokio::test]
    async fn test_validate_patched_directive_uses_hash_field() {
        let staging = tempfile::tempdir().unwrap();
        let content = b"patched output data";
        let patched_hash = xxh3_64(content);
        tokio::fs::write(staging.path().join("patched.esp"), content)
            .await
            .unwrap();

        let manifest = WabbajackManifest {
            name: "test".to_string(),
            author: "test".to_string(),
            description: "test".to_string(),
            game: "skyrimse".to_string(),
            version: "1.0".to_string(),
            archives: vec![modde_core::manifest::wabbajack::ArchiveEntry {
                hash: 77777,
                name: "source.zip".to_string(),
                size: 200,
                state: None,
            }],
            directives: vec![
                modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                    archive_hash_path: vec![serde_json::Value::Number(77777.into())],
                    patch_id: String::new(),
                    to: "patched.esp".to_string(),
                    hash: patched_hash, // uses the directive's own hash, not archive hash
                },
            ],
        };

        let report = validate_install(&manifest, staging.path()).await.unwrap();
        assert_eq!(report.total_files, 1);
        assert_eq!(report.verified, 1);
        assert!(report.mismatches.is_empty());
    }

    #[tokio::test]
    async fn test_validate_create_bsa_ignored() {
        let staging = tempfile::tempdir().unwrap();

        let manifest = WabbajackManifest {
            name: "test".to_string(),
            author: "test".to_string(),
            description: "test".to_string(),
            game: "skyrimse".to_string(),
            version: "1.0".to_string(),
            archives: vec![],
            directives: vec![
                modde_core::manifest::wabbajack::RawDirective::CreateBSA {
                    temp_id: "bsa_temp_001".to_string(),
                    to: "output.bsa".to_string(),
                    file_states: vec![],
                },
            ],
        };

        let expected = collect_expected_files(&manifest);
        assert!(expected.is_empty(), "CreateBSA should not produce expected files");

        let report = validate_install(&manifest, staging.path()).await.unwrap();
        assert_eq!(report.total_files, 0);
    }

    #[tokio::test]
    async fn test_validate_unknown_directive_ignored() {
        let staging = tempfile::tempdir().unwrap();

        let manifest = WabbajackManifest {
            name: "test".to_string(),
            author: "test".to_string(),
            description: "test".to_string(),
            game: "skyrimse".to_string(),
            version: "1.0".to_string(),
            archives: vec![],
            directives: vec![
                modde_core::manifest::wabbajack::RawDirective::Unknown,
            ],
        };

        let expected = collect_expected_files(&manifest);
        assert!(expected.is_empty(), "Unknown directives should be filtered out");

        let report = validate_install(&manifest, staging.path()).await.unwrap();
        assert_eq!(report.total_files, 0);
    }

    #[tokio::test]
    async fn test_validate_nested_file_path() {
        let staging = tempfile::tempdir().unwrap();
        let subdir = staging.path().join("subdir");
        tokio::fs::create_dir_all(&subdir).await.unwrap();

        let content = b"nested file content";
        let expected_hash = xxh3_64(content);
        tokio::fs::write(subdir.join("test.txt"), content)
            .await
            .unwrap();

        let manifest = WabbajackManifest {
            name: "test".to_string(),
            author: "test".to_string(),
            description: "test".to_string(),
            game: "skyrimse".to_string(),
            version: "1.0".to_string(),
            archives: vec![],
            directives: vec![
                modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                    archive_hash_path: vec![serde_json::Value::Number(0.into())],
                    patch_id: String::new(),
                    to: "subdir/test.txt".to_string(),
                    hash: expected_hash,
                },
            ],
        };

        let report = validate_install(&manifest, staging.path()).await.unwrap();
        assert_eq!(report.total_files, 1);
        assert_eq!(report.verified, 1);
        assert!(report.missing.is_empty());
        assert!(report.mismatches.is_empty());
    }
}
