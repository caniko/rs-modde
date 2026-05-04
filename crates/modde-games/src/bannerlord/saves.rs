use std::path::Path;

use anyhow::Result;
use smallvec::SmallVec;

use crate::save_patterns::{CaptureSummary, PatternSaveTracker};
use crate::traits::{DetectedSave, SaveTracker};

pub struct BannerlordSaveTracker;

pub static BANNERLORD_SAVE_TRACKER: BannerlordSaveTracker = BannerlordSaveTracker;

const TRACKER: PatternSaveTracker = PatternSaveTracker {
    prefix_rules: &[],
    file_extensions: &["sav"],
    default_category: "campaign",
    recursive: false,
    exclude_patterns: &[],
    label_extractor: |_, rel| Some(rel.to_string()),
    summary: CaptureSummary::ByCategory,
};

impl SaveTracker for BannerlordSaveTracker {
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
