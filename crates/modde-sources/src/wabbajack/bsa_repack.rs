use std::path::Path;

use anyhow::Result;

use modde_core::manifest::wabbajack::BSAFileState;

/// Reconstruct a BSA/BA2 archive from CreateBSA directive file states.
///
/// If `bsab` tool is available on PATH, shells out to it. Otherwise uses a
/// minimal built-in BSA writer.
pub async fn create_bsa(
    file_states: &[BSAFileState],
    staging_dir: &Path,
    output: &Path,
) -> Result<()> {
    // TODO: implement minimal BSA/BA2 writer or shell out to bsab
    let _ = (file_states, staging_dir, output);
    todo!("BSA/BA2 archive creation not yet implemented")
}
