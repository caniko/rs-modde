use std::path::Path;

use anyhow::Result;
use smallvec::SmallVec;

use crate::save_patterns::{CaptureSummary, PatternSaveTracker, PrefixSaveRule};
use crate::traits::{DetectedSave, SaveTracker};

pub struct CyberpunkSaveTracker;

pub static CYBERPUNK_SAVE_TRACKER: CyberpunkSaveTracker = CyberpunkSaveTracker;

/// Prefixes used by Cyberpunk 2077 save directories.
const SAVE_PREFIXES: &[PrefixSaveRule] = &[
    PrefixSaveRule {
        prefix: "ManualSave-",
        category: "manual",
    },
    PrefixSaveRule {
        prefix: "AutoSave-",
        category: "auto",
    },
    PrefixSaveRule {
        prefix: "QuickSave-",
        category: "quick",
    },
    PrefixSaveRule {
        prefix: "PointOfNoReturn-",
        category: "point-of-no-return",
    },
];

const CYBERPUNK_PATTERN_TRACKER: PatternSaveTracker = PatternSaveTracker {
    prefix_rules: SAVE_PREFIXES,
    file_extensions: &[],
    default_category: "manual",
    recursive: false,
    exclude_patterns: &["user.gls"],
    label_extractor: extract_label,
    summary: CaptureSummary::ByCategory,
};

impl SaveTracker for CyberpunkSaveTracker {
    fn save_patterns(&self) -> SmallVec<[String; 2]> {
        CYBERPUNK_PATTERN_TRACKER.save_patterns()
    }

    fn detect_saves(&self, save_dir: &Path) -> Result<Vec<DetectedSave>> {
        CYBERPUNK_PATTERN_TRACKER.detect_saves(save_dir)
    }

    fn exclude_patterns(&self) -> SmallVec<[String; 2]> {
        CYBERPUNK_PATTERN_TRACKER.exclude_patterns()
    }

    fn describe_capture(&self, saves: &[DetectedSave]) -> String {
        CYBERPUNK_PATTERN_TRACKER.describe_capture(saves)
    }
}

/// Try to extract a human-readable label from save metadata.
///
/// Cyberpunk saves with `NamedSaves` may have a metadata.9.json containing
/// a custom name. Falls back to the directory name.
fn extract_label(save_path: &Path, dir_name: &str) -> Option<String> {
    // Try NamedSaves metadata (metadata.9.json)
    let meta_path = save_path.join("metadata.9.json");
    if meta_path.exists()
        && let Ok(content) = std::fs::read_to_string(&meta_path)
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
        && let Some(name) = json
            .get("customName")
            .or_else(|| json.get("name"))
            .and_then(|v| v.as_str())
        && !name.is_empty()
    {
        return Some(name.to_string());
    }

    // Fall back to directory name (strip prefix and numeric suffix for readability)
    Some(dir_name.to_string())
}
