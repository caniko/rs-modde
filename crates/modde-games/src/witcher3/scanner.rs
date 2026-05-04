use anyhow::Result;

use crate::scanner_patterns::DirectoryModRule;
use crate::traits::{DiscoveredMod, ModScanner, ScanContext};

pub struct Witcher3Scanner;

pub static WITCHER3_SCANNER: Witcher3Scanner = Witcher3Scanner;

impl ModScanner for Witcher3Scanner {
    fn scan_directories(&self) -> &[&str] {
        &["mods", "dlc"]
    }

    fn scan_filesystem(&self, ctx: &ScanContext<'_>) -> Result<Vec<DiscoveredMod>> {
        let mut out = Vec::new();
        DirectoryModRule {
            rel_dir: "mods",
            mod_id_prefix: "mod",
            source_location: "mods",
            confidence: 0.9,
            marker_file: None,
            marker_confidence: None,
        }
        .scan(ctx.install_dir, &mut out)?;
        DirectoryModRule {
            rel_dir: "dlc",
            mod_id_prefix: "dlc",
            source_location: "dlc",
            confidence: 0.75,
            marker_file: None,
            marker_confidence: None,
        }
        .scan(ctx.install_dir, &mut out)?;
        Ok(out)
    }

    fn mod_id_footprint(&self, mod_id: &str) -> Option<modde_core::scanner::ModFootprint> {
        if let Some(name) = mod_id.strip_prefix("mod/") {
            return Some(modde_core::scanner::ModFootprint::Directory(format!(
                "mods/{}/",
                name.to_lowercase()
            )));
        }
        let name = mod_id.strip_prefix("dlc/")?.to_lowercase();
        Some(modde_core::scanner::ModFootprint::Directory(format!(
            "dlc/{name}/"
        )))
    }
}
