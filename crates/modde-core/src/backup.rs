use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::info;

use crate::error::{CoreError, Result};
use crate::paths;

// ── Backup Manager ──────────────────────────────────────────────

/// Manages zip-based backups for mods and plugin load orders.
pub struct BackupManager {
    backup_dir: PathBuf,
}

/// Metadata for a single backup.
#[derive(Debug, Clone)]
pub struct BackupEntry {
    pub name: String,
    pub path: PathBuf,
    pub created: SystemTime,
}

impl BackupManager {
    /// Create a new backup manager rooted at the default backup directory.
    pub fn new() -> Result<Self> {
        let backup_dir = paths::data_dir().join("backups");
        fs::create_dir_all(&backup_dir)?;
        Ok(Self { backup_dir })
    }

    /// Create a backup of a mod directory, stored as a zip file.
    pub fn create_mod_backup(&self, mod_id: &str, mod_dir: &Path) -> Result<BackupEntry> {
        let dest_dir = self.backup_dir.join("mods").join(mod_id);
        fs::create_dir_all(&dest_dir)?;

        let timestamp = unix_timestamp();
        let name = format!("{mod_id}_{timestamp}.zip");
        let zip_path = dest_dir.join(&name);

        info!(%mod_id, path = %zip_path.display(), "creating mod backup");

        create_zip_from_dir(mod_dir, &zip_path)?;

        let created = fs::metadata(&zip_path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::now());

        Ok(BackupEntry {
            name,
            path: zip_path,
            created,
        })
    }

    /// Restore a mod from its latest backup zip into `dest_dir`.
    pub fn restore_mod_backup(&self, mod_id: &str, dest_dir: &Path) -> Result<BackupEntry> {
        let entries = self.list_mod_backups(mod_id)?;
        let latest = entries.last().ok_or_else(|| {
            CoreError::Other(format!("no backups found for mod '{mod_id}'").into())
        })?;

        info!(%mod_id, backup = %latest.path.display(), "restoring mod backup");

        extract_zip_to_dir(&latest.path, dest_dir)?;

        Ok(latest.clone())
    }

    /// List available backups for a mod, sorted oldest-first.
    pub fn list_mod_backups(&self, mod_id: &str) -> Result<Vec<BackupEntry>> {
        let dir = self.backup_dir.join("mods").join(mod_id);
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "zip") {
                let created = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::now());
                entries.push(BackupEntry {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    path,
                    created,
                });
            }
        }

        entries.sort_by_key(|e| e.created);
        Ok(entries)
    }

    /// Backup the plugin load order for a profile+game as a JSON file.
    pub fn backup_plugin_order(
        &self,
        profile: &str,
        game: &str,
        plugins: &[String],
    ) -> Result<PathBuf> {
        let dir = self.backup_dir.join("plugins").join(game);
        fs::create_dir_all(&dir)?;

        let timestamp = unix_timestamp();
        let name = format!("{profile}_{timestamp}.json");
        let path = dir.join(&name);

        let json = serde_json::to_string_pretty(plugins)?;
        fs::write(&path, json)?;

        info!(%profile, %game, path = %path.display(), "plugin order backed up");
        Ok(path)
    }

    /// Restore the most recent plugin load order backup for a profile+game.
    pub fn restore_plugin_order(&self, profile: &str, game: &str) -> Result<Vec<String>> {
        let dir = self.backup_dir.join("plugins").join(game);
        if !dir.exists() {
            return Err(CoreError::Other(
                format!("no plugin backups for game '{game}'").into(),
            ));
        }

        let prefix = format!("{profile}_");
        let mut candidates: Vec<_> = fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            .collect();

        candidates.sort_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH)
        });

        let latest = candidates.last().ok_or_else(|| {
            CoreError::Other(
                format!("no plugin backups for profile '{profile}' / game '{game}'").into(),
            )
        })?;

        let data = fs::read_to_string(latest.path())?;
        let plugins: Vec<String> = serde_json::from_str(&data)?;

        Ok(plugins)
    }
}

// ── Helpers ─────────────────────────────────────────────────────

/// Produce a Unix timestamp (seconds since epoch).
fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Create a zip archive from a directory.
fn create_zip_from_dir(src: &Path, dest: &Path) -> Result<()> {
    let file = fs::File::create(dest)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let entries = walkdir(src)?;
    for entry in &entries {
        let rel = entry
            .strip_prefix(src)
            .map_err(|e| CoreError::Other(e.to_string().into()))?;
        let rel_str = rel.to_string_lossy();

        if entry.is_dir() {
            zip.add_directory(rel_str.as_ref(), options)
                .map_err(|e| CoreError::Other(e.to_string().into()))?;
        } else {
            zip.start_file(rel_str.as_ref(), options)
                .map_err(|e| CoreError::Other(e.to_string().into()))?;
            let mut f = fs::File::open(entry)?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            std::io::Write::write_all(&mut zip, &buf)?;
        }
    }

    zip.finish().map_err(|e| CoreError::Other(e.to_string().into()))?;
    Ok(())
}

/// Extract a zip archive into `dest`.
fn extract_zip_to_dir(zip_path: &Path, dest: &Path) -> Result<()> {
    let file = fs::File::open(zip_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| CoreError::Other(e.to_string().into()))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| CoreError::Other(e.to_string().into()))?;
        let out_path = dest.join(entry.name());

        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut outfile)?;
        }
    }

    Ok(())
}

/// Recursively list all files and directories under `root`.
fn walkdir(root: &Path) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    walk_recursive(root, &mut result)?;
    result.sort();
    Ok(result)
}

fn walk_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        out.push(path.clone());
        if path.is_dir() {
            walk_recursive(&path, out)?;
        }
    }
    Ok(())
}
