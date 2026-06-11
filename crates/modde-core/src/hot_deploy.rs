//! Planning and filesystem helpers for experimental live VFS hot-deploys.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::collision::{CollisionClassifier, CollisionSeverity};
use crate::fs::{symlink, walk_files_relative};

/// One relative path whose deployed source changes during a hot-deploy patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotDeployChange {
    pub rel_path: String,
    pub before: Option<PathBuf>,
    pub after: Option<PathBuf>,
}

/// Minimal patch required to transform one materialized farm into another.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HotDeployPatch {
    pub changes: Vec<HotDeployChange>,
}

impl HotDeployPatch {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    #[must_use]
    pub fn added_count(&self) -> usize {
        self.changes
            .iter()
            .filter(|change| change.before.is_none() && change.after.is_some())
            .count()
    }

    #[must_use]
    pub fn removed_count(&self) -> usize {
        self.changes
            .iter()
            .filter(|change| change.before.is_some() && change.after.is_none())
            .count()
    }

    #[must_use]
    pub fn changed_count(&self) -> usize {
        self.changes
            .iter()
            .filter(|change| change.before.is_some() && change.after.is_some())
            .count()
    }

    /// Top-level path components touched by this patch.
    #[must_use]
    pub fn touched_roots(&self) -> BTreeSet<String> {
        self.changes
            .iter()
            .filter_map(|change| change.rel_path.split('/').next())
            .filter(|root| !root.is_empty())
            .map(str::to_string)
            .collect()
    }
}

/// Build the minimal path-level patch between two symlink-farm link maps.
#[must_use]
pub fn plan_patch(
    before: &HashMap<String, PathBuf>,
    after: &HashMap<String, PathBuf>,
) -> HotDeployPatch {
    let keys: BTreeSet<&str> = before
        .keys()
        .map(String::as_str)
        .chain(after.keys().map(String::as_str))
        .collect();
    let mut changes = Vec::new();

    for rel_path in keys {
        let before_source = before.get(rel_path);
        let after_source = after.get(rel_path);
        if before_source == after_source {
            continue;
        }
        changes.push(HotDeployChange {
            rel_path: rel_path.to_string(),
            before: before_source.cloned(),
            after: after_source.cloned(),
        });
    }

    HotDeployPatch { changes }
}

/// Validate that every changed deployed path is classified as cosmetic.
pub fn ensure_cosmetic_patch(
    patch: &HotDeployPatch,
    classifier: &dyn CollisionClassifier,
) -> Result<()> {
    for change in &patch.changes {
        let severity = classifier.classify_severity(&change.rel_path);
        if severity != CollisionSeverity::Cosmetic {
            bail!(
                "hot-deploy refused: patch touches '{}' classified as {}",
                change.rel_path,
                severity
            );
        }
    }
    Ok(())
}

/// Validate that the target mod itself contains only cosmetic files.
pub fn ensure_cosmetic_mod(mod_dir: &Path, classifier: &dyn CollisionClassifier) -> Result<usize> {
    if !mod_dir.is_dir() {
        bail!(
            "hot-deploy refused: store directory is missing: {}",
            mod_dir.display()
        );
    }

    let files = walk_files_relative(mod_dir)
        .with_context(|| format!("failed to inspect mod files in {}", mod_dir.display()))?;
    if files.is_empty() {
        bail!(
            "hot-deploy refused: mod has no files: {}",
            mod_dir.display()
        );
    }

    for (rel_path, _) in &files {
        let severity = classifier.classify_severity(rel_path);
        if severity != CollisionSeverity::Cosmetic {
            bail!(
                "hot-deploy refused: mod file '{}' is classified as {}",
                rel_path,
                severity
            );
        }
    }

    Ok(files.len())
}

/// Apply a path-level patch to the profile staging directory.
pub fn apply_patch_to_staging(staging: &Path, patch: &HotDeployPatch) -> Result<()> {
    std::fs::create_dir_all(staging)
        .with_context(|| format!("failed to create staging dir {}", staging.display()))?;

    for change in &patch.changes {
        let dst = staging.join(&change.rel_path);
        if dst.symlink_metadata().is_ok() {
            remove_path(&dst)?;
        }

        if let Some(source) = &change.after {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            symlink(source, &dst).with_context(|| {
                format!("failed to link {} -> {}", dst.display(), source.display())
            })?;
        }
    }

    Ok(())
}

/// Remove a file, symlink, or directory.
pub fn remove_path(path: &Path) -> Result<()> {
    let metadata = path
        .symlink_metadata()
        .with_context(|| format!("failed to stat {}", path.display()))?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        std::fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))
    } else {
        std::fs::remove_dir_all(path)
            .with_context(|| format!("failed to remove directory {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::{CollisionClassifier, CollisionSeverity};
    use std::path::Path;
    use tempfile::TempDir;

    struct TestClassifier;

    impl CollisionClassifier for TestClassifier {
        fn index_archive(&self, _archive_path: &Path) -> Result<Vec<(String, u64)>> {
            Ok(Vec::new())
        }

        fn classify_severity(&self, file_path: &str) -> CollisionSeverity {
            match file_path.rsplit('.').next().unwrap_or("") {
                "archive" | "dds" | "png" => CollisionSeverity::Cosmetic,
                "ini" => CollisionSeverity::Config,
                "lua" | "dll" | "reds" => CollisionSeverity::Dangerous,
                _ => CollisionSeverity::Unknown,
            }
        }

        fn archive_extensions(&self) -> &[&str] {
            &[]
        }
    }

    #[test]
    fn plan_patch_reports_add_remove_and_changed_paths() {
        let before = HashMap::from([
            ("a.dds".to_string(), PathBuf::from("/store/old/a.dds")),
            ("b.dds".to_string(), PathBuf::from("/store/old/b.dds")),
            ("c.dds".to_string(), PathBuf::from("/store/old/c.dds")),
        ]);
        let after = HashMap::from([
            ("a.dds".to_string(), PathBuf::from("/store/old/a.dds")),
            ("b.dds".to_string(), PathBuf::from("/store/new/b.dds")),
            ("d.dds".to_string(), PathBuf::from("/store/new/d.dds")),
        ]);

        let patch = plan_patch(&before, &after);

        assert_eq!(patch.added_count(), 1);
        assert_eq!(patch.removed_count(), 1);
        assert_eq!(patch.changed_count(), 1);
        assert_eq!(
            patch
                .changes
                .iter()
                .map(|change| change.rel_path.as_str())
                .collect::<Vec<_>>(),
            ["b.dds", "c.dds", "d.dds"]
        );
    }

    #[test]
    fn cosmetic_patch_gate_accepts_only_cosmetic_paths() {
        let classifier = TestClassifier;
        let cosmetic = HotDeployPatch {
            changes: vec![HotDeployChange {
                rel_path: "archive/pc/mod/body.archive".to_string(),
                before: None,
                after: Some(PathBuf::from("/store/body.archive")),
            }],
        };
        ensure_cosmetic_patch(&cosmetic, &classifier).unwrap();

        let dangerous = HotDeployPatch {
            changes: vec![HotDeployChange {
                rel_path: "r6/scripts/foo.reds".to_string(),
                before: None,
                after: Some(PathBuf::from("/store/foo.reds")),
            }],
        };
        assert!(ensure_cosmetic_patch(&dangerous, &classifier).is_err());
    }

    #[test]
    fn cosmetic_mod_gate_rejects_mixed_or_unknown_content() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("archive/pc/mod")).unwrap();
        std::fs::write(dir.path().join("archive/pc/mod/body.archive"), b"").unwrap();
        assert_eq!(ensure_cosmetic_mod(dir.path(), &TestClassifier).unwrap(), 1);

        std::fs::create_dir_all(dir.path().join("r6/scripts")).unwrap();
        std::fs::write(dir.path().join("r6/scripts/foo.reds"), b"").unwrap();
        assert!(ensure_cosmetic_mod(dir.path(), &TestClassifier).is_err());
    }
}
