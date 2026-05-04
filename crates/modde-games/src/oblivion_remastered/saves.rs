use std::path::Path;

use anyhow::Result;
use smallvec::SmallVec;

use crate::save_patterns::{CaptureSummary, PatternSaveTracker};
use crate::traits::{DetectedSave, SaveTracker};

pub struct OblivionRemasteredSaveTracker;

pub static OBLIVION_REMASTERED_SAVE_TRACKER: OblivionRemasteredSaveTracker =
    OblivionRemasteredSaveTracker;

const TRACKER: PatternSaveTracker = PatternSaveTracker {
    prefix_rules: &[],
    file_extensions: &["ess", "sav"],
    default_category: "manual",
    recursive: false,
    exclude_patterns: &["*.bak"],
    label_extractor: |_, rel| Some(rel.to_string()),
    summary: CaptureSummary::ByCategory,
};

impl SaveTracker for OblivionRemasteredSaveTracker {
    fn save_patterns(&self) -> SmallVec<[String; 2]> {
        TRACKER.save_patterns()
    }

    fn detect_saves(&self, save_dir: &Path) -> Result<Vec<DetectedSave>> {
        TRACKER.detect_saves(save_dir)
    }

    fn exclude_patterns(&self) -> SmallVec<[String; 2]> {
        TRACKER.exclude_patterns()
    }

    fn describe_capture(&self, saves: &[DetectedSave]) -> String {
        TRACKER.describe_capture(saves)
    }
}
