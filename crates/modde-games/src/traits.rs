use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Result;
use smallvec::SmallVec;

/// Whether a mod is safe to add/remove without breaking existing saves.
///
/// Mods that alter game logic (scripts, gameplay tweaks, new items/quests)
/// will corrupt or break saves if removed mid-playthrough. Cosmetic mods
/// (textures, meshes, UI themes) can be freely toggled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModSafety {
    /// Alters game logic — removing this mod will break saves that depend on it.
    /// Examples: REDscript mods, CET lua scripts, .tweak overrides, ESP/ESM plugins.
    SaveBreaking,
    /// Cosmetic only — safe to add/remove without affecting saves.
    /// Examples: texture replacers, mesh swaps, UI reskins.
    SaveSafe,
    /// Cannot determine automatically (e.g. mod not installed locally, or mixed content).
    /// Treated as `SaveBreaking` for safety when computing fingerprints.
    Unknown,
}

impl ModSafety {
    /// Returns `true` if this mod should be included in save fingerprints.
    pub fn affects_saves(self) -> bool {
        matches!(self, ModSafety::SaveBreaking | ModSafety::Unknown)
    }
}

/// Trait implemented by each supported game.
pub trait GamePlugin: Send + Sync {
    /// Unique game identifier (e.g. "skyrim-se").
    fn game_id(&self) -> &str;

    /// Human-readable display name.
    fn display_name(&self) -> &str;

    /// Attempt to detect the game's install location.
    /// Default: delegates to `detection::find_game_install(self.game_id())`.
    fn detect_install(&self) -> Option<PathBuf> {
        crate::detection::find_game_install(self.game_id())
    }

    /// Return the mod directory relative to the install path.
    fn mod_directory(&self, install: &Path) -> PathBuf;

    /// Deploy staged mods into the game's mod directory.
    /// Default: recursive symlink farm via `modde_core::fs::deploy_symlinks`.
    fn deploy(&self, staging: &Path, target: &Path) -> Result<()> {
        modde_core::fs::deploy_symlinks(staging, target)
    }

    /// Run any post-deployment steps (e.g. REDmod deploy).
    fn post_deploy(&self, _install: &Path) -> Result<()> {
        Ok(())
    }

    /// Return the save directory for this game, if known.
    fn save_directory(&self) -> Option<PathBuf> {
        None
    }

    /// Classify whether a mod is save-breaking based on its installed content.
    ///
    /// `mod_dir` is the path to the mod's staging directory. The game plugin
    /// inspects the files within to determine if the mod alters game logic
    /// (scripts, plugins, tweaks) or is purely cosmetic (textures, meshes).
    ///
    /// Default: `Unknown` (conservative — included in fingerprints).
    fn classify_mod(&self, _mod_dir: &Path) -> ModSafety {
        ModSafety::Unknown
    }

    /// Scan the game directory for proxy/hook DLLs that need Wine `n,b` overrides.
    ///
    /// Returns DLL base names (without extension) that should be added to
    /// `WINEDLLOVERRIDES` as `name=n,b` so Wine loads the native version
    /// instead of its built-in stub.
    fn wine_dll_overrides(&self, _game_dir: &Path) -> SmallVec<[String; 4]> {
        SmallVec::new()
    }

    /// Scan the staging directory for proxy DLLs that mods deploy.
    /// This catches DLLs that may have been deleted by other tools (e.g. fgmod)
    /// from the game directory but are still needed.
    fn wine_dll_overrides_from_staging(&self, _staging: &Path) -> SmallVec<[String; 4]> {
        SmallVec::new()
    }

    /// Return the directory containing the game executable, relative to the install root.
    /// Used to locate proxy DLLs that need Wine overrides.
    fn executable_dir(&self, install: &Path) -> PathBuf {
        install.to_path_buf()
    }

    // ── DRY trait methods (generic → game-specific via data) ─────

    /// INI file names managed per-profile (e.g., ["Skyrim.ini", "SkyrimPrefs.ini"]).
    fn ini_file_names(&self) -> &[&str] { &[] }

    /// Archive file extensions this game uses (e.g., ["bsa", "ba2"]).
    fn archive_extensions(&self) -> &[&str] { &[] }

    /// Whether this game has a plugin/load order system (ESP/ESM/ESL).
    fn has_plugin_system(&self) -> bool { false }

    /// Steam app ID for Proton prefix path resolution.
    fn steam_app_id_u32(&self) -> Option<u32> { None }

    /// Game folder name in Proton's AppData/Local for plugins.txt.
    fn plugins_txt_folder(&self) -> Option<&str> { None }

    /// Nexus Mods game domain name for API calls.
    fn nexus_game_domain(&self) -> Option<&str> { None }
}

/// A detected save file or directory within a game's save directory.
#[derive(Debug, Clone)]
pub struct DetectedSave {
    /// Path relative to the game's save directory.
    pub rel_path: PathBuf,
    /// Category: "manual", "auto", "quick", "point-of-no-return", etc.
    /// Uses `Cow<'static, str>` because categories are almost always
    /// static string literals, avoiding heap allocation in the common case.
    pub category: Cow<'static, str>,
    /// Human-readable label (e.g. custom name from NamedSaves).
    pub label: Option<String>,
    /// Last modification time.
    pub modified: SystemTime,
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

/// Game-specific save detection and classification.
///
/// Implemented per-game alongside `GamePlugin`. The core `SaveManager` handles
/// the git vault; this trait tells it *what* to look for and how to describe it.
pub trait SaveTracker: Send + Sync {
    /// Glob patterns matching save entries in the save directory.
    /// Typically 1–3 patterns per game; `SmallVec<[_; 2]>` avoids heap allocation.
    fn save_patterns(&self) -> SmallVec<[String; 2]>;

    /// Scan the save directory and return all detected saves with classification.
    fn detect_saves(&self, save_dir: &Path) -> Result<Vec<DetectedSave>>;

    /// Patterns to exclude from auto-capture triggers (files that exist in
    /// the save dir but aren't actual saves, e.g. global settings).
    /// Typically 0–2 patterns; `SmallVec<[_; 2]>` avoids heap allocation.
    fn exclude_patterns(&self) -> SmallVec<[String; 2]> {
        SmallVec::new()
    }

    /// Generate a human-readable commit message for a capture.
    fn describe_capture(&self, saves: &[DetectedSave]) -> String {
        match saves.len() {
            0 => "capture: no new saves".into(),
            1 => {
                let s = &saves[0];
                let name = s.label.as_deref()
                    .unwrap_or_else(|| s.rel_path.to_str().unwrap_or("unknown"));
                format!("capture: {} [{}]", name, s.category)
            }
            n => format!("capture: {} saves", n),
        }
    }
}
