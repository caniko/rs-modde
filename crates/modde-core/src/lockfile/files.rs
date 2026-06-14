use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{CoreError, Result};
use crate::hash;
use crate::installer::StagedFile;
use crate::paths;
use super::LockedFile;
use super::validation::validate_relative_path;

pub(in crate::lockfile) fn group_installed_files(files: Vec<(String, StagedFile)>) -> BTreeMap<String, Vec<StagedFile>> {
    let mut grouped: BTreeMap<String, Vec<StagedFile>> = BTreeMap::new();
    for (mod_id, file) in files {
        grouped.entry(mod_id).or_default().push(file);
    }
    grouped
}

pub(in crate::lockfile) async fn lock_store_file(mod_id: &str, file: &StagedFile) -> Result<LockedFile> {
    validate_relative_path(&file.rel_path)?;
    validate_relative_path(&file.origin_rel_path)?;
    let path = paths::store_dir().join(mod_id).join(&file.rel_path);
    lock_file_at(
        &path,
        &file.rel_path,
        &file.origin_rel_path,
        file.merge_group.clone(),
    )
    .await
}

pub(in crate::lockfile) async fn lock_file_at(
    path: &Path,
    rel_path: &str,
    origin_rel_path: &str,
    merge_group: Option<String>,
) -> Result<LockedFile> {
    let metadata = tokio::fs::metadata(path).await.map_err(|error| {
        CoreError::Other(
            format!(
                "required lockfile input missing {}: {error}",
                path.display()
            )
            .into(),
        )
    })?;
    let xxh64 = hash::hash_file_xxh64(path).await?;
    let xxh3 = hash::hash_file_xxhash(path).await?;
    Ok(LockedFile {
        rel_path: rel_path.to_string(),
        origin_rel_path: origin_rel_path.to_string(),
        size: metadata.len(),
        sha256: hash::hash_file_sha256(path).await?,
        xxh64: format!("{xxh64:016x}"),
        xxh3: format!("{xxh3:016x}"),
        merge_group,
    })
}

pub(in crate::lockfile) async fn verify_locked_file(path: &Path, file: &LockedFile) -> Result<()> {
    let metadata = tokio::fs::metadata(path).await.map_err(|error| {
        CoreError::Other(format!("locked file missing {}: {error}", path.display()).into())
    })?;
    if metadata.len() != file.size {
        return Err(CoreError::Validation(
            format!(
                "locked file size mismatch for {}: expected {}, got {}",
                path.display(),
                file.size,
                metadata.len()
            )
            .into(),
        ));
    }
    let expected_xxh64 = u64::from_str_radix(&file.xxh64, 16).map_err(|error| {
        CoreError::Validation(
            format!(
                "locked file {} has invalid xxh64 digest '{}': {error}",
                file.rel_path, file.xxh64
            )
            .into(),
        )
    })?;
    hash::verify_xxh64(path, expected_xxh64).await?;
    let expected_xxh3 = u64::from_str_radix(&file.xxh3, 16).map_err(|error| {
        CoreError::Validation(
            format!(
                "locked file {} has invalid xxh3 digest '{}': {error}",
                file.rel_path, file.xxh3
            )
            .into(),
        )
    })?;
    hash::verify_xxhash(path, expected_xxh3).await?;
    hash::verify_sha256(path, &file.sha256).await?;
    Ok(())
}
