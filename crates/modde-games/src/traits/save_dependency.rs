use std::path::{Path, PathBuf};

use anyhow::Result;

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

/// The type of save-record dependency found while checking whether a mod can
/// be removed from an existing playthrough.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SaveDependencyKind {
    PluginRecord,
    PapyrusScript,
    ActiveScript,
    UnattachedInstance,
    UndefinedElement,
    ParseIncomplete,
}

impl SaveDependencyKind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            SaveDependencyKind::PluginRecord => "plugin record",
            SaveDependencyKind::PapyrusScript => "Papyrus script",
            SaveDependencyKind::ActiveScript => "active script",
            SaveDependencyKind::UnattachedInstance => "unattached instance",
            SaveDependencyKind::UndefinedElement => "undefined element",
            SaveDependencyKind::ParseIncomplete => "parse incomplete",
        }
    }
}

/// A single reason a save depends on the mod being removed.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveDependencyFinding {
    pub save_path: PathBuf,
    pub profile: Option<String>,
    pub dependency_kind: SaveDependencyKind,
    pub symbol: String,
    pub source_file: Option<String>,
    pub confidence: f32,
}

/// Aggregated report returned to CLI and UI removal gates.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveRemovalGateReport {
    pub mod_id: String,
    pub safety: ModSafety,
    pub analyzed_saves: usize,
    pub blocking_findings: Vec<SaveDependencyFinding>,
    pub warnings: Vec<String>,
}

impl SaveRemovalGateReport {
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        !self.blocking_findings.is_empty()
    }
}

/// Read-only analyzer used before removing mods from active playthroughs.
pub trait SaveDependencyAnalyzer: Send + Sync {
    fn analyze_mod_removal(
        &self,
        mod_id: &str,
        mod_dir: &Path,
        save_roots: &[PathBuf],
    ) -> Result<SaveRemovalGateReport>;
}

impl ModSafety {
    /// Returns `true` if this mod should be included in save fingerprints.
    #[must_use]
    pub fn affects_saves(self) -> bool {
        matches!(self, ModSafety::SaveBreaking | ModSafety::Unknown)
    }
}
