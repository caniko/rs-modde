use std::borrow::Cow;
use std::path::Path;
use std::time::SystemTime;

use anyhow::Result;
use smallvec::SmallVec;

use crate::traits::{DetectedSave, SaveTracker};

pub struct CyberpunkSaveTracker;

pub static CYBERPUNK_SAVE_TRACKER: CyberpunkSaveTracker = CyberpunkSaveTracker;

/// Prefixes used by Cyberpunk 2077 save directories.
const SAVE_PREFIXES: &[(&str, &str)] = &[
    ("ManualSave-", "manual"),
    ("AutoSave-", "auto"),
    ("QuickSave-", "quick"),
    ("PointOfNoReturn-", "point-of-no-return"),
];

impl SaveTracker for CyberpunkSaveTracker {
    fn save_patterns(&self) -> SmallVec<[String; 2]> {
        SAVE_PREFIXES.iter().map(|(p, _)| format!("{p}*")).collect()
    }

    fn detect_saves(&self, save_dir: &Path) -> Result<Vec<DetectedSave>> {
        let mut saves = Vec::new();

        if !save_dir.exists() {
            return Ok(saves);
        }

        for entry in std::fs::read_dir(save_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // Only track save directories/files matching known prefixes
            let Some((_, category)) = SAVE_PREFIXES
                .iter()
                .find(|(prefix, _)| name_str.starts_with(prefix))
            else {
                continue;
            };

            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);

            let label = extract_label(&entry.path(), &name_str);

            saves.push(DetectedSave {
                rel_path: name.into(),
                category: Cow::Borrowed(category),
                label,
                modified,
            });
        }

        // Sort newest first
        saves.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(saves)
    }

    fn exclude_patterns(&self) -> SmallVec<[String; 2]> {
        smallvec::smallvec!["user.gls".into()]
    }

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
            n => {
                // Summarize by category
                let mut cats = std::collections::HashMap::new();
                for s in saves {
                    *cats.entry(&*s.category).or_insert(0u32) += 1;
                }
                let summary: Vec<String> = cats
                    .into_iter()
                    .map(|(cat, count)| format!("{count} {cat}"))
                    .collect();
                format!("capture: {} saves ({})", n, summary.join(", "))
            }
        }
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
                    && !name.is_empty() {
                        return Some(name.to_string());
                    }

    // Fall back to directory name (strip prefix and numeric suffix for readability)
    Some(dir_name.to_string())
}
