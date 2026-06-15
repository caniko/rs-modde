use std::collections::HashMap;

use super::ModSafety;

/// Content types a game can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentCategory {
    Plugin,    // .esp, .esm, .esl
    Texture,   // .dds, .png, .tga
    Mesh,      // .nif
    Sound,     // .wav, .xwm, .fuz
    Script,    // .pex, .psc, .reds, .lua
    Interface, // .swf
    Archive,   // .bsa, .ba2, .archive
    Config,    // .ini, .json, .yaml, .xml
    Binary,    // .dll
    Other,
}

impl ContentCategory {
    /// Human-readable label for display.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            ContentCategory::Plugin => "plugins",
            ContentCategory::Texture => "textures",
            ContentCategory::Mesh => "meshes",
            ContentCategory::Sound => "sounds",
            ContentCategory::Script => "scripts",
            ContentCategory::Interface => "interfaces",
            ContentCategory::Archive => "archives",
            ContentCategory::Config => "configs",
            ContentCategory::Binary => "binaries",
            ContentCategory::Other => "other",
        }
    }

    /// Display order (lower = shown first).
    #[must_use]
    pub fn order(self) -> u8 {
        match self {
            ContentCategory::Plugin => 0,
            ContentCategory::Script => 1,
            ContentCategory::Binary => 2,
            ContentCategory::Texture => 3,
            ContentCategory::Mesh => 4,
            ContentCategory::Sound => 5,
            ContentCategory::Interface => 6,
            ContentCategory::Archive => 7,
            ContentCategory::Config => 8,
            ContentCategory::Other => 9,
        }
    }
}

/// Summary of content types found in a mod.
#[derive(Debug, Clone, Default)]
pub struct ContentSummary {
    pub counts: HashMap<ContentCategory, usize>,
}

impl ContentSummary {
    /// Return counts sorted by display order, excluding zero counts.
    #[must_use]
    pub fn sorted_counts(&self) -> Vec<(ContentCategory, usize)> {
        let mut entries: Vec<_> = self
            .counts
            .iter()
            .filter(|(_, count)| **count > 0)
            .map(|(cat, count)| (*cat, *count))
            .collect();
        entries.sort_by_key(|(cat, _)| cat.order());
        entries
    }

    /// Format as a human-readable string like "5 textures, 2 meshes, 1 plugin".
    #[must_use]
    pub fn display_string(&self) -> String {
        let parts: Vec<String> = self
            .sorted_counts()
            .iter()
            .map(|(cat, count)| format!("{} {}", count, cat.label()))
            .collect();
        if parts.is_empty() {
            "No files".to_string()
        } else {
            parts.join(", ")
        }
    }
}
/// Configuration for extension-based mod classification.
///
/// Both Bethesda and Cyberpunk games classify mods by scanning file extensions
/// (and optionally directory paths). This struct captures the game-specific
/// lists so the shared walker can be reused via static dispatch.
pub struct ModClassifyConfig {
    /// File extensions that indicate save-breaking content (lowercase, no dot).
    pub save_breaking_ext: &'static [&'static str],
    /// File extensions that indicate cosmetic-only content (lowercase, no dot).
    pub cosmetic_ext: &'static [&'static str],
    /// Directory path fragments (relative, `/`-separated) that signal save-breaking content.
    /// Checked via `contains()` on the normalized relative path. Empty slice to skip.
    pub save_breaking_dirs: &'static [&'static str],
}

/// Classify a mod by walking its directory and checking file extensions / directory paths
/// against the provided configuration. Returns early on the first save-breaking indicator.
#[must_use]
pub fn classify_mod_by_content(mod_dir: &std::path::Path, config: &ModClassifyConfig) -> ModSafety {
    if !mod_dir.exists() {
        return ModSafety::Unknown;
    }

    let mut has_any_file = false;
    let mut has_cosmetic = false;

    let mut stack = vec![mod_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                if !config.save_breaking_dirs.is_empty() {
                    let rel = path.strip_prefix(mod_dir).unwrap_or(&path);
                    let rel_normalized = rel.to_string_lossy().to_lowercase().replace('\\', "/");
                    for &pattern in config.save_breaking_dirs {
                        if rel_normalized.contains(pattern) {
                            return ModSafety::SaveBreaking;
                        }
                    }
                }
                stack.push(path);
                continue;
            }

            has_any_file = true;

            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if config.save_breaking_ext.contains(&ext_lower.as_str()) {
                    return ModSafety::SaveBreaking;
                }
                if config.cosmetic_ext.contains(&ext_lower.as_str()) {
                    has_cosmetic = true;
                }
            }
        }
    }

    if has_cosmetic && has_any_file {
        ModSafety::SaveSafe
    } else {
        ModSafety::Unknown
    }
}
