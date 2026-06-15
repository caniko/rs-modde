use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Result;
use smallvec::SmallVec;

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
