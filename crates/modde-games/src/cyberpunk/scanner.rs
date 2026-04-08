use std::path::Path;

use anyhow::Result;

use crate::traits::{DiscoveredFile, DiscoveredMod, ModScanner, ModSource, ScanContext, walk_files_relative};
use super::manifest::RedModManifest;

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

impl ModScanner for CyberpunkScanner {
    fn scan_directories(&self) -> &[&str] {
        SCAN_DIRS
    }

    fn scan_filesystem(&self, ctx: &ScanContext<'_>) -> Result<Vec<DiscoveredMod>> {
        let install = ctx.install_dir;
        let mut mods = Vec::new();

        scan_cet_mods(install, &mut mods)?;
        scan_redscript_mods(install, &mut mods)?;
        scan_tweakxl_mods(install, &mut mods)?;
        scan_archive_mods(install, &mut mods)?;
        scan_redmod_mods(install, &mut mods)?;

        Ok(mods)
    }
}

/// Cyber Engine Tweaks mods: each subdirectory of `.../cyber_engine_tweaks/mods/` is one mod.
fn scan_cet_mods(install: &Path, out: &mut Vec<DiscoveredMod>) -> Result<()> {
    let cet_dir = install.join("bin/x64/plugins/cyber_engine_tweaks/mods");
    if !cet_dir.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(&cet_dir)?.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let has_init = entry.path().join("init.lua").exists();
        let files = walk_files_relative(install, &entry.path());

        if files.is_empty() {
            continue;
        }

        out.push(DiscoveredMod {
            mod_id: format!("cet/{name}"),
            display_name: name,
            version: None,
            files,
            source: ModSource::Filesystem {
                location: "cet".into(),
            },
            confidence: if has_init { 0.95 } else { 0.7 },
        });
    }
    Ok(())
}

/// REDscript mods: each subdirectory of `r6/scripts/` is one mod.
fn scan_redscript_mods(install: &Path, out: &mut Vec<DiscoveredMod>) -> Result<()> {
    let scripts_dir = install.join("r6/scripts");
    if !scripts_dir.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(&scripts_dir)?.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let files = walk_files_relative(install, &entry.path());

        if files.is_empty() {
            continue;
        }

        out.push(DiscoveredMod {
            mod_id: format!("reds/{name}"),
            display_name: name,
            version: None,
            files,
            source: ModSource::Filesystem {
                location: "r6/scripts".into(),
            },
            confidence: 0.9,
        });
    }
    Ok(())
}

/// TweakXL mods: each subdirectory of `r6/tweaks/` is one mod.
fn scan_tweakxl_mods(install: &Path, out: &mut Vec<DiscoveredMod>) -> Result<()> {
    let tweaks_dir = install.join("r6/tweaks");
    if !tweaks_dir.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(&tweaks_dir)?.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let files = walk_files_relative(install, &entry.path());

        if files.is_empty() {
            continue;
        }

        out.push(DiscoveredMod {
            mod_id: format!("tweak/{name}"),
            display_name: name,
            version: None,
            files,
            source: ModSource::Filesystem {
                location: "r6/tweaks".into(),
            },
            confidence: 0.9,
        });
    }
    Ok(())
}

/// Archive mods: each `.archive` file in `archive/pc/mod/` is one mod.
fn scan_archive_mods(install: &Path, out: &mut Vec<DiscoveredMod>) -> Result<()> {
    let archive_dir = install.join("archive/pc/mod");
    if !archive_dir.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(&archive_dir)?.flatten() {
        let path = entry.path();
        if path.is_dir() || path.extension().and_then(|e| e.to_str()) != Some("archive") {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
        let rel = path
            .strip_prefix(install)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        out.push(DiscoveredMod {
            mod_id: format!("archive/{stem}"),
            display_name: stem.to_string(),
            version: None,
            files: vec![DiscoveredFile { rel_path: rel, size }],
            source: ModSource::Filesystem {
                location: "archive/pc/mod".into(),
            },
            confidence: 0.85,
        });
    }
    Ok(())
}

/// REDmod mods: each subdirectory of `mods/` is one mod (parse `info.json`).
fn scan_redmod_mods(install: &Path, out: &mut Vec<DiscoveredMod>) -> Result<()> {
    let mods_dir = install.join("mods");
    if !mods_dir.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(&mods_dir)?.flatten() {
        if !entry.path().is_dir() {
            continue;
        }

        let dir_name = entry.file_name().to_string_lossy().to_string();
        let info_json = entry.path().join("info.json");

        let (name, version) = if info_json.exists() {
            match std::fs::read_to_string(&info_json).ok().and_then(|s| RedModManifest::parse(&s).ok()) {
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
