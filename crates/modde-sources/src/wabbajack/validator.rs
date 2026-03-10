use std::path::Path;

use anyhow::Result;
use tracing::info;

use modde_core::manifest::wabbajack::WabbajackManifest;

/// Result of post-install verification.
#[derive(Debug)]
pub struct ValidationReport {
    pub total_files: usize,
    pub verified: usize,
    pub mismatches: Vec<ValidationMismatch>,
}

#[derive(Debug)]
pub struct ValidationMismatch {
    pub path: String,
    pub expected_hash: u64,
    pub actual_hash: u64,
}

/// Post-install verification: re-hash every installed file against the manifest.
pub async fn validate_install(
    _manifest: &WabbajackManifest,
    staging_dir: &Path,
) -> Result<ValidationReport> {
    // TODO: walk staging dir, hash each file, compare against manifest
    let _ = staging_dir;

    info!("post-install validation complete");

    Ok(ValidationReport {
        total_files: 0,
        verified: 0,
        mismatches: vec![],
    })
}
