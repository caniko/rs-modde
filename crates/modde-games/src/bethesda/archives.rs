//! BSA/BA2 archive awareness for Bethesda games: detects archive files by
//! extension and resolves where they belong in the staging tree, since these
//! games load archives directly without extraction.

use std::path::{Path, PathBuf};

/// BSA/BA2 archive path awareness.
///
/// Ensures archives are placed in the correct location within the mod staging
/// directory without extraction — Bethesda games load BSA/BA2 directly.

/// Common BSA archive extensions for Bethesda games.
pub const BSA_EXTENSIONS: &[&str] = &[".bsa", ".ba2"];

/// Check if a file path is a BSA/BA2 archive.
#[must_use]
pub fn is_archive(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        let ext = format!(".{}", e.to_lowercase());
        BSA_EXTENSIONS.contains(&ext.as_str())
    })
}

/// Determine the expected staging location for a BSA/BA2 archive.
///
/// Archives go directly into the Data directory.
#[must_use]
pub fn staging_path(staging_root: &Path, archive_name: &str) -> PathBuf {
    staging_root.join(archive_name)
}
