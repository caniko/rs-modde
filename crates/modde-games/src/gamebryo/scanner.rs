use std::path::Path;

use anyhow::{Context, Result};

use crate::traits::{DiscoveredFile, DiscoveredMod, ModScanner, ModSource, ScanContext};

pub struct GamebryoScanner {
    pub game_id: &'static str,
}

pub static FALLOUT_NEW_VEGAS_SCANNER: GamebryoScanner = GamebryoScanner {
    game_id: "fallout-new-vegas",
};

pub static OBLIVION_SCANNER: GamebryoScanner = GamebryoScanner {
    game_id: "oblivion",
};

const PLUGIN_EXTENSIONS: &[&str] = &["esp", "esm"];
const ARCHIVE_EXTENSIONS: &[&str] = &["bsa"];

impl ModScanner for GamebryoScanner {
    fn scan_directories(&self) -> &[&str] {
        &["Data"]
    }

    fn scan_filesystem(&self, ctx: &ScanContext<'_>) -> Result<Vec<DiscoveredMod>> {
        let data_dir = ctx.install_dir.join("Data");
        if !data_dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut mods = Vec::new();
        for entry in std::fs::read_dir(&data_dir)
            .with_context(|| format!("failed to read directory: {}", data_dir.display()))?
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("")
                .to_lowercase();
            if !PLUGIN_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }

            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let mut files = vec![make_data_file(ctx.install_dir, &path)];
            for archive_ext in ARCHIVE_EXTENSIONS {
                let archive_path = data_dir.join(format!("{stem}.{archive_ext}"));
                if archive_path.exists() {
                    files.push(make_data_file(ctx.install_dir, &archive_path));
                }
            }

            let filename = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(stem)
                .to_string();
            mods.push(DiscoveredMod {
                mod_id: format!("plugin/{filename}"),
                display_name: filename,
                version: None,
                files,
                source: ModSource::Filesystem {
                    location: "Data".into(),
                },
                confidence: 0.85,
            });
        }

        Ok(mods)
    }

    fn mod_id_footprint(&self, mod_id: &str) -> Option<modde_core::scanner::ModFootprint> {
        let filename = mod_id.strip_prefix("plugin/")?.to_lowercase();
        Some(modde_core::scanner::ModFootprint::File(filename))
    }
}

fn make_data_file(install_root: &Path, file_path: &Path) -> DiscoveredFile {
    let rel = file_path
        .strip_prefix(install_root)
        .unwrap_or(file_path)
        .to_string_lossy()
        .replace('\\', "/");
    let size = file_path.metadata().map_or(0, |m| m.len());
    DiscoveredFile {
        rel_path: rel,
        size,
    }
}
