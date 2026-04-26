//! Cyberpunk 2077 collision classifier.
//!
//! Cyberpunk `.archive` files use a proprietary format that is not yet
//! reverse-engineered in this codebase, so `index_archive` returns an empty
//! list. Collisions are detected at the loose-file level.

use std::path::Path;

use anyhow::Result;

use modde_core::collision::{CollisionClassifier, CollisionSeverity};

/// Collision classifier for Cyberpunk 2077.
pub struct CyberpunkCollisionClassifier;

impl CollisionClassifier for CyberpunkCollisionClassifier {
    fn index_archive(&self, _archive_path: &Path) -> Result<Vec<(String, u64)>> {
        // Cyberpunk .archive format is not yet supported for content listing.
        Ok(Vec::new())
    }

    fn classify_severity(&self, file_path: &str) -> CollisionSeverity {
        let ext = file_path.rsplit('.').next().unwrap_or("").to_lowercase();

        match ext.as_str() {
            // Scripts, tweaks, DLLs — save-breaking / dangerous
            "reds" | "lua" | "tweak" | "xl" | "yaml" | "yml" | "dll" => {
                CollisionSeverity::Dangerous
            }
            // Config files
            "ini" | "cfg" | "json" | "toml" => CollisionSeverity::Config,
            // Archives, textures — cosmetic
            "archive" | "png" | "jpg" | "dds" | "tga" => CollisionSeverity::Cosmetic,
            _ => CollisionSeverity::Unknown,
        }
    }

    fn archive_extensions(&self) -> &[&str] {
        // Cyberpunk .archive files cannot be indexed yet
        &[]
    }
}
