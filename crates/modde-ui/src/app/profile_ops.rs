use std::path::PathBuf;

use modde_core::profile::{ProfileManager, ReorderError, try_reorder};
use modde_core::resolver::GameId;

use super::{
    ExperimentWriteKind, ExperimentWriteOutcome, ProfileWriteOutcome, ReorderDirection,
    format_lock_reason,
};

pub(super) async fn create_profile(
    db: modde_core::db::ModdeDb,
    profile: modde_core::Profile,
) -> Result<ProfileWriteOutcome, String> {
    ProfileManager::with_db(db)
        .create(&profile)
        .await
        .map_err(|err| err.to_string())?;
    Ok(ProfileWriteOutcome {
        status_message: Some("Profile created".to_string()),
        reload: true,
    })
}

pub(super) async fn delete_profile(
    db: modde_core::db::ModdeDb,
    name: String,
) -> Result<ProfileWriteOutcome, String> {
    ProfileManager::with_db(db)
        .delete(&name, None)
        .await
        .map_err(|err| err.to_string())?;
    Ok(ProfileWriteOutcome {
        status_message: Some(format!("Profile '{name}' deleted")),
        reload: true,
    })
}

pub(super) async fn fork_profile(
    db: modde_core::db::ModdeDb,
    source: String,
    new_name: String,
    game_id: GameId,
) -> Result<ProfileWriteOutcome, String> {
    ProfileManager::with_db(db)
        .fork(&source, &new_name, &game_id)
        .await
        .map_err(|err| err.to_string())?;
    Ok(ProfileWriteOutcome {
        status_message: Some(format!("Profile forked as '{new_name}'")),
        reload: true,
    })
}

pub(super) async fn reorder_mod(
    db: modde_core::db::ModdeDb,
    profile_name: String,
    mod_id: String,
    direction: ReorderDirection,
) -> Result<ProfileWriteOutcome, String> {
    let pm = ProfileManager::with_db(db);
    let mut profile = pm
        .load(&profile_name, None)
        .await
        .map_err(|err| err.to_string())?;

    match try_reorder(&mut profile, &mod_id, direction) {
        Ok(()) => {
            pm.create_or_update(&profile)
                .await
                .map_err(|err| err.to_string())?;
            Ok(ProfileWriteOutcome {
                status_message: Some(format!(
                    "Moved '{mod_id}' {}",
                    match direction {
                        ReorderDirection::Up => "up",
                        ReorderDirection::Down => "down",
                    }
                )),
                reload: true,
            })
        }
        Err(ReorderError::ProfileLocked { reason }) => Ok(ProfileWriteOutcome {
            status_message: Some(format!(
                "Load order is locked by {} — unlock the profile to reorder.",
                format_lock_reason(&reason)
            )),
            reload: false,
        }),
        Err(ReorderError::ModPinned {
            mod_id: mid,
            reason,
        }) => Ok(ProfileWriteOutcome {
            status_message: Some(format!(
                "'{mid}' is pinned ({}) — unpin it to reorder.",
                format_lock_reason(&reason)
            )),
            reload: false,
        }),
        Err(ReorderError::AdjacentPinned { neighbor_id, .. }) => Ok(ProfileWriteOutcome {
            status_message: Some(format!("Cannot move past a pinned mod ('{neighbor_id}').")),
            reload: false,
        }),
        Err(ReorderError::ModNotFound { mod_id: mid }) => Ok(ProfileWriteOutcome {
            status_message: Some(format!("Mod not found in profile: {mid}")),
            reload: false,
        }),
        Err(ReorderError::AtBoundary) => Ok(ProfileWriteOutcome {
            status_message: None,
            reload: false,
        }),
    }
}

pub(super) async fn add_mod_to_profile(
    db: modde_core::db::ModdeDb,
    profile_name: String,
    mod_id: String,
) -> Result<ProfileWriteOutcome, String> {
    let pm = ProfileManager::with_db(db);
    let mut profile = pm
        .load(&profile_name, None)
        .await
        .map_err(|err| err.to_string())?;
    profile.mods.push(modde_core::EnabledMod {
        mod_id: mod_id.clone(),
        enabled: true,
        ..Default::default()
    });
    pm.create_or_update(&profile)
        .await
        .map_err(|err| err.to_string())?;
    Ok(ProfileWriteOutcome {
        status_message: Some(format!("Added mod: {mod_id}")),
        reload: true,
    })
}

pub(super) async fn remove_mod_from_profile(
    db: modde_core::db::ModdeDb,
    profile_name: String,
    index: usize,
) -> Result<ProfileWriteOutcome, String> {
    let pm = ProfileManager::with_db(db);
    let mut profile = pm
        .load(&profile_name, None)
        .await
        .map_err(|err| err.to_string())?;
    if index >= profile.mods.len() {
        return Ok(ProfileWriteOutcome {
            status_message: None,
            reload: false,
        });
    }
    let removed = profile.mods.remove(index);
    pm.create_or_update(&profile)
        .await
        .map_err(|err| err.to_string())?;
    Ok(ProfileWriteOutcome {
        status_message: Some(format!("Removed mod: {}", removed.mod_id)),
        reload: true,
    })
}

pub(super) async fn toggle_mod_enabled(
    db: modde_core::db::ModdeDb,
    profile_name: String,
    mod_id: String,
    enabled: bool,
) -> Result<ProfileWriteOutcome, String> {
    let pm = ProfileManager::with_db(db);
    let mut profile = pm
        .load(&profile_name, None)
        .await
        .map_err(|err| err.to_string())?;
    if let Some(mod_entry) = profile.mods.iter_mut().find(|entry| entry.mod_id == mod_id) {
        mod_entry.enabled = enabled;
    }
    pm.create_or_update(&profile)
        .await
        .map_err(|err| err.to_string())?;
    Ok(ProfileWriteOutcome {
        status_message: Some(format!(
            "Mod {mod_id} {}",
            if enabled { "enabled" } else { "disabled" }
        )),
        reload: true,
    })
}

pub(super) async fn set_mod_lock(
    db: modde_core::db::ModdeDb,
    profile_name: String,
    mod_id: String,
    locked: bool,
) -> Result<ProfileWriteOutcome, String> {
    let pm = ProfileManager::with_db(db);
    let mut profile = pm
        .load(&profile_name, None)
        .await
        .map_err(|err| err.to_string())?;
    let Some(mod_entry) = profile.mods.iter_mut().find(|entry| entry.mod_id == mod_id) else {
        return Ok(ProfileWriteOutcome {
            status_message: None,
            reload: false,
        });
    };

    mod_entry.lock = locked.then_some(modde_core::LockReason::Manual { note: None });
    pm.update(&profile).await.map_err(|err| err.to_string())?;
    Ok(ProfileWriteOutcome {
        status_message: Some(if locked {
            format!("Pinned '{mod_id}'")
        } else {
            format!("Unpinned '{mod_id}'")
        }),
        reload: true,
    })
}

pub(super) async fn run_experiment_write(
    db: modde_core::db::ModdeDb,
    kind: ExperimentWriteKind,
    profile_name: Option<String>,
    game_id: GameId,
    save_dir: Option<PathBuf>,
    current_depth: usize,
) -> Result<ExperimentWriteOutcome, String> {
    let pm = ProfileManager::with_db(db);
    match kind {
        ExperimentWriteKind::Try => {
            let profile_name =
                profile_name.ok_or_else(|| "No active profile selected".to_string())?;
            pm.try_profile(&profile_name, &game_id, save_dir.as_deref())
                .await
                .map_err(|err| err.to_string())?;
            let next_depth = current_depth.saturating_add(1);
            Ok(ExperimentWriteOutcome {
                previous_profile: None,
                status_message: format!("Experiment started (depth {next_depth})"),
                reload: true,
            })
        }
        ExperimentWriteKind::Rollback => {
            let previous_profile = pm
                .rollback(&game_id, save_dir.as_deref())
                .await
                .map_err(|err| err.to_string())?;
            Ok(ExperimentWriteOutcome {
                status_message: format!("Rolled back to '{previous_profile}'"),
                previous_profile: Some(previous_profile),
                reload: true,
            })
        }
        ExperimentWriteKind::Commit => {
            pm.commit(&game_id).await.map_err(|err| err.to_string())?;
            Ok(ExperimentWriteOutcome {
                previous_profile: None,
                status_message: "Experiment committed".to_string(),
                reload: true,
            })
        }
    }
}
