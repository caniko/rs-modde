use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Result;
use smallvec::SmallVec;

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

/// Whether a mod is safe to add/remove without breaking existing saves.
///
/// Mods that alter game logic (scripts, gameplay tweaks, new items/quests)
/// will corrupt or break saves if removed mid-playthrough. Cosmetic mods
/// (textures, meshes, UI themes) can be freely toggled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModSafety {
    /// Alters game logic — removing this mod will break saves that depend on it.
    /// Examples: `REDscript` mods, CET lua scripts, .tweak overrides, ESP/ESM plugins.
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
    #[must_use]
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

    /// Run any post-deployment steps (e.g. `REDmod` deploy).
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

    // ── DRY trait methods ─────────────────────────────────────────
    fn ini_file_names(&self) -> &[&str] {
        &[]
    }
    fn archive_extensions(&self) -> &[&str] {
        &[]
    }
    fn has_plugin_system(&self) -> bool {
        false
    }
    fn steam_app_id_u32(&self) -> Option<u32> {
        None
    }
    fn plugins_txt_folder(&self) -> Option<&str> {
        None
    }
    fn nexus_game_domain(&self) -> Option<&str> {
        None
    }

    /// Numeric Nexus game ID. Required by the GraphQL v2 API for
    /// browse/search queries (which take `gameId: Int`, not a domain
    /// string). Games that only speak REST can leave this `None`.
    fn nexus_game_id_u32(&self) -> Option<u32> {
        None
    }

    // ── Install-method detection (V8 installer pipeline) ────────

    /// Claim an extracted archive as a game-specific install method.
    ///
    /// Runs **before** the generic probes (FOMOD, BAIN, DLL overlay) in
    /// [`modde_core::installer::analyze`], so a game can authoritatively
    /// identify layouts it knows about — e.g. Cyberpunk recognizing a
    /// `REDmod` by `info.json` + `archives/` presence, or ENB for Bethesda.
    ///
    /// Return `None` to fall through to the generic probes.
    fn analyze_mod_archive(
        &self,
        _extracted_dir: &Path,
    ) -> Option<modde_core::installer::InstallMethod> {
        None
    }

    /// Decide whether an extracted archive drops cleanly into the game's
    /// mod dir without any staging (e.g. a Skyrim archive with a
    /// top-level `Data/` directory, or a Cyberpunk archive with `r6/`).
    ///
    /// Called as the last fallback by
    /// [`modde_core::installer::analyze`] — if this returns `true` the
    /// plan becomes `InstallMethod::BareExtract`, otherwise the analyzer
    /// falls through to [`InstallMethod::Unknown`] and the caller dumps
    /// a dossier for the skill path.
    fn recognizes_bare_layout(&self, _extracted_dir: &Path) -> bool {
        false
    }

    /// Classify a file extension into a content category.
    fn classify_extension(&self, ext: &str) -> ContentCategory {
        match ext {
            "esp" | "esm" | "esl" => ContentCategory::Plugin,
            "dds" | "png" | "tga" | "jpg" => ContentCategory::Texture,
            "nif" => ContentCategory::Mesh,
            "wav" | "xwm" | "fuz" | "mp3" | "ogg" => ContentCategory::Sound,
            "pex" | "psc" | "reds" | "lua" => ContentCategory::Script,
            "swf" => ContentCategory::Interface,
            "bsa" | "ba2" | "archive" => ContentCategory::Archive,
            "ini" | "json" | "yaml" | "xml" | "toml" => ContentCategory::Config,
            "dll" | "so" => ContentCategory::Binary,
            _ => ContentCategory::Other,
        }
    }

    /// Scan a mod directory and return a content summary.
    fn summarize_content(&self, mod_dir: &Path) -> ContentSummary {
        let mut summary = ContentSummary::default();
        let mut stack = vec![mod_dir.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    let cat = self.classify_extension(&ext.to_lowercase());
                    *summary.counts.entry(cat).or_insert(0) += 1;
                }
            }
        }
        summary
    }
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
    /// Human-readable label (e.g. custom name from `NamedSaves`).
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
                let name = s
                    .label
                    .as_deref()
                    .unwrap_or_else(|| s.rel_path.to_str().unwrap_or("unknown"));
                format!("capture: {} [{}]", name, s.category)
            }
            n => format!("capture: {n} saves"),
        }
    }
}

// ── Mod Scanner ─────────────────────────────────────────────────

pub struct ScanContext<'a> {
    pub install_dir: &'a Path,
}

#[derive(Debug, Clone)]
pub struct DiscoveredFile {
    pub rel_path: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub enum ModSource {
    Filesystem { location: String },
    Archive { archive_name: String },
}

#[derive(Debug, Clone)]
pub struct DiscoveredMod {
    pub mod_id: String,
    pub display_name: String,
    pub version: Option<String>,
    pub files: Vec<DiscoveredFile>,
    pub source: ModSource,
    pub confidence: f64,
}

pub trait ModScanner: Send + Sync {
    fn scan_directories(&self) -> &[&str];
    fn scan_filesystem(&self, ctx: &ScanContext<'_>) -> anyhow::Result<Vec<DiscoveredMod>>;

    /// Inverse of [`ModScanner::scan_filesystem`]'s `mod_id` scheme: given
    /// a `mod_id` this scanner would produce, return the filesystem footprint
    /// that mod owns (directory subtree or single file).
    ///
    /// Used by `modde_core::scanner::detect_stale_duplicates` to correlate
    /// profile rows with a Wabbajack manifest's install directives. The
    /// default impl returns `None`, which causes the dedup path to skip
    /// the row. Game plugins that want their filesystem-scanner rows to
    /// participate in dedup should override this.
    fn mod_id_footprint(&self, _mod_id: &str) -> Option<modde_core::scanner::ModFootprint> {
        None
    }
}

#[must_use]
pub fn walk_files_relative(base: &Path, dir: &Path) -> Vec<DiscoveredFile> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                result.extend(walk_files_relative(base, &path));
            } else if let Ok(meta) = path.metadata()
                && let Ok(rel) = path.strip_prefix(base) {
                    result.push(DiscoveredFile {
                        rel_path: rel.to_string_lossy().to_string(),
                        size: meta.len(),
                    });
                }
        }
    }
    result
}

#[must_use]
pub fn slug(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}
