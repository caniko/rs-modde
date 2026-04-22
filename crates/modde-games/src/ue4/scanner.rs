use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;

use crate::traits::{
    DiscoveredFile, DiscoveredMod, ModScanner, ModSource, ScanContext, walk_files_relative,
};

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

/// Returns the basename stem if `rel` ends with a UE4 pak-triple extension.
///
/// `rel` is expected to be a relative path string; both `/` and `\` are
/// tolerated. Matching is case-insensitive.
fn stem_for(rel: &str) -> Option<String> {
    let lower = rel.to_lowercase();
    if !(lower.ends_with(".pak") || lower.ends_with(".ucas") || lower.ends_with(".utoc")) {
        return None;
    }
    let path = std::path::Path::new(rel);
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

impl Ue4Scanner {
    fn scan_subdir(
        &self,
        install: &Path,
        subdir: &str,
        location: &str,
        out: &mut Vec<DiscoveredMod>,
    ) {
        let dir = install
            .join(self.project_name)
            .join("Content")
            .join("Paks")
            .join(subdir);
        if !dir.is_dir() {
            return;
        }

        // Group discovered files by file stem so a .pak + .ucas + .utoc triple
        // collapses into a single DiscoveredMod.
        let mut by_stem: BTreeMap<String, Vec<DiscoveredFile>> = BTreeMap::new();

        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                // Some packaged mods ship as a subdir containing the pak triple.
                for f in walk_files_relative(install, &path) {
                    if let Some(stem) = stem_for(&f.rel_path) {
                        by_stem.entry(stem).or_default().push(f);
                    }
                }
                continue;
            }

            let Ok(meta) = path.metadata() else {
                continue;
            };
            let Ok(rel) = path.strip_prefix(install) else {
                continue;
            };
            let rel_str = rel.to_string_lossy().to_string();
            let Some(stem) = stem_for(&rel_str) else {
                continue;
            };
            by_stem.entry(stem).or_default().push(DiscoveredFile {
                rel_path: rel_str,
                size: meta.len(),
            });
        }

        for (stem, files) in by_stem {
            out.push(DiscoveredMod {
                mod_id: format!("pak/{stem}"),
                display_name: stem,
                version: None,
                files,
                source: ModSource::Filesystem {
                    location: location.into(),
                },
                confidence: 0.9,
            });
        }
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
