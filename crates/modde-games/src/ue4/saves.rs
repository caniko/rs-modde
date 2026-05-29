//! Save detection patterns for Unreal Engine 4 games.

use std::path::Path;

use crate::save_patterns::{CaptureSummary, PatternSaveTracker};

pub static STELLAR_BLADE_SAVE_TRACKER: PatternSaveTracker = PatternSaveTracker {
    prefix_rules: &[],
    file_extensions: &["sav"],
    default_category: "manual",
    recursive: true,
    exclude_patterns: &[],
    label_extractor: save_file_label,
    summary: CaptureSummary::ByCategory,
};

pub static SUBNAUTICA2_SAVE_TRACKER: PatternSaveTracker = PatternSaveTracker {
    prefix_rules: &[],
    file_extensions: &["sav"],
    default_category: "manual",
    recursive: true,
    exclude_patterns: &[],
    label_extractor: save_file_label,
    summary: CaptureSummary::ByCategory,
};

fn save_file_label(path: &Path, rel_name: &str) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(std::string::ToString::to_string)
        .or_else(|| Some(rel_name.to_string()))
}
