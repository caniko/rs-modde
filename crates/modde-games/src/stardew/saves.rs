use std::path::Path;

use anyhow::Result;
use smallvec::SmallVec;

use crate::save_patterns::{CaptureSummary, PatternSaveTracker};
use crate::traits::{DetectedSave, SaveTracker};

pub struct StardewSaveTracker;

pub static STARDEW_SAVE_TRACKER: StardewSaveTracker = StardewSaveTracker;

const TRACKER: PatternSaveTracker = PatternSaveTracker {
    prefix_rules: &[],
    file_extensions: &[],
    default_category: "farm",
    recursive: false,
    exclude_patterns: &["startup_preferences"],
    label_extractor: |_, rel| Some(rel.to_string()),
    summary: CaptureSummary::Default,
};

impl SaveTracker for StardewSaveTracker {
    fn save_patterns(&self) -> SmallVec<[String; 2]> {
        smallvec::smallvec!["*_*".into()]
    }

    fn detect_saves(&self, save_dir: &Path) -> Result<Vec<DetectedSave>> {
        let mut saves = Vec::new();
        if !save_dir.exists() {
            return Ok(saves);
        }
        for entry in std::fs::read_dir(save_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.contains('_') {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            saves.push(DetectedSave {
                rel_path: name.clone().into(),
                category: std::borrow::Cow::Borrowed("farm"),
                label: Some(name),
                modified,
            });
        }
        saves.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(saves)
    }

    fn exclude_patterns(&self) -> SmallVec<[String; 2]> {
        TRACKER.exclude_patterns()
    }

    fn describe_capture(&self, saves: &[DetectedSave]) -> String {
        TRACKER.describe_capture(saves)
    }
}
