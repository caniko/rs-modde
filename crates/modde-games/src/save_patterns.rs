use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use smallvec::SmallVec;

use crate::traits::{DetectedSave, SaveTracker};

pub type SaveLabelExtractor = fn(&Path, &str) -> Option<String>;

#[derive(Debug, Clone, Copy)]
pub struct PrefixSaveRule {
    pub prefix: &'static str,
    pub category: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub enum CaptureSummary {
    Default,
    ByCategory,
}

#[derive(Debug, Clone, Copy)]
pub struct PatternSaveTracker {
    pub prefix_rules: &'static [PrefixSaveRule],
    pub file_extensions: &'static [&'static str],
    pub default_category: &'static str,
    pub recursive: bool,
    pub exclude_patterns: &'static [&'static str],
    pub label_extractor: SaveLabelExtractor,
    pub summary: CaptureSummary,
}

impl PatternSaveTracker {
    #[must_use]
    pub fn save_patterns(self) -> SmallVec<[String; 2]> {
        let mut patterns: SmallVec<[String; 2]> = self
            .prefix_rules
            .iter()
            .map(|rule| format!("{}*", rule.prefix))
            .collect();
        patterns.extend(self.file_extensions.iter().map(|ext| {
            if self.recursive {
                format!("**/*.{ext}")
            } else {
                format!("*.{ext}")
            }
        }));
        patterns
    }

    #[must_use]
    pub fn exclude_patterns(self) -> SmallVec<[String; 2]> {
        self.exclude_patterns
            .iter()
            .map(|pattern| (*pattern).to_string())
            .collect()
    }

    pub fn detect_saves(self, save_dir: &Path) -> Result<Vec<DetectedSave>> {
        let mut saves = Vec::new();
        if !save_dir.exists() {
            return Ok(saves);
        }

        self.detect_in_dir(save_dir, save_dir, &mut saves)?;

        saves.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(saves)
    }

    fn detect_in_dir(self, base: &Path, dir: &Path, saves: &mut Vec<DetectedSave>) -> Result<()> {
        for entry in std::fs::read_dir(dir)
            .with_context(|| format!("failed to read directory: {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let metadata = entry.metadata()?;
            if metadata.is_dir() && self.recursive {
                self.detect_in_dir(base, &path, saves)?;
            }

            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let extension_matches =
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        self.file_extensions
                            .iter()
                            .any(|candidate| ext.eq_ignore_ascii_case(candidate))
                    });
            let category = self
                .prefix_rules
                .iter()
                .filter(|_| self.file_extensions.is_empty() || extension_matches)
                .find(|rule| name_str.starts_with(rule.prefix))
                .map(|rule| rule.category)
                .or_else(|| extension_matches.then_some(self.default_category));

            let Some(category) = category else {
                continue;
            };

            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let rel_path = path
                .strip_prefix(base)
                .map_or_else(|_| PathBuf::from(&name), PathBuf::from);
            let rel_name = rel_path.to_string_lossy();
            let label = (self.label_extractor)(&path, &rel_name);

            saves.push(DetectedSave {
                rel_path,
                category: Cow::Borrowed(category),
                label,
                modified,
            });
        }

        Ok(())
    }

    #[must_use]
    pub fn describe_capture(self, saves: &[DetectedSave]) -> String {
        match self.summary {
            CaptureSummary::Default => default_capture_summary(saves),
            CaptureSummary::ByCategory => category_capture_summary(saves),
        }
    }
}

impl SaveTracker for PatternSaveTracker {
    fn save_patterns(&self) -> SmallVec<[String; 2]> {
        PatternSaveTracker::save_patterns(*self)
    }

    fn detect_saves(&self, save_dir: &Path) -> Result<Vec<DetectedSave>> {
        PatternSaveTracker::detect_saves(*self, save_dir)
    }

    fn exclude_patterns(&self) -> SmallVec<[String; 2]> {
        PatternSaveTracker::exclude_patterns(*self)
    }

    fn describe_capture(&self, saves: &[DetectedSave]) -> String {
        PatternSaveTracker::describe_capture(*self, saves)
    }
}

fn default_capture_summary(saves: &[DetectedSave]) -> String {
    match saves.len() {
        0 => "capture: no new saves".into(),
        1 => {
            let save = &saves[0];
            let name = save
                .label
                .as_deref()
                .unwrap_or_else(|| save.rel_path.to_str().unwrap_or("unknown"));
            format!("capture: {} [{}]", name, save.category)
        }
        n => format!("capture: {n} saves"),
    }
}

fn category_capture_summary(saves: &[DetectedSave]) -> String {
    match saves.len() {
        0 | 1 => default_capture_summary(saves),
        n => {
            let mut categories = std::collections::BTreeMap::new();
            for save in saves {
                *categories.entry(&*save.category).or_insert(0u32) += 1;
            }
            let summary = categories
                .into_iter()
                .map(|(category, count)| format!("{count} {category}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("capture: {n} saves ({summary})")
        }
    }
}
