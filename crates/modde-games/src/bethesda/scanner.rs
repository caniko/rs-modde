use std::path::Path;

use anyhow::Result;

use super::plugins_txt;
use crate::traits::{DiscoveredFile, DiscoveredMod, ModScanner, ModSource, ScanContext};

/// Data-driven Bethesda mod scanner.
pub struct BethesdaScanner {
    pub game_id: &'static str,
    pub steam_app_id: u32,
    pub game_folder_name: &'static str,
}

pub static SKYRIM_SCANNER: BethesdaScanner = BethesdaScanner {
    game_id: "skyrim-se",
    steam_app_id: plugins_txt::SKYRIM_SE_APP_ID,
    game_folder_name: "Skyrim Special Edition",
};

pub static FALLOUT4_SCANNER: BethesdaScanner = BethesdaScanner {
    game_id: "fallout4",
    steam_app_id: plugins_txt::FALLOUT4_APP_ID,
    game_folder_name: "Fallout4",
};

pub static STARFIELD_SCANNER: BethesdaScanner = BethesdaScanner {
    game_id: "starfield",
    steam_app_id: plugins_txt::STARFIELD_APP_ID,
    game_folder_name: "Starfield",
};

const BETHESDA_SCAN_DIRS: &[&str] = &["Data"];

/// Plugin file extensions that Bethesda games use.
const PLUGIN_EXTENSIONS: &[&str] = &["esp", "esm", "esl"];

/// Archive extensions that may accompany a plugin.
const ARCHIVE_EXTENSIONS: &[&str] = &["bsa", "ba2"];

impl ModScanner for BethesdaScanner {
    fn scan_directories(&self) -> &[&str] {
        BETHESDA_SCAN_DIRS
    }

    fn scan_filesystem(&self, ctx: &ScanContext<'_>) -> Result<Vec<DiscoveredMod>> {
        let data_dir = ctx.install_dir.join("Data");
        if !data_dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut mods = Vec::new();

        // Try to read plugins.txt for load order and enabled status.
        let known_plugins = plugins_txt::read_plugins_txt(self.steam_app_id, self.game_folder_name)
            .unwrap_or_default();

        // Scan plugins listed in plugins.txt first (these are authoritative).
        let mut seen_stems: std::collections::HashSet<String> = std::collections::HashSet::new();

        for plugin_entry in &known_plugins {
            let plugin_path = data_dir.join(&plugin_entry.name);
            if !plugin_path.exists() {
                continue;
            }

            let stem = std::path::Path::new(&plugin_entry.name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&plugin_entry.name)
                .to_string();

            seen_stems.insert(stem.to_lowercase());

            let mut files = vec![make_data_file(ctx.install_dir, &plugin_path)];

            // Look for companion archives (same stem).
            for ext in ARCHIVE_EXTENSIONS {
                let archive_path = data_dir.join(format!("{stem}.{ext}"));
                if archive_path.exists() {
                    files.push(make_data_file(ctx.install_dir, &archive_path));
                }
                // Bethesda also uses " - Textures" suffix for texture BSAs.
                let tex_path = data_dir.join(format!("{stem} - Textures.{ext}"));
                if tex_path.exists() {
                    files.push(make_data_file(ctx.install_dir, &tex_path));
                }
            }

            mods.push(DiscoveredMod {
                mod_id: format!("plugin/{}", plugin_entry.name),
                display_name: plugin_entry.name.clone(),
                version: None,
                files,
                source: ModSource::Filesystem {
                    location: "Data".into(),
                },
                confidence: 0.95,
            });
        }

        // Also scan for plugins NOT in plugins.txt (disabled or unmanaged).
        for entry in std::fs::read_dir(&data_dir)?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                continue;
            }

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            if !PLUGIN_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }

            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            if seen_stems.contains(&stem.to_lowercase()) {
                continue;
            }

            let mut files = vec![make_data_file(ctx.install_dir, &path)];

            for archive_ext in ARCHIVE_EXTENSIONS {
                let archive_path = data_dir.join(format!("{stem}.{archive_ext}"));
                if archive_path.exists() {
                    files.push(make_data_file(ctx.install_dir, &archive_path));
                }
            }

            mods.push(DiscoveredMod {
                mod_id: format!("plugin/{stem}.{ext}"),
                display_name: format!("{stem}.{ext}"),
                version: None,
                files,
                source: ModSource::Filesystem {
                    location: "Data".into(),
                },
                confidence: 0.8, // Lower confidence since not in plugins.txt
            });
        }

        Ok(mods)
    }

    fn mod_id_footprint(&self, mod_id: &str) -> Option<modde_core::scanner::ModFootprint> {
        // Inverse of the `plugin/<filename>` scheme produced by `scan_filesystem`.
        // Footprint is Data-relative because MO2 Bethesda mod folders mirror
        // `Data/` (not the game install root), so the manifest paths we compare
        // against are Data-relative after `strip_mo2_prefix` in
        // `detect_stale_duplicates`.
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
