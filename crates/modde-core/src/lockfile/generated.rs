use std::path::PathBuf;

use super::files::lock_file_at;
use super::validation::validate_relative_path;
use super::{LockProfile, PatcherStageLock, ToolOutputLock, WabbajackManifestLock};
use crate::db::ModdeDb;
use crate::error::{CoreError, Result};
use crate::profile::{Profile, ProfileSource};
use crate::resolver::GameId;
use crate::{hash, paths};

pub(in crate::lockfile) async fn lock_patchers(
    db: &ModdeDb,
    profile: &Profile,
) -> Result<Vec<PatcherStageLock>> {
    let profile_id = profile.id.ok_or_else(|| {
        CoreError::Validation(format!("profile '{}' is not persisted", profile.name).into())
    })?;
    let stages = db.list_patcher_stages(profile_id).await?;
    let mut locked = Vec::with_capacity(stages.len());
    for stage in stages {
        let output_rows = db
            .list_patcher_stage_outputs(profile_id, &stage.name)
            .await?;
        let root = stage_generated_dir(
            &LockProfile {
                name: profile.name.clone(),
                game_id: profile.game_id.to_string(),
                source: profile.source.clone(),
                load_order_lock: profile.load_order_lock.clone(),
                mod_order: Vec::new(),
            },
            &stage.name,
        );
        let mut outputs = Vec::with_capacity(output_rows.len());
        for row in output_rows {
            validate_relative_path(&row.rel_path)?;
            outputs.push(
                lock_file_at(
                    &root.join(&row.rel_path),
                    &row.rel_path,
                    &row.rel_path,
                    None,
                )
                .await?,
            );
        }
        locked.push(PatcherStageLock {
            name: stage.name,
            stage_kind: stage.stage_kind,
            enabled: stage.enabled,
            sort_index: stage.sort_index,
            settings: stage.settings,
            output_mod: stage.output_mod,
            outputs,
        });
    }
    Ok(locked)
}

pub(in crate::lockfile) async fn lock_tool_outputs(
    db: &ModdeDb,
    game_id: &GameId,
) -> Result<Vec<ToolOutputLock>> {
    let rows = db.load_all_applied_file_rows(game_id).await?;
    let mut outputs = Vec::with_capacity(rows.len());
    for row in rows {
        validate_relative_path(&row.rel_path)?;
        let file = lock_file_at(
            &paths::store_dir().join("__overwrite__").join(&row.rel_path),
            &row.rel_path,
            &row.rel_path,
            None,
        )
        .await?;
        outputs.push(ToolOutputLock {
            tool_id: row.tool_id,
            rel_path: row.rel_path,
            file,
        });
    }
    Ok(outputs)
}

pub(in crate::lockfile) async fn lock_wabbajack_manifest(
    source: &ProfileSource,
) -> Result<Option<WabbajackManifestLock>> {
    let ProfileSource::Wabbajack { manifest_hash } = source else {
        return Ok(None);
    };
    let cached = paths::wabbajack_cache_path(manifest_hash);
    if cached.exists() {
        Ok(Some(WabbajackManifestLock {
            manifest_hash: manifest_hash.clone(),
            cached_path: Some(cached.display().to_string()),
            sha256: Some(hash::hash_file_sha256(&cached).await?),
        }))
    } else {
        Ok(Some(WabbajackManifestLock {
            manifest_hash: manifest_hash.clone(),
            cached_path: None,
            sha256: None,
        }))
    }
}

pub(in crate::lockfile) fn stage_generated_dir(profile: &LockProfile, stage_name: &str) -> PathBuf {
    paths::generated_dir()
        .join(&profile.game_id)
        .join(&profile.name)
        .join(stage_name)
}
