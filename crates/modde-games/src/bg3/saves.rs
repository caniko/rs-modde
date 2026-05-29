//! Save detection for Baldur's Gate 3.

use std::path::Path;

use anyhow::Result;
use smallvec::SmallVec;

use crate::save_patterns::{CaptureSummary, PatternSaveTracker};
use crate::traits::{DetectedSave, SaveTracker};

/// [`SaveTracker`] for Baldur's Gate 3 save directories.
pub struct Bg3SaveTracker;

pub static BG3_SAVE_TRACKER: Bg3SaveTracker = Bg3SaveTracker;

const TRACKER: PatternSaveTracker = PatternSaveTracker {
    prefix_rules: &[],
    file_extensions: &["lsv"],
    default_category: "manual",
    recursive: true,
    exclude_patterns: &[],
    label_extractor: |_, rel| Some(rel.to_string()),
    summary: CaptureSummary::ByCategory,
};

impl SaveTracker for Bg3SaveTracker {
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
