use super::*;

/// Best-effort estimate of the peak resident bytes a directive will need
/// during its apply step.
///
/// Most apply work is dominated by archive operations — either streaming a
/// single file out of a native decoder, or feeding a source file plus its
/// patch into bsdiff. We intentionally over-count rather than under-count:
/// being throttled too aggressively just slows the install, while admitting
/// a worker that the host can't satisfy crashes it.
/// Apply-weight tunables for per-directive weighting.
///
/// Read once from the environment before the apply fan-out so that
/// `estimate_directive_weight` does not touch `std::env` per directive.
#[derive(Debug, Clone, Copy)]
pub(super) struct DirectiveWeights {
    from_archive_factor: f64,
    patched_factor: f64,
    create_bsa_floor_mb: u64,
}

impl DirectiveWeights {
    /// Read the tunables from the environment (site operators can dial without rebuilding).
    pub(super) fn from_env() -> Self {
        let from_archive_factor: f64 = std::env::var("MODDE_APPLY_WEIGHT_FROM_ARCHIVE_FACTOR")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|&v: &f64| v > 0.0)
            .unwrap_or(0.5);
        let patched_factor: f64 = std::env::var("MODDE_APPLY_WEIGHT_PATCHED_FACTOR")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|&v: &f64| v > 0.0)
            .unwrap_or(3.0);
        let create_bsa_floor_mb: u64 = std::env::var("MODDE_APPLY_WEIGHT_BSA_FLOOR_MB")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(64);
        Self {
            from_archive_factor,
            patched_factor,
            create_bsa_floor_mb,
        }
    }
}

/// Apply-weight tunables for per-archive-batch weighting.
///
/// Read once from the environment before the apply fan-out so that
/// `estimate_archive_batch_weight` does not touch `std::env` per batch.
#[derive(Debug, Clone, Copy)]
pub(super) struct ArchiveBatchWeights {
    patched_factor: f64,
    archive_factor: f64,
}

impl ArchiveBatchWeights {
    /// Read the tunables from the environment (site operators can dial without rebuilding).
    pub(super) fn from_env() -> Self {
        let patched_factor: f64 = std::env::var("MODDE_APPLY_WEIGHT_PATCHED_BATCH_FACTOR")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|&v| v > 0.0)
            .unwrap_or(1.0);
        let archive_factor: f64 = std::env::var("MODDE_APPLY_WEIGHT_ARCHIVE_BATCH_FACTOR")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|&v| v > 0.0)
            .unwrap_or(0.5);
        Self {
            patched_factor,
            archive_factor,
        }
    }
}

pub(super) fn estimate_directive_weight(
    directive: &InstallDirective,
    archive_size_by_hash: &HashMap<u64, u64>,
    weights: &DirectiveWeights,
) -> u64 {
    const FLOOR_BYTES: u64 = 8 * 1024 * 1024; // 8 MiB

    let raw = match directive {
        InstallDirective::InlineFile { .. } => FLOOR_BYTES,
        InstallDirective::FromArchive { archive_hash, .. } => {
            let size = archive_size_by_hash.get(archive_hash).copied().unwrap_or(0);
            ((size as f64) * weights.from_archive_factor) as u64
        }
        InstallDirective::PatchedFromArchive { archive_hash, .. } => {
            let size = archive_size_by_hash.get(archive_hash).copied().unwrap_or(0);
            ((size as f64) * weights.patched_factor) as u64
        }
        InstallDirective::CreateBSA { file_states, .. } => file_states
            .iter()
            .map(|fs| fs.size)
            .sum::<u64>()
            .max(weights.create_bsa_floor_mb * 1024 * 1024),
    };

    raw.max(FLOOR_BYTES)
}

pub(super) fn estimate_archive_batch_weight(
    batch: &ArchiveInstallBatch,
    weights: &ArchiveBatchWeights,
) -> u64 {
    const FLOOR_BYTES: u64 = 8 * 1024 * 1024;
    let has_patch = archive_batch_has_patch(batch);
    let factor = if has_patch {
        weights.patched_factor
    } else {
        weights.archive_factor
    };
    (((batch.archive_size_bytes as f64) * factor) as u64).max(FLOOR_BYTES)
}

pub(super) fn archive_batch_has_patch(batch: &ArchiveInstallBatch) -> bool {
    batch.directives.iter().any(|directive| {
        matches!(
            &directive.directive,
            InstallDirective::PatchedFromArchive { .. }
        )
    })
}

#[cfg(all(unix, target_env = "gnu"))]
pub(super) fn trim_process_allocator() {
    // Native decoders can transiently allocate very large buffers for solid
    // archives. glibc often keeps those arenas mapped, which makes the next
    // batch look resident even after Rust values were dropped.
    // SAFETY: `malloc_trim(0)` is a process allocator hint on glibc. It takes
    // no borrowed Rust pointers and does not invalidate live allocations.
    unsafe {
        libc::malloc_trim(0);
    }
}

#[cfg(not(all(unix, target_env = "gnu")))]
pub(super) fn trim_process_allocator() {}

/// Find a path by case-insensitive path matching in a directory tree.
pub(super) fn find_path_case_insensitive(base: &Path, relative_path: &str) -> Result<PathBuf> {
    validate_archive_entry(relative_path)?;

    let parts: Vec<&str> = relative_path.split('/').collect();
    let mut current = base.to_path_buf();

    for part in &parts {
        let target_lower = part.to_lowercase();
        let mut found = false;

        for entry in std::fs::read_dir(&current)
            .with_context(|| format!("failed to read dir: {}", current.display()))?
        {
            let entry = entry?;
            if entry.file_name().to_string_lossy().to_lowercase() == target_lower {
                current = entry.path();
                // Reject symlinks in intermediate path components
                if current.symlink_metadata()?.file_type().is_symlink() {
                    anyhow::bail!("path component is a symlink (rejected for security): {part}");
                }
                found = true;
                break;
            }
        }

        if !found {
            anyhow::bail!(
                "path component '{}' not found in {}",
                part,
                current.display()
            );
        }
    }

    Ok(current)
}

/// Extract a file from a zip archive.
#[cfg(test)]
pub(super) fn extract_from_zip(archive_path: &Path, inner_path: &str) -> Result<Vec<u8>> {
    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("failed to open archive: {}", archive_path.display()))?;

    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read zip archive: {}", archive_path.display()))?;

    let entry_name = find_entry_in_archive(&archive, inner_path).with_context(|| {
        format!(
            "file '{}' not found in archive {}",
            inner_path,
            archive_path.display()
        )
    })?;

    let mut entry = archive.by_name(&entry_name)?;
    validate_zip_entry(&entry)?;
    let mut data = Vec::with_capacity(entry.size() as usize);
    std::io::Read::read_to_end(&mut entry, &mut data)?;

    Ok(data)
}

/// Find a file entry in a zip archive, trying multiple path formats.
#[cfg(test)]
pub(super) fn find_entry_in_archive(
    archive: &zip::ZipArchive<std::fs::File>,
    path: &str,
) -> Result<String> {
    // Normalize separators for comparison
    let normalized = path.replace('\\', "/");
    let backslash = path.replace('/', "\\");

    for i in 0..archive.len() {
        let name = archive.name_for_index(i).unwrap_or_default().to_string();
        if name == *path || name == normalized || name == backslash {
            return Ok(name);
        }
        // Case-insensitive fallback
        let name_lower = name.to_lowercase();
        if name_lower == path.to_lowercase()
            || name_lower == normalized.to_lowercase()
            || name_lower == backslash.to_lowercase()
        {
            return Ok(name);
        }
    }

    anyhow::bail!("entry not found: {path}");
}
