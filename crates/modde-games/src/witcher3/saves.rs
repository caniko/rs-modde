//! Save detection for The Witcher 3.

use std::path::Path;

use anyhow::Result;
use smallvec::SmallVec;

use crate::save_patterns::{CaptureSummary, PatternSaveTracker};
use crate::traits::{DetectedSave, SaveTracker};

/// [`SaveTracker`] for The Witcher 3 save files.
pub struct Witcher3SaveTracker;

pub static WITCHER3_SAVE_TRACKER: Witcher3SaveTracker = Witcher3SaveTracker;

const TRACKER: PatternSaveTracker = PatternSaveTracker {
    prefix_rules: &[],
    file_extensions: &["sav"],
    default_category: "save",
    recursive: false,
    exclude_patterns: &["*.png"],
    label_extractor: |_, rel| Some(rel.to_string()),
    summary: CaptureSummary::ByCategory,
};

impl SaveTracker for Witcher3SaveTracker {
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
