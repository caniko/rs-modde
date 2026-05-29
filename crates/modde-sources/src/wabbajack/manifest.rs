//! Reading the JSON manifest embedded inside a `.wabbajack` archive.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};

use modde_core::manifest::wabbajack::WabbajackManifest;

/// Parse a `.wabbajack` file (a zip archive containing a JSON manifest).
pub fn parse_wabbajack_file(path: &Path) -> Result<WabbajackManifest> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to open wabbajack file: {}", path.display()))?;

    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| "failed to read wabbajack file as zip archive")?;

    // Look for the manifest JSON inside the archive
    let manifest_name = find_manifest_entry(&archive)?;

    let mut entry = archive.by_name(&manifest_name)?;
    let mut json = String::new();
    entry.read_to_string(&mut json)?;

    let manifest: WabbajackManifest =
        serde_json::from_str(&json).with_context(|| "failed to parse wabbajack manifest JSON")?;

    Ok(manifest)
}

fn find_manifest_entry(archive: &zip::ZipArchive<std::fs::File>) -> Result<String> {
    for i in 0..archive.len() {
        let name = archive.name_for_index(i).unwrap_or_default().to_string();
        if name == "modlist" || name.ends_with(".json") {
            return Ok(name);
        }
    }
    anyhow::bail!("no manifest found in wabbajack archive")
}
