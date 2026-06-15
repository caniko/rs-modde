#![allow(clippy::wildcard_imports)]
//! Wabbajack archive import command.

use super::*;

pub(super) async fn import_archive(manifest_path: PathBuf, archives: Vec<PathBuf>) -> Result<()> {
    if archives.is_empty() {
        anyhow::bail!("at least one archive path is required");
    }
    let manifest = parse_wabbajack_manifest(&manifest_path)?;
    let store = modde_core::paths::store_dir();
    let results = import_archives(&manifest, &store, &archives).await?;

    let mut refused = 0_usize;
    for result in &results {
        match result.status {
            ArchiveImportStatus::Imported => {
                println!(
                    "imported {} -> {} ({})",
                    result.source_path.display(),
                    result
                        .store_path
                        .as_ref()
                        .map_or_else(|| "<missing>".into(), |p| p.display().to_string()),
                    result.matched_archive.as_deref().unwrap_or("<unknown>")
                );
            }
            ArchiveImportStatus::AlreadyPresent => {
                println!(
                    "already-present {} -> {} ({})",
                    result.source_path.display(),
                    result
                        .store_path
                        .as_ref()
                        .map_or_else(|| "<missing>".into(), |p| p.display().to_string()),
                    result.matched_archive.as_deref().unwrap_or("<unknown>")
                );
            }
            ArchiveImportStatus::Mismatched => {
                refused += 1;
                eprintln!(
                    "mismatched {}: filename appears in manifest, but computed xxh64 {:016x} does not match any archive hash",
                    result.source_path.display(),
                    result.computed_xxh64
                );
            }
            ArchiveImportStatus::Unused => {
                refused += 1;
                eprintln!(
                    "unused {}: computed xxh64 {:016x} is not referenced by the manifest",
                    result.source_path.display(),
                    result.computed_xxh64
                );
            }
        }
    }

    if refused > 0 {
        anyhow::bail!("refused {refused} archive import(s)");
    }

    Ok(())
}
