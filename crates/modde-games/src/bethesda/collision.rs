//! Bethesda game collision classifier.
//!
//! Uses [`ArchiveIndex`] to read BSA/BA2 file listings and classifies
//! collision severity based on file extensions.

use std::path::Path;

use anyhow::Result;

use modde_core::collision::{CollisionClassifier, CollisionSeverity};

use crate::policies::CollisionPolicy;

use super::archive_index::ArchiveIndex;

const BETHESDA_COLLISION_SEVERITIES: &[(&str, CollisionSeverity)] = &[
    ("esp", CollisionSeverity::Dangerous),
    ("esm", CollisionSeverity::Dangerous),
    ("esl", CollisionSeverity::Dangerous),
    ("pex", CollisionSeverity::Dangerous),
    ("dll", CollisionSeverity::Dangerous),
    ("psc", CollisionSeverity::Dangerous),
    ("ini", CollisionSeverity::Config),
    ("cfg", CollisionSeverity::Config),
    ("json", CollisionSeverity::Config),
    ("toml", CollisionSeverity::Config),
    ("xml", CollisionSeverity::Config),
    ("dds", CollisionSeverity::Cosmetic),
    ("png", CollisionSeverity::Cosmetic),
    ("tga", CollisionSeverity::Cosmetic),
    ("jpg", CollisionSeverity::Cosmetic),
    ("nif", CollisionSeverity::Cosmetic),
    ("hkx", CollisionSeverity::Cosmetic),
    ("fuz", CollisionSeverity::Cosmetic),
    ("wav", CollisionSeverity::Cosmetic),
    ("xwm", CollisionSeverity::Cosmetic),
    ("swf", CollisionSeverity::Cosmetic),
    ("btr", CollisionSeverity::Cosmetic),
    ("bto", CollisionSeverity::Cosmetic),
    ("btt", CollisionSeverity::Cosmetic),
    ("bsa", CollisionSeverity::Cosmetic),
    ("ba2", CollisionSeverity::Cosmetic),
];

const BETHESDA_COLLISION_POLICY: CollisionPolicy = CollisionPolicy {
    archive_extensions: &["bsa", "ba2"],
    severities: BETHESDA_COLLISION_SEVERITIES,
};

/// Collision classifier for Bethesda games (Skyrim, Fallout, Starfield).
pub struct BethesdaCollisionClassifier;

impl CollisionClassifier for BethesdaCollisionClassifier {
    fn index_archive(&self, archive_path: &Path) -> Result<Vec<(String, u64)>> {
        let index = ArchiveIndex::read(archive_path)?;
        Ok(index.files.into_iter().map(|f| (f.path, f.size)).collect())
    }

    fn classify_severity(&self, file_path: &str) -> CollisionSeverity {
        BETHESDA_COLLISION_POLICY.classify_severity(file_path)
    }

    fn archive_extensions(&self) -> &[&str] {
        BETHESDA_COLLISION_POLICY.archive_extensions
    }
}
