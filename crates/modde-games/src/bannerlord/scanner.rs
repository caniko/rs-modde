use anyhow::Result;

use crate::traits::{DiscoveredMod, ModScanner, ModSource, ScanContext, walk_files_relative};

pub struct BannerlordScanner;

pub static BANNERLORD_SCANNER: BannerlordScanner = BannerlordScanner;

impl ModScanner for BannerlordScanner {
    fn scan_directories(&self) -> &[&str] {
        &["Modules"]
    }

    fn scan_filesystem(&self, ctx: &ScanContext<'_>) -> Result<Vec<DiscoveredMod>> {
        let modules_dir = ctx.install_dir.join("Modules");
        if !modules_dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for entry in std::fs::read_dir(&modules_dir)?.flatten() {
            if !entry.file_type().is_ok_and(|ty| ty.is_dir()) {
                continue;
            }
            let module_dir = entry.path();
            let submodule = module_dir.join("SubModule.xml");
            if !submodule.is_file() {
                continue;
            }
            let info = super::parse_submodule_xml(&submodule).ok();
            let fallback = entry.file_name().to_string_lossy().to_string();
            let id = info
                .as_ref()
                .map_or(fallback.clone(), |info| info.id.clone());
            let display_name = info.map_or(fallback, |info| info.name);
            let files = walk_files_relative(ctx.install_dir, &module_dir);
            if files.is_empty() {
                continue;
            }
            out.push(DiscoveredMod {
                mod_id: format!("module/{id}"),
                display_name,
                version: None,
                files,
                source: ModSource::Filesystem {
                    location: "Modules".into(),
                },
                confidence: 0.95,
            });
        }

        Ok(out)
    }

    fn mod_id_footprint(&self, mod_id: &str) -> Option<modde_core::scanner::ModFootprint> {
        let name = mod_id.strip_prefix("module/")?.to_lowercase();
        Some(modde_core::scanner::ModFootprint::Directory(format!(
            "modules/{name}/"
        )))
    }
}
