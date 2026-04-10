//! Stage files from a temporary extraction directory into a mod's store
//! directory, according to an [`InstallPlan`].
//!
//! Execution is deliberately dumb: it copies files, records their paths,
//! and returns the manifest. It does not touch the database — the caller
//! wires the returned `Vec<StagedFile>` into `ModdeDb::record_install`.
//!
//! Variants that need user input (`Fomod` with no config, `Bain` with no
//! selection) return [`InstallerError::RequiresUserInput`] so the UI can
//! route to its wizard.

use std::fs;
use std::path::{Path, PathBuf};

use super::fs::walk_files;
use super::types::{InstallMethod, InstallPlan, InstallerError, InstallerResult, StagedFile};

/// Execute `plan`, staging files from `extracted_dir` into
/// `store_mod_dir`. On success, mutates `plan.staged_files` with the
/// final manifest and returns it by value for the caller to persist.
///
/// `extracted_dir` is the temp directory the archive was unzipped into.
/// `store_mod_dir` is the canonical per-mod directory in the store (the
/// caller decides the naming, typically `{domain}_{mod_id}_{file_id}`).
pub fn execute(
    plan: &mut InstallPlan,
    extracted_dir: &Path,
    store_mod_dir: &Path,
) -> InstallerResult<Vec<StagedFile>> {
    let source_root = if let Some(ref strip) = plan.strip_prefix {
        extracted_dir.join(strip)
    } else {
        extracted_dir.to_path_buf()
    };

    fs::create_dir_all(store_mod_dir)?;

    let files = match &plan.method {
        InstallMethod::BareExtract => stage_tree(&source_root, store_mod_dir, None)?,

        InstallMethod::REDmod { manifest: _ } => {
            // REDmod layout already ships under a top-level dir the game
            // expects; we stage everything under `mods/<mod-name>/`. The
            // store dir name is used as the mod name so the caller can
            // pick a stable identifier.
            let mod_name = store_mod_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("redmod");
            let nested = store_mod_dir.join("mods").join(mod_name);
            fs::create_dir_all(&nested)?;
            stage_tree(&source_root, &nested, Some(PathBuf::from("mods").join(mod_name)))?
        }

        InstallMethod::DllOverlay { .. } => {
            // Stage under a flagged subdir; deploy decides the final
            // destination using the game plugin's `executable_dir`.
            let overlay_dir = store_mod_dir.join("__dll_overlay__");
            fs::create_dir_all(&overlay_dir)?;
            stage_tree(
                &source_root,
                &overlay_dir,
                Some(PathBuf::from("__dll_overlay__")),
            )?
        }

        InstallMethod::Fomod { config_toml, .. } => {
            if config_toml.is_none() {
                return Err(InstallerError::RequiresUserInput { method: "fomod" });
            }
            // TODO(installer): apply the declarative config by consulting
            // `fomod_oxide` from the CLI/UI edge. For now, stage the raw
            // archive so the deploy step can re-run the FOMOD applier
            // against the stored `config_toml`. This preserves today's
            // Wabbajack-collection behavior while the richer applier
            // lands in a follow-up.
            stage_tree(&source_root, store_mod_dir, None)?
        }

        InstallMethod::Bain { selected_subdirs } => {
            if selected_subdirs.is_empty() {
                return Err(InstallerError::RequiresUserInput { method: "bain" });
            }
            let mut out = Vec::new();
            for subdir in selected_subdirs {
                let sub_src = source_root.join(subdir);
                if !sub_src.exists() {
                    return Err(InstallerError::MissingFile(subdir.clone()));
                }
                let sub_dest = store_mod_dir;
                out.extend(stage_tree(&sub_src, sub_dest, Some(PathBuf::from(subdir)))?);
            }
            out
        }

        InstallMethod::ScriptMerge { merge_group, base } => {
            // Execute the base method, then tag every staged file with
            // the merge group. Actual merging is deferred until the
            // script-merge feature lands.
            let mut inner_plan = InstallPlan {
                method: (**base).clone(),
                strip_prefix: plan.strip_prefix.clone(),
                source_archive_hash: plan.source_archive_hash.clone(),
                staged_files: Vec::new(),
            };
            let mut files = execute(&mut inner_plan, extracted_dir, store_mod_dir)?;
            for f in &mut files {
                f.merge_group = Some(merge_group.clone());
            }
            files
        }

        InstallMethod::Unknown { reason } => {
            return Err(InstallerError::UnknownMethod {
                reason: reason.clone(),
            });
        }
    };

    plan.staged_files = files.clone();
    Ok(files)
}

/// Copy every file under `src` into `dest`, preserving subdirs. Returns
/// the staged manifest with origin paths relative to `src` (optionally
/// prefixed by `origin_prefix` so callers can reconstruct the archive
/// path when the plan nests the source into a subdir).
fn stage_tree(
    src: &Path,
    dest: &Path,
    origin_prefix: Option<PathBuf>,
) -> InstallerResult<Vec<StagedFile>> {
    let mut out = Vec::new();
    let files = walk_files(src)?;
    for (abs, rel) in files {
        let dest_path = dest.join(&rel);
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Prefer rename when src/dest are on the same filesystem; fall
        // back to copy+remove otherwise.
        if fs::rename(&abs, &dest_path).is_err() {
            fs::copy(&abs, &dest_path)?;
            let _ = fs::remove_file(&abs);
        }
        let size = fs::metadata(&dest_path).map(|m| m.len()).unwrap_or(0);
        let origin_rel_path = match &origin_prefix {
            Some(p) => p.join(&rel).to_string_lossy().to_string(),
            None => rel.to_string_lossy().to_string(),
        };
        out.push(StagedFile {
            rel_path: dest_rel(dest, &dest_path),
            origin_rel_path,
            size,
            merge_group: None,
        });
    }
    Ok(out)
}

fn dest_rel(dest_root: &Path, dest_path: &Path) -> String {
    dest_path
        .strip_prefix(dest_root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| dest_path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn touch(p: &Path, body: &[u8]) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(p).unwrap();
        f.write_all(body).unwrap();
    }

    #[test]
    fn bare_extract_moves_files_into_store() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("staging");
        let store = tmp.path().join("store");
        touch(&staging.join("Data").join("foo.esp"), b"hello");
        touch(&staging.join("readme.md"), b"hi");

        let mut plan = InstallPlan {
            method: InstallMethod::BareExtract,
            strip_prefix: None,
            source_archive_hash: "h".into(),
            staged_files: vec![],
        };
        let files = execute(&mut plan, &staging, &store).unwrap();
        assert_eq!(files.len(), 2);
        assert!(store.join("Data/foo.esp").exists());
        assert!(store.join("readme.md").exists());
        assert!(plan.staged_files.len() == 2);
    }

    #[test]
    fn strip_prefix_is_applied() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("staging");
        let store = tmp.path().join("store");
        touch(&staging.join("ModName-1.0/Data/foo.esp"), b"hello");

        let mut plan = InstallPlan {
            method: InstallMethod::BareExtract,
            strip_prefix: Some(PathBuf::from("ModName-1.0")),
            source_archive_hash: "h".into(),
            staged_files: vec![],
        };
        let files = execute(&mut plan, &staging, &store).unwrap();
        assert_eq!(files.len(), 1);
        assert!(store.join("Data/foo.esp").exists());
        assert_eq!(files[0].rel_path, "Data/foo.esp");
    }

    #[test]
    fn fomod_without_config_requires_user_input() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("staging");
        let store = tmp.path().join("store");
        touch(&staging.join("fomod/ModuleConfig.xml"), b"<config/>");
        touch(&staging.join("Data/foo.esp"), b"hello");

        let mut plan = InstallPlan {
            method: InstallMethod::Fomod {
                module_config: PathBuf::from("fomod/ModuleConfig.xml"),
                config_toml: None,
            },
            strip_prefix: None,
            source_archive_hash: "h".into(),
            staged_files: vec![],
        };
        let err = execute(&mut plan, &staging, &store).unwrap_err();
        assert!(matches!(err, InstallerError::RequiresUserInput { .. }));
    }

    #[test]
    fn unknown_returns_unknown_method() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("staging");
        let store = tmp.path().join("store");
        touch(&staging.join("blob.bin"), b"x");

        let mut plan = InstallPlan {
            method: InstallMethod::Unknown {
                reason: "test".into(),
            },
            strip_prefix: None,
            source_archive_hash: "h".into(),
            staged_files: vec![],
        };
        let err = execute(&mut plan, &staging, &store).unwrap_err();
        assert!(matches!(err, InstallerError::UnknownMethod { .. }));
    }

    #[test]
    fn script_merge_tags_files() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("staging");
        let store = tmp.path().join("store");
        touch(&staging.join("Data/foo.esp"), b"hello");

        let mut plan = InstallPlan {
            method: InstallMethod::ScriptMerge {
                merge_group: "quest-scripts".into(),
                base: Box::new(InstallMethod::BareExtract),
            },
            strip_prefix: None,
            source_archive_hash: "h".into(),
            staged_files: vec![],
        };
        let files = execute(&mut plan, &staging, &store).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].merge_group.as_deref(), Some("quest-scripts"));
    }
}
