use std::path::Path;

use git2::{Repository, Signature};

use crate::error::Result;

pub(in crate::save) const STEAM_CLOUD_MARKER: &str = "steam_autocloud.vdf";
pub(in crate::save) const MODDE_LIVE_STATE_DIR: &str = ".modde";
pub(in crate::save) const MODDE_PROFILE_PARK_DIR: &str = "profiles";

pub(in crate::save) fn vault_signature() -> Signature<'static> {
    Signature::now("modde", "modde@localhost").expect("failed to create git signature")
}

/// Sanitize a profile name for use as a git branch name.
pub(in crate::save) fn sanitize_branch_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\' => '-',
            c => c,
        })
        .collect()
}

pub(in crate::save) fn count_tree_entries(repo: &Repository, tree: &git2::Tree) -> usize {
    let mut count = 0;
    for entry in tree {
        match entry.kind() {
            Some(git2::ObjectType::Blob) => count += 1,
            Some(git2::ObjectType::Tree) => {
                if let Ok(subtree) = repo.find_tree(entry.id()) {
                    count += count_tree_entries(repo, &subtree);
                }
            }
            _ => {}
        }
    }
    count
}

/// Recursively collect file paths in a git tree.
pub(in crate::save) fn collect_tree_paths(repo: &Repository, tree: &git2::Tree, prefix: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for entry in tree {
        let name = entry.name().unwrap_or("");
        let full = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        match entry.kind() {
            Some(git2::ObjectType::Blob) => paths.push(full),
            Some(git2::ObjectType::Tree) => {
                if let Ok(subtree) = repo.find_tree(entry.id()) {
                    paths.extend(collect_tree_paths(repo, &subtree, &full));
                }
            }
            _ => {}
        }
    }
    paths
}

/// Remove active root save entries while preserving Steam Cloud metadata and
/// modde's parked inactive profile saves.
pub(in crate::save) fn clear_active_save_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if is_live_metadata(&name) {
            continue;
        }

        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Park active root save entries under `.modde/profiles/<profile>/`.
///
/// This is intentionally in the live save tree: Steam Cloud sees inactive
/// saves as moved rather than simply deleted, while the game only sees saves
/// restored at the root.
pub(in crate::save) fn park_active_saves(game_save_dir: &Path, profile_name: &str) -> Result<()> {
    if !game_save_dir.exists() {
        return Ok(());
    }

    let parked_dir = game_save_dir
        .join(MODDE_LIVE_STATE_DIR)
        .join(MODDE_PROFILE_PARK_DIR)
        .join(sanitize_path_component(profile_name));

    if parked_dir.exists() {
        std::fs::remove_dir_all(&parked_dir)?;
    }
    std::fs::create_dir_all(&parked_dir)?;

    for entry in std::fs::read_dir(game_save_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if is_live_metadata(&name_str) {
            continue;
        }

        let src = entry.path();
        let dst = parked_dir.join(&name);
        if std::fs::rename(&src, &dst).is_err() {
            if src.is_dir() {
                copy_dir_contents(&src, &dst)?;
                std::fs::remove_dir_all(&src)?;
            } else {
                std::fs::copy(&src, &dst)?;
                std::fs::remove_file(&src)?;
            }
        }
    }

    Ok(())
}

pub(in crate::save) fn remove_live_metadata_from_vault(vault_path: &Path) -> Result<()> {
    for name in [STEAM_CLOUD_MARKER, MODDE_LIVE_STATE_DIR] {
        let path = vault_path.join(name);
        if path.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else if path.exists() {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub(in crate::save) fn is_live_metadata(name: &str) -> bool {
    name.eq_ignore_ascii_case(STEAM_CLOUD_MARKER) || name == MODDE_LIVE_STATE_DIR
}

pub(in crate::save) fn sanitize_path_component(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Copy all files/dirs from `src` to `dst`, returning the count of files copied.
fn copy_dir_contents(src: &Path, dst: &Path) -> Result<usize> {
    copy_dir_contents_filtered(src, dst, |_| true)
}

/// Copy all files/dirs from `src` to `dst`, skipping entries where `filter(name)` returns false.
pub(in crate::save) fn copy_dir_contents_filtered(
    src: &Path,
    dst: &Path,
    filter: impl Fn(&str) -> bool + Copy,
) -> Result<usize> {
    let mut count = 0usize;

    if !src.exists() {
        return Ok(0);
    }

    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if !filter(&name_str) {
            continue;
        }

        let src_path = entry.path();
        let dst_path = dst.join(&name);

        if src_path.is_dir() {
            count += copy_dir_contents_filtered(&src_path, &dst_path, filter)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
            count += 1;
        }
    }

    Ok(count)
}
