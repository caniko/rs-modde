//! Reads inline data entries embedded in a `.wabbajack` zip archive, guarding
//! against unsafe (absolute, traversal, or symlink) entry paths. See
//! [`InlineSource`].

use std::collections::HashSet;
use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, bail};

pub struct InlineSource {
    path: PathBuf,
    names: HashSet<String>,
    archive: Mutex<zip::ZipArchive<File>>,
}

impl InlineSource {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)
            .with_context(|| format!("failed to open wabbajack file: {}", path.display()))?;
        let archive = zip::ZipArchive::new(file)
            .with_context(|| format!("failed to read wabbajack zip: {}", path.display()))?;
        let mut names = HashSet::with_capacity(archive.len());
        for i in 0..archive.len() {
            if let Some(name) = archive.name_for_index(i) {
                names.insert(name.to_string());
            }
        }

        Ok(Self {
            path: path.to_path_buf(),
            names,
            archive: Mutex::new(archive),
        })
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        if !self.names.contains(name) {
            bail!(
                "inline data entry '{}' not found in wabbajack zip {}",
                name,
                self.path.display()
            );
        }

        let mut archive = self
            .archive
            .lock()
            .map_err(|_| anyhow::anyhow!("inline zip lock poisoned"))?;
        let mut entry = archive
            .by_name(name)
            .with_context(|| format!("inline data entry '{name}' not found in wabbajack zip"))?;
        validate_zip_entry(&entry)?;
        let mut data = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut data)?;
        Ok(data)
    }
}

fn validate_zip_entry<R: std::io::Read + ?Sized>(entry: &zip::read::ZipFile<'_, R>) -> Result<()> {
    let name = entry.name();
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/') || normalized.split('/').any(|part| part == "..") {
        bail!("inline zip entry contains unsafe path: {name}");
    }
    if entry.is_symlink() {
        bail!("inline zip entry is a symlink (rejected): {name}");
    }
    Ok(())
}
