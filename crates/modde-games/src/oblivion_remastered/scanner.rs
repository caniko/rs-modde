//! Mod scanner for Oblivion Remastered.

use anyhow::Result;

use crate::scanner_patterns::{FileGroupRule, SingleFileModRule};
use crate::traits::{DiscoveredMod, ModScanner, ScanContext};

/// [`ModScanner`] that discovers installed Oblivion Remastered mods.
pub struct OblivionRemasteredScanner;

pub static OBLIVION_REMASTERED_SCANNER: OblivionRemasteredScanner = OblivionRemasteredScanner;

const GROUP_EXTENSIONS: &[&str] = &["pak", "ucas", "utoc"];

impl ModScanner for OblivionRemasteredScanner {
    fn scan_directories(&self) -> &[&str] {
        &["Content/Paks/~mods", "Data"]
    }

    fn scan_filesystem(&self, ctx: &ScanContext<'_>) -> Result<Vec<DiscoveredMod>> {
        let mut out = Vec::new();
        let paks = super::paks_root(ctx.install_dir).join("~mods");
        FileGroupRule {
            rel_dir: "",
            extensions: GROUP_EXTENSIONS,
            mod_id_prefix: "pak",
            source_location: "paks-mods",
            confidence: 0.9,
        }
        .scan_dir(ctx.install_dir, &paks, &mut out);

        SingleFileModRule {
            rel_dir: "Data",
            extension: "esp",
            ignored_prefixes: &[],
            mod_id_prefix: "plugin",
            source_location: "Data",
            confidence: 0.8,
        }
        .scan(ctx.install_dir, &mut out)?;
        Ok(out)
    }

    fn mod_id_footprint(&self, mod_id: &str) -> Option<modde_core::scanner::ModFootprint> {
        if let Some(stem) = mod_id.strip_prefix("pak/") {
            return Some(modde_core::scanner::ModFootprint::File(format!(
                "oblivionremastered/content/paks/~mods/{}.pak",
                stem.to_lowercase()
            )));
        }
        let stem = mod_id.strip_prefix("plugin/")?.to_lowercase();
        Some(modde_core::scanner::ModFootprint::File(format!(
            "data/{stem}.esp"
        )))
    }
}
