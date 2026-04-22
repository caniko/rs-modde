use std::path::Path;

use anyhow::{Context, Result};
use tracing::info;

/// An INI tweak from a mod: a section/key/value triple.
#[derive(Debug, Clone)]
pub struct IniTweak {
    pub mod_id: String,
    pub ini_file: String, // e.g. "Skyrim.ini" or "SkyrimPrefs.ini"
    pub section: String,
    pub key: String,
    pub value: String,
}

/// Scan a mod directory for `.ini` files and extract all key=value pairs.
pub fn scan_mod_ini_tweaks(mod_id: &str, mod_dir: &Path) -> Result<Vec<IniTweak>> {
    let mut tweaks = Vec::new();
    if !mod_dir.exists() {
        return Ok(tweaks);
    }

    for entry in std::fs::read_dir(mod_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ini") {
            continue;
        }
        if !path.is_file() {
            continue;
        }

        let ini_file = path.file_name().unwrap().to_string_lossy().to_string();
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read INI: {}", path.display()))?;

        let mut current_section = String::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                current_section = trimmed[1..trimmed.len() - 1].to_string();
            } else if !trimmed.is_empty()
                && !trimmed.starts_with(';')
                && !trimmed.starts_with('#')
                && let Some((key, value)) = trimmed.split_once('=')
            {
                tweaks.push(IniTweak {
                    mod_id: mod_id.to_string(),
                    ini_file: ini_file.clone(),
                    section: current_section.clone(),
                    key: key.trim().to_string(),
                    value: value.trim().to_string(),
                });
            }
        }
    }
    Ok(tweaks)
}

/// Apply INI tweaks from all enabled mods to game INI files.
/// Mods are processed in priority order (last wins).
pub fn apply_ini_tweaks(tweaks: &[IniTweak], game_ini_dir: &Path) -> Result<usize> {
    let mut count = 0;
    for tweak in tweaks {
        let ini_path = game_ini_dir.join(&tweak.ini_file);
        if ini_path.exists() {
            super::ini::patch_ini(&ini_path, &tweak.section, &tweak.key, &tweak.value)?;
            count += 1;
        }
    }
    if count > 0 {
        info!(count, "applied INI tweaks");
    }
    Ok(count)
}
