//! Post-install verification for Wabbajack modlists: re-hashes staged files
//! against the expected output hashes carried by a [`WabbajackManifest`]'s
//! directives, plus a cheap existence-only preflight check.

use std::path::Path;

use anyhow::Result;
use tracing::{info, warn};

use modde_core::manifest::wabbajack::WabbajackManifest;

use super::staging::StagingStore;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExpectedFile {
    pub path: String,
    pub expected_hash: Option<u64>,
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
    let staging = StagingStore::new(staging_dir);

    for expected in &expected_files {
        if !staging.logical_exists(&expected.path).await {
            warn!(path = %expected.path, "expected file missing from staging directory");
            missing.push(expected.path.clone());
            continue;
        }

        let Some(expected_hash) = expected.expected_hash else {
            verified += 1;
            continue;
        };

        let (actual_xxh64, actual_xxh3) = staging.hash_logical_file_compat(&expected.path).await?;

        if actual_xxh64 == expected_hash || actual_xxh3 == expected_hash {
            verified += 1;
        } else {
            warn!(
                path = %expected.path,
                expected = format!("{expected_hash:016x}"),
                actual_xxh64 = format!("{actual_xxh64:016x}"),
                actual_xxh3 = format!("{actual_xxh3:016x}"),
                "hash mismatch"
            );
            mismatches.push(ValidationMismatch {
                path: expected.path.clone(),
                expected_hash,
                actual_hash: actual_xxh3,
            });
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

/// Cheap pre-install check: returns `true` when every file the manifest
/// expects already exists in the staging directory.  Only checks existence
/// (no hashing), so it is fast enough to run unconditionally before the
/// install pipeline.
pub async fn preflight_staging(manifest: &WabbajackManifest, staging_dir: &Path) -> bool {
    let expected = collect_expected_files(manifest);
    if expected.is_empty() {
        return false;
    }
    let staging = StagingStore::new(staging_dir);
    for expected in &expected {
        if !staging.logical_exists(&expected.path).await {
            return false;
        }
    }
    true
}

/// Collect expected logical staging files from the manifest.
///
/// `InlineFile`, `RemappedInlineFile`, and `PatchedFromArchive` carry expected
/// output hashes in the manifest. `FromArchive` directives do not; for those we
/// can validate existence only without inventing a false source-hash check.
pub(crate) fn collect_expected_files(manifest: &WabbajackManifest) -> Vec<ExpectedFile> {
    use modde_core::manifest::wabbajack::RawDirective;

    let mut files = Vec::new();

    for directive in &manifest.directives {
        match directive {
            RawDirective::FromArchive { to, .. } => files.push(ExpectedFile {
                path: to.clone(),
                expected_hash: None,
            }),
            RawDirective::PatchedFromArchive { to, hash, .. } => {
                files.push(ExpectedFile {
                    path: to.clone(),
                    expected_hash: Some(*hash),
                });
            }
            RawDirective::InlineFile { to, hash, .. }
            | RawDirective::RemappedInlineFile { to, hash, .. } => {
                files.push(ExpectedFile {
                    path: to.clone(),
                    expected_hash: Some(*hash),
                });
            }
            RawDirective::CreateBSA { .. } | RawDirective::Unknown => {}
        }
    }

    files
}

#[cfg(test)]
mod tests;
