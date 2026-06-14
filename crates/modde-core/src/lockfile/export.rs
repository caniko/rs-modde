use std::collections::HashSet;

use super::files::{group_installed_files, lock_store_file};
use super::generated::{lock_patchers, lock_tool_outputs, lock_wabbajack_manifest};
use super::helpers::{
    current_utc_timestamp, hidden_file_lock, incomplete_reason, nexus_provenance,
    plugin_entry_lock,
};
use super::validation::validate_payload;
use super::{
    ExportOptions, LockPayload, LockProfile, LockedMod, LOCK_FORMAT_VERSION, LOCK_KIND, ModdeLock,
};
use crate::db::ModdeDb;
use crate::error::{CoreError, Result};
use crate::profile::{Profile, ProfileSource};

pub async fn export_profile_lock(
    db: &ModdeDb,
    profile: &Profile,
    options: ExportOptions,
) -> Result<ModdeLock> {
    let profile_id = profile.id.ok_or_else(|| {
        CoreError::Validation(format!("profile '{}' is not persisted", profile.name).into())
    })?;
    let mut incomplete = Vec::new();
    let mut locked_mods = Vec::with_capacity(profile.mods.len());
    let mut seen_mods = HashSet::new();
    let installed = group_installed_files(db.installed_files_for_profile(profile_id).await?);

    for (order, enabled_mod) in profile.mods.iter().enumerate() {
        if !seen_mods.insert(enabled_mod.mod_id.clone()) {
            return Err(CoreError::Validation(
                format!("duplicate mod id '{}' in profile", enabled_mod.mod_id).into(),
            ));
        }
        let files = installed
            .get(&enabled_mod.mod_id)
            .cloned()
            .unwrap_or_default();
        if files.is_empty() {
            incomplete.push(incomplete_reason(
                &format!("mod:{}", enabled_mod.mod_id),
                "no tracked installed_mod_files rows are available",
                "installer record_install output for this mod",
                "reinstall the mod through modde so staged files are recorded",
                &format!(
                    "modde verify --profile {} --game {}",
                    profile.name, profile.game_id
                ),
            ));
        }
        if enabled_mod
            .source_archive_hash
            .as_deref()
            .is_none_or(str::is_empty)
        {
            incomplete.push(incomplete_reason(
                &format!("mod:{}", enabled_mod.mod_id),
                "missing source archive hash",
                "source archive hash captured by the installer",
                "reinstall the mod from its upstream source through modde",
                &format!(
                    "modde lock export --profile {} --game {} --output modde.lock",
                    profile.name, profile.game_id
                ),
            ));
        }
        if nexus_provenance(enabled_mod).is_none()
            && !matches!(profile.source, ProfileSource::Wabbajack { .. })
        {
            incomplete.push(incomplete_reason(
                &format!("mod:{}", enabled_mod.mod_id),
                "missing portable upstream locator",
                "Nexus ids, Wabbajack manifest provenance, or a future manual archive source",
                "reinstall from Nexus/Wabbajack or add a supported manual archive source before exporting",
                &format!(
                    "modde lock export --profile {} --game {} --output modde.lock",
                    profile.name, profile.game_id
                ),
            ));
        }

        let mut locked_files = Vec::with_capacity(files.len());
        for file in files {
            locked_files.push(lock_store_file(&enabled_mod.mod_id, &file).await.map_err(
                |error| {
                    CoreError::Other(
                        format!(
                            "failed to lock file for mod '{}': {error}",
                            enabled_mod.mod_id
                        )
                        .into(),
                    )
                },
            )?);
        }

        locked_mods.push(LockedMod {
            order,
            mod_id: enabled_mod.mod_id.clone(),
            display_name: enabled_mod.display_name.clone(),
            enabled: enabled_mod.enabled,
            version: enabled_mod.version.clone(),
            nexus: nexus_provenance(enabled_mod),
            source_archive_hash: enabled_mod.source_archive_hash.clone(),
            install_method: enabled_mod.install_method.clone(),
            fomod_config: enabled_mod.fomod_config.clone(),
            files: locked_files,
        });
    }

    let plugin_order = db
        .get_plugin_order(profile_id)
        .await?
        .into_iter()
        .map(plugin_entry_lock)
        .collect();
    let hidden_files = db
        .list_hidden_files(profile_id)
        .await?
        .into_iter()
        .map(hidden_file_lock)
        .collect();
    let patchers = lock_patchers(db, profile).await?;
    let tool_outputs = lock_tool_outputs(db, &profile.game_id).await?;
    let wabbajack_manifest = lock_wabbajack_manifest(&profile.source).await?;

    if !incomplete.is_empty() && !options.allow_incomplete {
        let details = incomplete
            .iter()
            .map(|reason| {
                format!(
                    "{}: {}; regenerate: {}; validate: {}",
                    reason.subject, reason.reason, reason.regenerate, reason.validate
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Err(CoreError::Validation(
            format!(
                "cannot export reproducible modde.lock; missing required provenance:\n{details}"
            )
            .into(),
        ));
    }

    let payload = LockPayload {
        modde_version: env!("CARGO_PKG_VERSION").to_string(),
        generated_at: current_utc_timestamp(),
        reproducible: incomplete.is_empty(),
        incomplete_reasons: incomplete,
        profile: LockProfile {
            name: profile.name.clone(),
            game_id: profile.game_id.to_string(),
            source: profile.source.clone(),
            load_order_lock: profile.load_order_lock.clone(),
            mod_order: profile.mods.iter().map(|m| m.mod_id.clone()).collect(),
        },
        mods: locked_mods,
        plugin_order,
        hidden_files,
        patchers,
        tool_outputs,
        wabbajack_manifest,
    };

    validate_payload(&payload)?;
    Ok(ModdeLock {
        kind: LOCK_KIND.to_string(),
        format_version: LOCK_FORMAT_VERSION,
        payload,
        signatures: Vec::new(),
    })
}
