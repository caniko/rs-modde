#![allow(clippy::wildcard_imports)]
use super::*;

#[derive(Debug)]
pub(super) struct GameFileSourcePath {
    pub(super) rel_path: String,
    pub(super) path: PathBuf,
}

pub(super) fn is_game_file_archive(
    archive: &modde_core::manifest::wabbajack::ArchiveEntry,
) -> bool {
    matches!(
        archive.state.as_ref(),
        Some(ArchiveState::GameFileSourceDownloader { .. })
    )
}

/// Validate that an archive entry name does not contain path traversal components
/// or represent a symlink entry, preventing zip-slip and symlink attacks.
pub(super) fn validate_archive_entry(name: &str) -> Result<()> {
    // Normalize separators for consistent checking
    let normalized = name.replace('\\', "/");

    // Reject entries with ".." path components (path traversal / zip-slip)
    for component in normalized.split('/') {
        if component == ".." {
            bail!("archive entry contains path traversal: {name}");
        }
    }

    // Reject absolute paths
    if normalized.starts_with('/') {
        bail!("archive entry contains absolute path: {name}");
    }

    Ok(())
}

/// Validate a zip entry, checking for path traversal and symlinks.
#[cfg(test)]
pub(super) fn validate_zip_entry<R: std::io::Read + ?Sized>(
    entry: &zip::read::ZipFile<'_, R>,
) -> Result<()> {
    let name = entry.name();
    validate_archive_entry(name)?;

    // Reject symlink entries from zip archives
    if entry.is_symlink() {
        bail!("archive entry is a symlink (rejected for security): {name}");
    }

    Ok(())
}

/// Normalize Windows-style backslash paths to forward slashes for Linux.
pub(super) fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

pub(super) fn game_file_source_is_whole_file(path: &Path, rel_path: &str, from: &str) -> bool {
    if from.trim().is_empty() || from == "." {
        return true;
    }

    let normalized_from = normalize_path(from);
    let normalized_rel_path = normalize_path(rel_path);
    let normalized_path = normalize_path(&path.to_string_lossy());
    let path_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(normalize_path);

    normalized_rel_path.eq_ignore_ascii_case(&normalized_from)
        || normalized_path.ends_with(&normalized_from)
        || path_name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(&normalized_from))
}

pub(super) async fn read_game_file_source(path: &Path, from: &str) -> Result<Vec<u8>> {
    if !from.trim().is_empty() {
        validate_archive_entry(from)?;
    }

    if path.symlink_metadata()?.file_type().is_symlink() {
        bail!(
            "game-file source is a symlink (rejected for security): {}",
            path.display()
        );
    }

    if from.trim().is_empty() || from == "." {
        return tokio::fs::read(path)
            .await
            .with_context(|| format!("failed to read game-file source: {}", path.display()));
    }

    if game_file_source_is_whole_file(path, &path.to_string_lossy(), from) {
        return tokio::fs::read(path)
            .await
            .with_context(|| format!("failed to read game-file source: {}", path.display()));
    }

    let archive_path = path.to_path_buf();
    let from = from.to_string();
    tokio::task::spawn_blocking(move || {
        let output = ArchiveBatchExtractor::extract_selected(
            &archive_path,
            &[ArchiveRequest {
                directive_index: 0,
                from,
                inner_path: None,
                kind: ArchiveRequestKind::Bytes,
            }],
        )?;
        output
            .bytes
            .get(&0)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("archive extractor returned no bytes"))
    })
    .await?
}
