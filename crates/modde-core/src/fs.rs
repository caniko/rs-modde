use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Check if an I/O error is a cross-device link error (EXDEV on Unix,
/// `ERROR_NOT_SAME_DEVICE` on Windows). Used to fall back from `rename`
/// to copy+delete when source and destination are on different filesystems.
#[must_use]
pub fn is_cross_device_error(e: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(libc::EXDEV)
    }
    #[cfg(windows)]
    {
        e.raw_os_error() == Some(17) // ERROR_NOT_SAME_DEVICE
    }
}

/// Create a symlink at `link` pointing to `original`, using the correct
/// platform API.  On Windows the call inspects `original` to decide between
/// `symlink_file` and `symlink_dir`.
pub fn symlink(original: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(original, link)
    }
    #[cfg(windows)]
    {
        if original.is_dir() {
            std::os::windows::fs::symlink_dir(original, link)
        } else {
            std::os::windows::fs::symlink_file(original, link)
        }
    }
}

/// Recursively visit every file under `dir`, calling `visitor(absolute_path)` for each.
///
/// This is the single recursive walker that all public helpers delegate to.
fn walk_dir(dir: &Path, visitor: &mut dyn FnMut(&Path) -> Result<()>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("failed to read directory: {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, visitor)?;
        } else {
            visitor(&path)?;
        }
    }
    Ok(())
}

/// Recursively walk a directory, collecting `(relative_path, absolute_path)` pairs for all files.
pub fn walk_files_relative(base: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut files = Vec::new();
    walk_dir(base, &mut |path| {
        let rel = path
            .strip_prefix(base)
            .with_context(|| "failed to compute relative path")?;
        files.push((rel.to_string_lossy().to_string(), path.to_path_buf()));
        Ok(())
    })?;
    Ok(files)
}

/// Recursively walk a directory, collecting all absolute file paths.
pub fn walk_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    walk_dir(dir, &mut |path| {
        files.push(path.to_path_buf());
        Ok(())
    })?;
    Ok(files)
}

/// Count files recursively in a directory.
pub fn count_files(dir: &Path) -> Result<u64> {
    let mut count = 0u64;
    walk_dir(dir, &mut |_| {
        count += 1;
        Ok(())
    })?;
    Ok(count)
}

/// Create a symlink asynchronously, using the correct platform API.
/// On Windows, inspects `original` to pick `symlink_file` vs `symlink_dir`.
pub async fn symlink_async(original: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        tokio::fs::symlink(original, link).await
    }
    #[cfg(windows)]
    {
        if original.is_dir() {
            tokio::fs::symlink_dir(original, link).await
        } else {
            tokio::fs::symlink_file(original, link).await
        }
    }
}

/// Deploy symlinks from `src` into `dst` recursively (creating directories as needed).
pub fn deploy_symlinks(src: &Path, dst: &Path) -> Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)
        .with_context(|| format!("failed to read staging dir: {}", src.display()))?
    {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path)?;
            deploy_symlinks(&src_path, &dst_path)?;
        } else {
            if dst_path.exists() || dst_path.symlink_metadata().is_ok() {
                std::fs::remove_file(&dst_path)?;
            }
            symlink(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge::MERGED_MOD_ID;
    use crate::resolver::{ModId, ResolvedLoadOrder};
    use crate::vfs::SymlinkFarm;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn walk_files_relative_empty() {
        let tmp = TempDir::new().unwrap();
        let files = walk_files_relative(tmp.path()).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn walk_files_relative_flat() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "a").unwrap();
        std::fs::write(tmp.path().join("b.esp"), "b").unwrap();
        let mut files = walk_files_relative(tmp.path()).unwrap();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].0, "a.txt");
        assert_eq!(files[1].0, "b.esp");
    }

    #[test]
    fn walk_files_relative_nested() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("sub/deep")).unwrap();
        std::fs::write(tmp.path().join("sub/deep/file.txt"), "x").unwrap();
        std::fs::write(tmp.path().join("top.txt"), "y").unwrap();
        let files = walk_files_relative(tmp.path()).unwrap();
        assert_eq!(files.len(), 2);
        let rels: Vec<&str> = files.iter().map(|(r, _)| r.as_str()).collect();
        assert!(rels.contains(&"top.txt"));
        assert!(rels.contains(&"sub/deep/file.txt"));
    }

    #[test]
    fn walk_files_relative_nonexistent() {
        let tmp = TempDir::new().unwrap();
        let files = walk_files_relative(&tmp.path().join("nope")).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn walk_files_flat_test() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a"), "a").unwrap();
        std::fs::write(tmp.path().join("b"), "b").unwrap();
        let files = walk_files(tmp.path()).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|p| p.is_absolute()));
    }

    #[test]
    fn count_files_test() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("sub")).unwrap();
        std::fs::write(tmp.path().join("a"), "a").unwrap();
        std::fs::write(tmp.path().join("sub/b"), "b").unwrap();
        assert_eq!(count_files(tmp.path()).unwrap(), 2);
    }

    #[test]
    fn count_files_nonexistent() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(count_files(&tmp.path().join("nope")).unwrap(), 0);
    }

    #[test]
    fn deploy_symlinks_test() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), "a").unwrap();
        std::fs::write(src.join("sub/b.txt"), "b").unwrap();

        deploy_symlinks(&src, &dst).unwrap();

        assert!(
            dst.join("a.txt")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            dst.join("sub/b.txt")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(dst.join("a.txt")).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(dst.join("sub/b.txt")).unwrap(), "b");
    }

    #[tokio::test]
    async fn deploy_with_merged_mod_uses_synthetic_source_for_merged_path() {
        let tmp = TempDir::new().unwrap();
        let source_a = tmp.path().join("store/mod_a/content/scripts/game/foo.ws");
        let source_b = tmp.path().join("store/mod_b/content/scripts/game/foo.ws");
        let source_c = tmp.path().join("store/mod_c/content/scripts/game/foo.ws");
        let source_merged = tmp
            .path()
            .join("profiles/fs-merged-path/__merged__/content/scripts/game/foo.ws");
        for path in [&source_a, &source_b, &source_c, &source_merged] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        }
        std::fs::write(&source_a, "a").unwrap();
        std::fs::write(&source_b, "b").unwrap();
        std::fs::write(&source_c, "c").unwrap();
        std::fs::write(&source_merged, "merged").unwrap();

        let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
        let rel_path = "content/scripts/game/foo.ws".to_string();
        mod_files.insert(
            ModId::from("mod_a"),
            vec![(rel_path.clone(), source_a.clone())],
        );
        mod_files.insert(
            ModId::from("mod_b"),
            vec![(rel_path.clone(), source_b.clone())],
        );
        mod_files.insert(
            ModId::from("mod_c"),
            vec![(rel_path.clone(), source_c.clone())],
        );
        mod_files.insert(
            ModId::from(MERGED_MOD_ID),
            vec![(rel_path.clone(), source_merged.clone())],
        );

        let resolved = ResolvedLoadOrder {
            order: vec![
                ModId::from("mod_a"),
                ModId::from("mod_b"),
                ModId::from("mod_c"),
                ModId::from(MERGED_MOD_ID),
            ],
        };
        let farm = SymlinkFarm::build("fs-merged-path", &resolved, &mod_files, None, None)
            .unwrap()
            .materialize()
            .await
            .unwrap();
        let game_dir = tmp.path().join("game");
        farm.deploy_to(&game_dir).await.unwrap();

        let deployed = game_dir.join(&rel_path);
        assert_eq!(
            std::fs::read_link(&deployed).unwrap(),
            farm.staging_dir.join(&rel_path)
        );
        assert_eq!(std::fs::read_to_string(deployed).unwrap(), "merged");
    }
}

#[cfg(test)]
mod deploy_with_merged_mod {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use crate::merge::MERGED_MOD_ID;
    use crate::resolver::{ModId, ResolvedLoadOrder};
    use crate::vfs::SymlinkFarm;

    #[tokio::test]
    async fn symlink_farm_deploys_synthetic_winner() {
        let tmp = TempDir::new().unwrap();
        let rel_path = "content/scripts/game/foo.ws".to_string();
        let source_a = tmp.path().join("store/mod_a").join(&rel_path);
        let source_merged = tmp
            .path()
            .join("profiles/fs-synthetic-winner/__merged__")
            .join(&rel_path);
        for path in [&source_a, &source_merged] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        }
        std::fs::write(&source_a, "a").unwrap();
        std::fs::write(&source_merged, "merged").unwrap();

        let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
        mod_files.insert(ModId::from("mod_a"), vec![(rel_path.clone(), source_a)]);
        mod_files.insert(
            ModId::from(MERGED_MOD_ID),
            vec![(rel_path.clone(), source_merged)],
        );
        let resolved = ResolvedLoadOrder {
            order: vec![ModId::from("mod_a"), ModId::from(MERGED_MOD_ID)],
        };

        let farm = SymlinkFarm::build("fs-synthetic-winner", &resolved, &mod_files, None, None)
            .unwrap()
            .materialize()
            .await
            .unwrap();
        let game_dir = tmp.path().join("game");
        farm.deploy_to(&game_dir).await.unwrap();

        assert_eq!(
            std::fs::read_to_string(game_dir.join(rel_path)).unwrap(),
            "merged"
        );
    }
}
