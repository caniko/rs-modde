use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context;

use crate::traits::{DiscoveredFile, DiscoveredMod, ModSource, walk_files_relative};

#[derive(Debug, Clone, Copy)]
pub struct DirectoryModRule {
    pub rel_dir: &'static str,
    pub mod_id_prefix: &'static str,
    pub source_location: &'static str,
    pub confidence: f64,
    pub marker_file: Option<&'static str>,
    pub marker_confidence: Option<f64>,
}

impl DirectoryModRule {
    pub fn scan(self, install: &Path, out: &mut Vec<DiscoveredMod>) -> anyhow::Result<()> {
        let dir = install.join(self.rel_dir);
        if !dir.is_dir() {
            return Ok(());
        }

        for entry in std::fs::read_dir(&dir)
            .with_context(|| format!("failed to read directory: {}", dir.display()))?
            .flatten()
        {
            if !entry.path().is_dir() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            let files = walk_files_relative(install, &entry.path());
            if files.is_empty() {
                continue;
            }

            let confidence = self
                .marker_file
                .filter(|marker| entry.path().join(marker).exists())
                .and(self.marker_confidence)
                .unwrap_or(self.confidence);

            out.push(DiscoveredMod {
                mod_id: format!("{}/{name}", self.mod_id_prefix),
                display_name: name,
                version: None,
                files,
                source: ModSource::Filesystem {
                    location: self.source_location.into(),
                },
                confidence,
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SingleFileModRule {
    pub rel_dir: &'static str,
    pub extension: &'static str,
    pub ignored_prefixes: &'static [&'static str],
    pub mod_id_prefix: &'static str,
    pub source_location: &'static str,
    pub confidence: f64,
}

impl SingleFileModRule {
    pub fn scan(self, install: &Path, out: &mut Vec<DiscoveredMod>) -> anyhow::Result<()> {
        let dir = install.join(self.rel_dir);
        if !dir.is_dir() {
            return Ok(());
        }

        for entry in std::fs::read_dir(&dir)
            .with_context(|| format!("failed to read directory: {}", dir.display()))?
            .flatten()
        {
            let path = entry.path();
            if path.is_dir()
                || !path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case(self.extension))
            {
                continue;
            }

            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            let stem_lower = stem.to_lowercase();
            if self
                .ignored_prefixes
                .iter()
                .any(|prefix| stem_lower.starts_with(&prefix.to_lowercase()))
            {
                continue;
            }
            let size = path.metadata().map_or(0, |m| m.len());
            let rel = path
                .strip_prefix(install)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");

            out.push(DiscoveredMod {
                mod_id: format!("{}/{stem}", self.mod_id_prefix),
                display_name: stem.to_string(),
                version: None,
                files: vec![DiscoveredFile {
                    rel_path: rel,
                    size,
                }],
                source: ModSource::Filesystem {
                    location: self.source_location.into(),
                },
                confidence: self.confidence,
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FileGroupRule {
    pub rel_dir: &'static str,
    pub extensions: &'static [&'static str],
    pub mod_id_prefix: &'static str,
    pub source_location: &'static str,
    pub confidence: f64,
}

impl FileGroupRule {
    pub fn scan(self, install: &Path, out: &mut Vec<DiscoveredMod>) {
        let dir = install.join(self.rel_dir);
        self.scan_dir(install, &dir, out);
    }

    pub fn scan_dir(self, install: &Path, dir: &Path, out: &mut Vec<DiscoveredMod>) {
        if !dir.is_dir() {
            return;
        }

        let mut by_stem: BTreeMap<String, Vec<DiscoveredFile>> = BTreeMap::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                for file in walk_files_relative(install, &path) {
                    if let Some(stem) = stem_for_extensions(&file.rel_path, self.extensions) {
                        by_stem.entry(stem).or_default().push(file);
                    }
                }
                continue;
            }

            let Ok(meta) = path.metadata() else {
                continue;
            };
            let Ok(rel) = path.strip_prefix(install) else {
                continue;
            };
            let rel = rel.to_string_lossy().to_string();
            let Some(stem) = stem_for_extensions(&rel, self.extensions) else {
                continue;
            };

            by_stem.entry(stem).or_default().push(DiscoveredFile {
                rel_path: rel,
                size: meta.len(),
            });
        }

        for (stem, files) in by_stem {
            out.push(DiscoveredMod {
                mod_id: format!("{}/{stem}", self.mod_id_prefix),
                display_name: stem,
                version: None,
                files,
                source: ModSource::Filesystem {
                    location: self.source_location.into(),
                },
                confidence: self.confidence,
            });
        }
    }
}

fn stem_for_extensions(rel: &str, extensions: &[&str]) -> Option<String> {
    let lower = rel.to_lowercase();
    if !extensions
        .iter()
        .any(|ext| lower.ends_with(&format!(".{ext}")))
    {
        return None;
    }
    Path::new(rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(std::string::ToString::to_string)
}
