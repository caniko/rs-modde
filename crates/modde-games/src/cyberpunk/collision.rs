//! Cyberpunk 2077 collision classifier.
//!
//! Cyberpunk `.archive` files use a proprietary format that is not yet
//! reverse-engineered in this codebase, so `index_archive` returns an empty
//! list. Collisions are detected at the loose-file level.

use std::path::Path;

use anyhow::Result;

use modde_core::collision::{CollisionClassifier, CollisionSeverity};

use crate::policies::CollisionPolicy;

const CYBERPUNK_COLLISION_SEVERITIES: &[(&str, CollisionSeverity)] = &[
    ("reds", CollisionSeverity::Dangerous),
    ("lua", CollisionSeverity::Dangerous),
    ("tweak", CollisionSeverity::Dangerous),
    ("xl", CollisionSeverity::Dangerous),
    ("yaml", CollisionSeverity::Dangerous),
    ("yml", CollisionSeverity::Dangerous),
    ("dll", CollisionSeverity::Dangerous),
    ("ini", CollisionSeverity::Config),
    ("cfg", CollisionSeverity::Config),
    ("json", CollisionSeverity::Config),
    ("toml", CollisionSeverity::Config),
    ("archive", CollisionSeverity::Cosmetic),
    ("png", CollisionSeverity::Cosmetic),
    ("jpg", CollisionSeverity::Cosmetic),
    ("dds", CollisionSeverity::Cosmetic),
    ("tga", CollisionSeverity::Cosmetic),
];

const CYBERPUNK_COLLISION_POLICY: CollisionPolicy = CollisionPolicy {
    archive_extensions: &[],
    severities: CYBERPUNK_COLLISION_SEVERITIES,
};

/// Collision classifier for Cyberpunk 2077.
pub struct CyberpunkCollisionClassifier;

impl CollisionClassifier for CyberpunkCollisionClassifier {
    fn index_archive(&self, _archive_path: &Path) -> Result<Vec<(String, u64)>> {
        // Cyberpunk .archive format is not yet supported for content listing.
        Ok(Vec::new())
    }

    fn classify_severity(&self, file_path: &str) -> CollisionSeverity {
        CYBERPUNK_COLLISION_POLICY.classify_severity(file_path)
    }

    fn archive_extensions(&self) -> &[&str] {
        // Cyberpunk .archive files cannot be indexed yet
        CYBERPUNK_COLLISION_POLICY.archive_extensions
    }
}
