//! Mod scanner for Baldur's Gate 3.

use anyhow::Result;

use crate::scanner_patterns::SingleFileModRule;
use crate::traits::{DiscoveredMod, ModScanner, ScanContext};

/// [`ModScanner`] that discovers installed BG3 `.pak` mods.
pub struct Bg3Scanner;

pub static BG3_SCANNER: Bg3Scanner = Bg3Scanner;

impl ModScanner for Bg3Scanner {
    fn scan_directories(&self) -> &[&str] {
        &["Mods"]
    }

    fn scan_filesystem(&self, ctx: &ScanContext<'_>) -> Result<Vec<DiscoveredMod>> {
        let data_root = super::data_root_from_install(ctx.install_dir);
        let scan_root = if data_root.join("Mods").is_dir() {
            data_root
        } else {
            ctx.install_dir.to_path_buf()
        };
        let mut out = Vec::new();
        SingleFileModRule {
            rel_dir: "Mods",
            extension: "pak",
            ignored_prefixes: &[],
            mod_id_prefix: "pak",
            source_location: "Mods",
            confidence: 0.95,
        }
        .scan(&scan_root, &mut out)?;
        Ok(out)
    }

    fn mod_id_footprint(&self, mod_id: &str) -> Option<modde_core::scanner::ModFootprint> {
        let stem = mod_id.strip_prefix("pak/")?.to_lowercase();
        Some(modde_core::scanner::ModFootprint::File(format!(
            "mods/{stem}.pak"
        )))
    }
}
