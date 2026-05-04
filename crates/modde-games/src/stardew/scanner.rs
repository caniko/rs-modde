use anyhow::Result;

use crate::scanner_patterns::DirectoryModRule;
use crate::traits::{DiscoveredMod, ModScanner, ScanContext};

pub struct StardewScanner;

pub static STARDEW_SCANNER: StardewScanner = StardewScanner;

impl ModScanner for StardewScanner {
    fn scan_directories(&self) -> &[&str] {
        &["Mods"]
    }

    fn scan_filesystem(&self, ctx: &ScanContext<'_>) -> Result<Vec<DiscoveredMod>> {
        let mut out = Vec::new();
        DirectoryModRule {
            rel_dir: "Mods",
            mod_id_prefix: "smapi",
            source_location: "Mods",
            confidence: 0.75,
            marker_file: Some("manifest.json"),
            marker_confidence: Some(0.98),
        }
        .scan(ctx.install_dir, &mut out)?;
        Ok(out)
    }

    fn mod_id_footprint(&self, mod_id: &str) -> Option<modde_core::scanner::ModFootprint> {
        let name = mod_id.strip_prefix("smapi/")?.to_lowercase();
        Some(modde_core::scanner::ModFootprint::Directory(format!(
            "mods/{name}/"
        )))
    }
}
