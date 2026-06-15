use std::path::Path;

// ── Mod Scanner ─────────────────────────────────────────────────

/// Input handed to a [`ModScanner`]: the game install directory to scan.
pub struct ScanContext<'a> {
    pub install_dir: &'a Path,
}

/// A single file belonging to a [`DiscoveredMod`], with its install-relative path and size.
#[derive(Debug, Clone)]
pub struct DiscoveredFile {
    pub rel_path: String,
    pub size: u64,
}

/// Where a [`DiscoveredMod`] was found: an on-disk location or inside an archive.
#[derive(Debug, Clone)]
pub enum ModSource {
    Filesystem { location: String },
    Archive { archive_name: String },
}

/// A mod found by a [`ModScanner`], with its identity, files, source, and a
/// `confidence` score for how certain the scanner is about the detection.
#[derive(Debug, Clone)]
pub struct DiscoveredMod {
    pub mod_id: String,
    pub display_name: String,
    pub version: Option<String>,
    pub files: Vec<DiscoveredFile>,
    pub source: ModSource,
    pub confidence: f64,
}

/// Game-specific discovery of already-installed mods.
///
/// Each game implements this to recognise its own mod layout (directory
/// conventions, loose files, archives) and report what it finds. This is the
/// central scanning interface the installer and profile importer rely on.
pub trait ModScanner: Send + Sync {
    /// Install-relative directories this scanner inspects for mods.
    fn scan_directories(&self) -> &[&str];
    /// Scan the filesystem for installed mods and return what was discovered.
    fn scan_filesystem(&self, ctx: &ScanContext<'_>) -> anyhow::Result<Vec<DiscoveredMod>>;

    /// Inverse of [`ModScanner::scan_filesystem`]'s `mod_id` scheme: given
    /// a `mod_id` this scanner would produce, return the filesystem footprint
    /// that mod owns (directory subtree or single file).
    ///
    /// Used by `modde_core::scanner::detect_stale_duplicates` to correlate
    /// profile rows with a Wabbajack manifest's install directives. The
    /// default impl returns `None`, which causes the dedup path to skip
    /// the row. Game plugins that want their filesystem-scanner rows to
    /// participate in dedup should override this.
    fn mod_id_footprint(&self, _mod_id: &str) -> Option<modde_core::scanner::ModFootprint> {
        None
    }
}

#[must_use]
pub fn walk_files_relative(base: &Path, dir: &Path) -> Vec<DiscoveredFile> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                result.extend(walk_files_relative(base, &path));
            } else if let Ok(meta) = path.metadata()
                && let Ok(rel) = path.strip_prefix(base)
            {
                result.push(DiscoveredFile {
                    rel_path: rel.to_string_lossy().to_string(),
                    size: meta.len(),
                });
            }
        }
    }
    result
}

#[must_use]
pub fn slug(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}
