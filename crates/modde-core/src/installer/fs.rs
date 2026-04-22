//! Filesystem helpers shared by the install pipeline.
//!
//! `extract_archive` and `find_fomod_config` used to live in
//! `modde-cli/src/commands/install.rs`. They are hoisted here so both the
//! CLI and the UI install paths share a single implementation, and so the
//! installer crate can probe extracted archives directly without reaching
//! back into CLI code.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tracing::warn;

use super::types::{InstallerError, InstallerResult};

/// Extract a zip archive into `dest`. Refuses entries whose paths would
/// escape `dest` (e.g. `../../etc/passwd`) — zip's `enclosed_name` guard.
pub fn extract_archive(archive_path: &Path, dest: &Path) -> InstallerResult<()> {
    let file = fs::File::open(archive_path)
        .map_err(|e| InstallerError::Extract(format!("open {}: {e}", archive_path.display())))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| InstallerError::Extract(format!("read {}: {e}", archive_path.display())))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| InstallerError::Extract(format!("entry {i}: {e}")))?;
        let Some(name) = entry.enclosed_name() else {
            warn!("skipping archive entry with unsafe path");
            continue;
        };
        let out_path = dest.join(name);

        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out_file = fs::File::create(&out_path)?;
            io::copy(&mut entry, &mut out_file)?;
        }
    }

    Ok(())
}

/// Locate a FOMOD `ModuleConfig.xml` under `mod_dir`, trying the canonical
/// path first and then falling back to case-insensitive matching for
/// archives built on case-sensitive filesystems.
///
/// Returns the absolute path, or `None` if this mod is not FOMOD-packaged.
#[must_use]
pub fn find_fomod_config(mod_dir: &Path) -> Option<PathBuf> {
    let canonical = mod_dir.join("fomod").join("ModuleConfig.xml");
    if canonical.exists() {
        return Some(canonical);
    }

    let entries = fs::read_dir(mod_dir).ok()?;
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        if !entry.file_name().eq_ignore_ascii_case("fomod") {
            continue;
        }
        let inner_entries = fs::read_dir(entry.path()).ok()?;
        for inner in inner_entries.flatten() {
            if inner.file_name().eq_ignore_ascii_case("moduleconfig.xml") {
                return Some(inner.path());
            }
        }
    }
    None
}

/// Walk `dir` recursively, returning (absolute path, relative path) pairs
/// for every regular file. Used by `execute` when staging files.
pub(crate) fn walk_files(dir: &Path) -> InstallerResult<Vec<(PathBuf, PathBuf)>> {
    let mut out = Vec::new();
    walk_files_into(dir, dir, &mut out)?;
    Ok(out)
}

fn walk_files_into(
    base: &Path,
    dir: &Path,
    out: &mut Vec<(PathBuf, PathBuf)>,
) -> InstallerResult<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_files_into(base, &path, out)?;
        } else if path.is_file()
            && let Ok(rel) = path.strip_prefix(base)
        {
            out.push((path.clone(), rel.to_path_buf()));
        }
    }
    Ok(())
}

/// Hash a file with xxh64, returning the hex digest. Used to populate
/// `InstallPlan::source_archive_hash` on download.
pub fn xxh64_file_hex(path: &Path) -> InstallerResult<String> {
    use std::io::Read;
    use xxhash_rust::xxh64::Xxh64;

    let mut file = fs::File::open(path)?;
    let mut hasher = Xxh64::new(0);
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:016x}", hasher.digest()))
}
