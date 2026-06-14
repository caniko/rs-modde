use super::generated::stage_generated_dir;
use super::files::verify_locked_file;
use super::signatures::verify_signatures;
use super::validation::validate_lock;
use super::{ModdeLock, VerifyReport};
use crate::error::Result;
use crate::paths;

pub async fn verify_lock_against_disk(lock: &ModdeLock) -> Result<VerifyReport> {
    validate_lock(lock)?;
    verify_signatures(lock)?;

    let mut checked_files = 0;
    for locked_mod in &lock.payload.mods {
        for file in &locked_mod.files {
            let path = paths::store_dir()
                .join(&locked_mod.mod_id)
                .join(&file.rel_path);
            verify_locked_file(&path, file).await?;
            checked_files += 1;
        }
    }
    for patcher in &lock.payload.patchers {
        let root = stage_generated_dir(&lock.payload.profile, &patcher.name);
        for output in &patcher.outputs {
            verify_locked_file(&root.join(&output.rel_path), output).await?;
            checked_files += 1;
        }
    }
    for output in &lock.payload.tool_outputs {
        let path = paths::store_dir()
            .join("__overwrite__")
            .join(&output.rel_path);
        verify_locked_file(&path, &output.file).await?;
        checked_files += 1;
    }

    let mut warnings = Vec::new();
    if !lock.payload.reproducible {
        warnings.push("lock is marked non-reproducible".to_string());
    }
    Ok(VerifyReport {
        signature_count: lock.signatures.len(),
        checked_files,
        warnings,
    })
}
