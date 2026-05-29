//! Mod scanner for Cyberpunk 2077.

use std::path::Path;

use anyhow::{Context, Result};

use super::manifest::RedModManifest;
use crate::scanner_patterns::{DirectoryModRule, SingleFileModRule};
use crate::traits::{DiscoveredMod, ModScanner, ModSource, ScanContext, walk_files_relative};

/// [`ModScanner`] that discovers installed Cyberpunk 2077 mods.
pub struct CyberpunkScanner;

pub static CYBERPUNK_SCANNER: CyberpunkScanner = CyberpunkScanner;

/// Directories scanned for Cyberpunk 2077 mods, relative to install root.
const SCAN_DIRS: &[&str] = &[
    "bin/x64/plugins/cyber_engine_tweaks/mods",
    "r6/scripts",
    "r6/tweaks",
    "archive/pc/mod",
    "mods",
];

const CET_RULE: DirectoryModRule = DirectoryModRule {
    rel_dir: "bin/x64/plugins/cyber_engine_tweaks/mods",
    mod_id_prefix: "cet",
    source_location: "cet",
    confidence: 0.7,
    marker_file: Some("init.lua"),
    marker_confidence: Some(0.95),
};

const REDSCRIPT_RULE: DirectoryModRule = DirectoryModRule {
    rel_dir: "r6/scripts",
    mod_id_prefix: "reds",
    source_location: "r6/scripts",
    confidence: 0.9,
    marker_file: None,
    marker_confidence: None,
};

const TWEAKXL_RULE: DirectoryModRule = DirectoryModRule {
    rel_dir: "r6/tweaks",
    mod_id_prefix: "tweak",
    source_location: "r6/tweaks",
    confidence: 0.9,
    marker_file: None,
    marker_confidence: None,
};

const ARCHIVE_RULE: SingleFileModRule = SingleFileModRule {
    rel_dir: "archive/pc/mod",
    extension: "archive",
    ignored_prefixes: &[],
    mod_id_prefix: "archive",
    source_location: "archive/pc/mod",
    confidence: 0.85,
};

impl ModScanner for CyberpunkScanner {
    fn scan_directories(&self) -> &[&str] {
        SCAN_DIRS
    }

    fn scan_filesystem(&self, ctx: &ScanContext<'_>) -> Result<Vec<DiscoveredMod>> {
        let install = ctx.install_dir;
        let mut mods = Vec::new();

        CET_RULE.scan(install, &mut mods)?;
        REDSCRIPT_RULE.scan(install, &mut mods)?;
        TWEAKXL_RULE.scan(install, &mut mods)?;
        ARCHIVE_RULE.scan(install, &mut mods)?;
        scan_redmod_mods(install, &mut mods)?;

        Ok(mods)
    }

    /// Inverse of the scheme used in the `scan_*_mods` helpers below.
    /// Must stay in sync with them — if a new scan pass is added (or a
    /// prefix changes), this function needs the matching branch.
    ///
    /// Directory footprints are lowercased and terminated with a
    /// trailing `/`; file footprints are lowercased and match the on-disk
    /// layout exactly. Both conventions match what
    /// `modde_core::scanner::detect_stale_duplicates` expects.
    fn mod_id_footprint(&self, mod_id: &str) -> Option<modde_core::scanner::ModFootprint> {
        use modde_core::scanner::ModFootprint;
        if let Some(name) = mod_id.strip_prefix("cet/") {
            Some(ModFootprint::Directory(format!(
                "bin/x64/plugins/cyber_engine_tweaks/mods/{}/",
                name.to_lowercase()
            )))
        } else if let Some(name) = mod_id.strip_prefix("reds/") {
            Some(ModFootprint::Directory(format!(
                "r6/scripts/{}/",
                name.to_lowercase()
            )))
        } else if let Some(name) = mod_id.strip_prefix("tweak/") {
            Some(ModFootprint::Directory(format!(
                "r6/tweaks/{}/",
                name.to_lowercase()
            )))
        } else if let Some(name) = mod_id.strip_prefix("redmod/") {
            Some(ModFootprint::Directory(format!(
                "mods/{}/",
                name.to_lowercase()
            )))
        } else {
            mod_id.strip_prefix("archive/").map(|stem| {
                ModFootprint::File(format!("archive/pc/mod/{}.archive", stem.to_lowercase()))
            })
        }
    }
}

/// `REDmod` mods: each subdirectory of `mods/` is one mod (parse `info.json`).
fn scan_redmod_mods(install: &Path, out: &mut Vec<DiscoveredMod>) -> Result<()> {
    let mods_dir = install.join("mods");
    if !mods_dir.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(&mods_dir)
        .with_context(|| format!("failed to read directory: {}", mods_dir.display()))?
        .flatten()
    {
        if !entry.path().is_dir() {
            continue;
        }

        let dir_name = entry.file_name().to_string_lossy().to_string();
        let info_json = entry.path().join("info.json");

        let (name, version) = if info_json.exists() {
            match std::fs::read_to_string(&info_json)
                .ok()
                .and_then(|s| RedModManifest::parse(&s).ok())
            {
                Some(manifest) => (manifest.name, manifest.version),
                None => (dir_name.clone(), None),
            }
        } else {
            (dir_name.clone(), None)
        };

        let files = walk_files_relative(install, &entry.path());
        if files.is_empty() {
            continue;
        }

        out.push(DiscoveredMod {
            mod_id: format!("redmod/{dir_name}"),
            display_name: name,
            version,
            files,
            source: ModSource::Filesystem {
                location: "mods".into(),
            },
            confidence: if info_json.exists() { 0.95 } else { 0.8 },
        });
    }
    Ok(())
}
