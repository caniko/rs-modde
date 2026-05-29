//! Mod scanner for Unreal Engine 4 pak-based games.

use anyhow::Result;

use crate::scanner_patterns::FileGroupRule;
use crate::traits::{DiscoveredMod, ModScanner, ScanContext};

/// Data-driven scanner for UE4 pak-based mods.
///
/// Walks `<ProjectName>/Content/Paks/~mods` and `.../LogicMods`, grouping
/// `.pak` / `.ucas` / `.utoc` triples by file stem. Each stem becomes one
/// [`DiscoveredMod`] with `mod_id = "pak/<stem>"`.
pub struct Ue4Scanner {
    pub game_id: &'static str,
    pub project_name: &'static str,
}

pub static STELLAR_BLADE_SCANNER: Ue4Scanner = Ue4Scanner {
    game_id: "stellar-blade",
    project_name: "SB",
};

pub static SUBNAUTICA2_SCANNER: Ue4Scanner = Ue4Scanner {
    game_id: "subnautica2",
    project_name: "Subnautica2",
};

const UE4_GROUP_EXTENSIONS: &[&str] = &["pak", "ucas", "utoc"];

impl Ue4Scanner {
    fn scan_subdir(
        &self,
        install: &std::path::Path,
        subdir: &str,
        location: &'static str,
        out: &mut Vec<DiscoveredMod>,
    ) {
        let dir = install
            .join(self.project_name)
            .join("Content")
            .join("Paks")
            .join(subdir);
        FileGroupRule {
            rel_dir: "",
            extensions: UE4_GROUP_EXTENSIONS,
            mod_id_prefix: "pak",
            source_location: location,
            confidence: 0.9,
        }
        .scan_dir(install, &dir, out);
    }
}

impl ModScanner for Ue4Scanner {
    fn scan_directories(&self) -> &[&str] {
        // Used by cache invalidation. Exact subpaths are composed at scan
        // time because they depend on `project_name`.
        &["Content/Paks/~mods", "Content/Paks/LogicMods"]
    }

    fn scan_filesystem(&self, ctx: &ScanContext<'_>) -> Result<Vec<DiscoveredMod>> {
        let mut out = Vec::new();
        self.scan_subdir(ctx.install_dir, "~mods", "paks-mods", &mut out);
        self.scan_subdir(ctx.install_dir, "LogicMods", "logic-mods", &mut out);
        Ok(out)
    }

    fn mod_id_footprint(&self, mod_id: &str) -> Option<modde_core::scanner::ModFootprint> {
        // Represent the mod by its `.pak` file; ucas/utoc siblings collapse
        // into the same row (same convention as Cyberpunk's `archive/` branch).
        let stem = mod_id.strip_prefix("pak/")?.to_lowercase();
        Some(modde_core::scanner::ModFootprint::File(format!(
            "{}/content/paks/~mods/{stem}.pak",
            self.project_name.to_lowercase()
        )))
    }
}
