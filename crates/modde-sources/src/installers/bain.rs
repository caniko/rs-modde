use std::path::Path;

use anyhow::Result;

/// A BAIN package: a mod archive with numbered subdirectories
/// like `00 Core`, `01 Optional Textures`, `02 Patches`.
#[derive(Debug, Clone)]
pub struct BainPackage {
    pub name: String,
    pub sub_packages: Vec<BainSubPackage>,
}

#[derive(Debug, Clone)]
pub struct BainSubPackage {
    pub index: u32,
    pub name: String,
    pub path: String,
}

/// Detect if a directory contains a BAIN package structure.
#[must_use]
pub fn detect_bain(dir: &Path) -> Option<BainPackage> {
    if !dir.is_dir() {
        return None;
    }

    let mut subs = Vec::new();
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        if !entry.file_type().ok()?.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        // BAIN subdirs start with digits: "00 Core", "01 Optional", etc.
        if let Some(idx_str) = name.split_whitespace().next()
            && let Ok(idx) = idx_str.parse::<u32>() {
                subs.push(BainSubPackage {
                    index: idx,
                    name: name.clone(),
                    path: entry.path().to_string_lossy().to_string(),
                });
            }
    }

    if subs.is_empty() {
        return None;
    }

    subs.sort_by_key(|s| s.index);

    Some(BainPackage {
        name: dir.file_name()?.to_string_lossy().to_string(),
        sub_packages: subs,
    })
}

/// Install selected BAIN sub-packages by copying their contents to the destination.
pub fn install_bain(
    package: &BainPackage,
    selected: &[u32], // indices to install
    dest: &Path,
) -> Result<usize> {
    let mut count = 0;
    for sub in &package.sub_packages {
        if !selected.contains(&sub.index) {
            continue;
        }

        let src = std::path::PathBuf::from(&sub.path);
        copy_dir_recursive(&src, dest)?;
        count += 1;
    }
    Ok(count)
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_bain() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Create BAIN-style subdirectories
        std::fs::create_dir(root.join("00 Core")).unwrap();
        std::fs::create_dir(root.join("01 Optional")).unwrap();
        std::fs::create_dir(root.join("02 Patches")).unwrap();

        let pkg = detect_bain(root).expect("should detect BAIN package");
        assert_eq!(pkg.sub_packages.len(), 3);
        assert_eq!(pkg.sub_packages[0].index, 0);
        assert_eq!(pkg.sub_packages[0].name, "00 Core");
        assert_eq!(pkg.sub_packages[1].index, 1);
        assert_eq!(pkg.sub_packages[2].index, 2);
    }

    #[test]
    fn test_detect_non_bain() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Create regular directories without numeric prefixes
        std::fs::create_dir(root.join("textures")).unwrap();
        std::fs::create_dir(root.join("meshes")).unwrap();

        assert!(
            detect_bain(root).is_none(),
            "should not detect non-BAIN dirs"
        );
    }

    #[test]
    fn test_install_selected() {
        let tmp_src = tempfile::tempdir().unwrap();
        let tmp_dest = tempfile::tempdir().unwrap();
        let src = tmp_src.path();
        let dest = tmp_dest.path();

        // Create BAIN package with files
        let core = src.join("00 Core");
        std::fs::create_dir(&core).unwrap();
        std::fs::write(core.join("plugin.esp"), b"core-data").unwrap();

        let optional = src.join("01 Optional");
        std::fs::create_dir(&optional).unwrap();
        std::fs::write(optional.join("texture.dds"), b"tex-data").unwrap();

        let pkg = detect_bain(src).expect("should detect BAIN package");

        // Only install sub-package 0
        let count = install_bain(&pkg, &[0], dest).unwrap();
        assert_eq!(count, 1);
        assert!(
            dest.join("plugin.esp").exists(),
            "core file should be copied"
        );
        assert!(
            !dest.join("texture.dds").exists(),
            "optional file should not be copied"
        );

        // Now also install sub-package 1
        let count = install_bain(&pkg, &[1], dest).unwrap();
        assert_eq!(count, 1);
        assert!(
            dest.join("texture.dds").exists(),
            "optional file should now be copied"
        );
    }
}
