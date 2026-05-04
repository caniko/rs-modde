use std::path::{Path, PathBuf};

use smallvec::SmallVec;

use modde_core::collision::{CollisionClassifier, CollisionSeverity};

use crate::traits::{ContentCategory, ModSafety};

/// Extension and directory rules used to classify installed mod content.
#[derive(Debug, Clone, Copy)]
pub struct ContentPolicy {
    pub save_breaking_ext: &'static [&'static str],
    pub cosmetic_ext: &'static [&'static str],
    pub save_breaking_dirs: &'static [&'static str],
    pub categories: &'static [(&'static str, ContentCategory)],
}

impl ContentPolicy {
    #[must_use]
    pub fn classify_extension(self, ext: &str) -> ContentCategory {
        self.categories
            .iter()
            .find_map(|(candidate, category)| (*candidate == ext).then_some(*category))
            .unwrap_or(ContentCategory::Other)
    }

    #[must_use]
    pub fn classify_mod(self, mod_dir: &Path) -> ModSafety {
        if !mod_dir.exists() {
            return ModSafety::Unknown;
        }

        let mut has_any_file = false;
        let mut has_cosmetic = false;
        let mut stack = vec![mod_dir.to_path_buf()];

        while let Some(dir) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if !self.save_breaking_dirs.is_empty() {
                        let rel = path.strip_prefix(mod_dir).unwrap_or(&path);
                        let rel = rel.to_string_lossy().to_lowercase().replace('\\', "/");
                        if self
                            .save_breaking_dirs
                            .iter()
                            .any(|pattern| rel.contains(pattern))
                        {
                            return ModSafety::SaveBreaking;
                        }
                    }
                    stack.push(path);
                    continue;
                }

                has_any_file = true;
                let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                    continue;
                };
                let ext = ext.to_lowercase();
                if self.save_breaking_ext.contains(&ext.as_str()) {
                    return ModSafety::SaveBreaking;
                }
                if self.cosmetic_ext.contains(&ext.as_str()) {
                    has_cosmetic = true;
                }
            }
        }

        if has_cosmetic && has_any_file {
            ModSafety::SaveSafe
        } else {
            ModSafety::Unknown
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BareLayoutPolicy {
    pub root_dirs: &'static [&'static str],
    pub root_file_exts: &'static [&'static str],
    pub case_insensitive_dirs: bool,
}

impl BareLayoutPolicy {
    #[must_use]
    pub fn recognizes(self, extracted_dir: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(extracted_dir) else {
            return false;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if self.case_insensitive_dirs {
                    let name = name.to_lowercase();
                    if self.root_dirs.iter().any(|candidate| *candidate == name) {
                        return true;
                    }
                } else if self.root_dirs.iter().any(|candidate| *candidate == name) {
                    return true;
                }
            } else if path.is_file()
                && let Some(ext) = path.extension().and_then(|e| e.to_str())
            {
                let ext = ext.to_lowercase();
                if self.root_file_exts.contains(&ext.as_str()) {
                    return true;
                }
            }
        }

        false
    }
}

#[derive(Debug, Clone, Copy)]
pub enum StagingDllSearch {
    DirectChildDirs,
    NestedModsBinX64,
}

#[derive(Debug, Clone, Copy)]
pub struct DllOverridePolicy {
    pub proxy_dlls: &'static [&'static str],
    pub staging_search: StagingDllSearch,
}

impl DllOverridePolicy {
    #[must_use]
    pub fn from_executable_dir(self, executable_dir: &Path) -> SmallVec<[String; 4]> {
        let mut out = SmallVec::new();
        for &name in self.proxy_dlls {
            if executable_dir.join(format!("{name}.dll")).exists() {
                out.push(name.to_string());
            }
        }
        out
    }

    #[must_use]
    pub fn from_staging(self, staging: &Path) -> SmallVec<[String; 4]> {
        match self.staging_search {
            StagingDllSearch::DirectChildDirs => self.scan_direct_child_dirs(staging),
            StagingDllSearch::NestedModsBinX64 => self.scan_nested_mods_bin_x64(staging),
        }
    }

    fn push_dll_name(self, out: &mut SmallVec<[String; 4]>, file_name: &std::ffi::OsStr) {
        let name = file_name.to_string_lossy().to_lowercase();
        if let Some(stem) = name.strip_suffix(".dll")
            && self.proxy_dlls.contains(&stem)
            && !out.iter().any(|item| item == stem)
        {
            out.push(stem.to_string());
        }
    }

    fn scan_direct_child_dirs(self, staging: &Path) -> SmallVec<[String; 4]> {
        let mut out = SmallVec::new();
        let Ok(entries) = std::fs::read_dir(staging) else {
            return out;
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let Ok(inner) = std::fs::read_dir(entry.path()) else {
                continue;
            };
            for file in inner.flatten() {
                self.push_dll_name(&mut out, &file.file_name());
            }
        }
        out
    }

    fn scan_nested_mods_bin_x64(self, staging: &Path) -> SmallVec<[String; 4]> {
        let mut out = SmallVec::new();
        let mods_dir = staging.join("mods");
        if !mods_dir.is_dir() {
            return out;
        }

        for entry in std::fs::read_dir(&mods_dir).into_iter().flatten().flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let mod_bin_x64 = entry.path().join("bin/x64");
            if !mod_bin_x64.is_dir() {
                continue;
            }
            for file in std::fs::read_dir(&mod_bin_x64)
                .into_iter()
                .flatten()
                .flatten()
            {
                self.push_dll_name(&mut out, &file.file_name());
            }
        }

        out
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ModDirectoryLayout {
    Relative(&'static str),
    Ue4PaksMods { project_name: &'static str },
}

impl ModDirectoryLayout {
    #[must_use]
    pub fn resolve(self, install: &Path) -> PathBuf {
        match self {
            Self::Relative(path) => install.join(path),
            Self::Ue4PaksMods { project_name } => install
                .join(project_name)
                .join("Content")
                .join("Paks")
                .join("~mods"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CollisionPolicy {
    pub archive_extensions: &'static [&'static str],
    pub severities: &'static [(&'static str, CollisionSeverity)],
}

impl CollisionPolicy {
    #[must_use]
    pub fn classify_severity(self, file_path: &str) -> CollisionSeverity {
        let ext = file_path.rsplit('.').next().unwrap_or("").to_lowercase();
        self.severities
            .iter()
            .find_map(|(candidate, severity)| (*candidate == ext).then_some(*severity))
            .unwrap_or(CollisionSeverity::Unknown)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PolicyCollisionClassifier {
    pub policy: CollisionPolicy,
}

impl CollisionClassifier for PolicyCollisionClassifier {
    fn index_archive(&self, _archive_path: &Path) -> anyhow::Result<Vec<(String, u64)>> {
        Ok(Vec::new())
    }

    fn classify_severity(&self, file_path: &str) -> CollisionSeverity {
        self.policy.classify_severity(file_path)
    }

    fn archive_extensions(&self) -> &[&str] {
        self.policy.archive_extensions
    }
}
