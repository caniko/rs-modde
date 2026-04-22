//! Bethesda game collision classifier.
//!
//! Uses [`ArchiveIndex`] to read BSA/BA2 file listings and classifies
//! collision severity based on file extensions.

use std::path::Path;

use anyhow::Result;

use modde_core::collision::{CollisionClassifier, CollisionSeverity};

use super::archive_index::ArchiveIndex;

/// Collision classifier for Bethesda games (Skyrim, Fallout, Starfield).
pub struct BethesdaCollisionClassifier;

impl CollisionClassifier for BethesdaCollisionClassifier {
    fn index_archive(&self, archive_path: &Path) -> Result<Vec<(String, u64)>> {
        let index = ArchiveIndex::read(archive_path)?;
        Ok(index.files.into_iter().map(|f| (f.path, f.size)).collect())
    }

    fn classify_severity(&self, file_path: &str) -> CollisionSeverity {
        let ext = file_path.rsplit('.').next().unwrap_or("").to_lowercase();

        match ext.as_str() {
            // Scripts, plugins, DLLs — save-breaking / dangerous
            "esp" | "esm" | "esl" | "pex" | "dll" | "psc" => CollisionSeverity::Dangerous,
            // Config files — medium risk
            "ini" | "cfg" | "json" | "toml" | "xml" => CollisionSeverity::Config,
            // Textures, meshes, sounds, animations — cosmetic
            "dds" | "png" | "tga" | "jpg" | "nif" | "hkx" | "fuz" | "wav" | "xwm" | "swf"
            | "btr" | "bto" | "btt" => CollisionSeverity::Cosmetic,
            // BSA/BA2 themselves don't collide at file level — they contain files
            "bsa" | "ba2" => CollisionSeverity::Cosmetic,
            _ => CollisionSeverity::Unknown,
        }
    }

    fn archive_extensions(&self) -> &[&str] {
        &["bsa", "ba2"]
    }
}
