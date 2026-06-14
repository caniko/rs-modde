use std::collections::HashSet;
use std::path::Path;

use super::{LOCK_FORMAT_VERSION, LOCK_KIND, LockPayload, ModdeLock};
use crate::error::{CoreError, Result};

pub fn validate_lock(lock: &ModdeLock) -> Result<()> {
    if lock.kind != LOCK_KIND {
        return Err(CoreError::Validation(
            format!("unsupported lock kind '{}'", lock.kind).into(),
        ));
    }
    if lock.format_version > LOCK_FORMAT_VERSION {
        return Err(CoreError::Validation(
            format!(
                "unsupported modde.lock format version {} (supported major {})",
                lock.format_version, LOCK_FORMAT_VERSION
            )
            .into(),
        ));
    }
    validate_payload(&lock.payload)
}

pub(in crate::lockfile) fn validate_payload(payload: &LockPayload) -> Result<()> {
    validate_path_component(&payload.profile.name, "profile name")?;
    let mut seen_mods = HashSet::new();
    for locked_mod in &payload.mods {
        validate_path_component(&locked_mod.mod_id, "mod id")?;
        if !seen_mods.insert(&locked_mod.mod_id) {
            return Err(CoreError::Validation(
                format!("duplicate mod id '{}'", locked_mod.mod_id).into(),
            ));
        }
        let mut seen_files = HashSet::new();
        for file in &locked_mod.files {
            validate_relative_path(&file.rel_path)?;
            if !seen_files.insert(&file.rel_path) {
                return Err(CoreError::Validation(
                    format!(
                        "duplicate file '{}' in mod '{}'",
                        file.rel_path, locked_mod.mod_id
                    )
                    .into(),
                ));
            }
            validate_relative_path(&file.origin_rel_path)?;
        }
    }
    let mut seen_plugins = HashSet::new();
    for plugin in &payload.plugin_order {
        if !seen_plugins.insert(plugin.plugin_name.to_ascii_lowercase()) {
            return Err(CoreError::Validation(
                format!("duplicate plugin '{}'", plugin.plugin_name).into(),
            ));
        }
    }
    let mut seen_hidden = HashSet::new();
    for hidden in &payload.hidden_files {
        validate_relative_path(&hidden.rel_path)?;
        if !seen_hidden.insert((&hidden.mod_id, &hidden.rel_path)) {
            return Err(CoreError::Validation(
                format!(
                    "duplicate hidden file '{}:{}'",
                    hidden.mod_id, hidden.rel_path
                )
                .into(),
            ));
        }
    }
    let mut seen_patchers = HashSet::new();
    for patcher in &payload.patchers {
        validate_path_component(&patcher.name, "patcher stage name")?;
        if !seen_patchers.insert(&patcher.name) {
            return Err(CoreError::Validation(
                format!("duplicate patcher stage '{}'", patcher.name).into(),
            ));
        }
        let mut seen_outputs = HashSet::new();
        for output in &patcher.outputs {
            validate_relative_path(&output.rel_path)?;
            if !seen_outputs.insert(&output.rel_path) {
                return Err(CoreError::Validation(
                    format!(
                        "duplicate patcher output '{}' in stage '{}'",
                        output.rel_path, patcher.name
                    )
                    .into(),
                ));
            }
        }
    }
    let mut seen_tool_outputs = HashSet::new();
    for output in &payload.tool_outputs {
        validate_relative_path(&output.rel_path)?;
        if !seen_tool_outputs.insert((&output.tool_id, &output.rel_path)) {
            return Err(CoreError::Validation(
                format!(
                    "duplicate tool output '{}:{}'",
                    output.tool_id, output.rel_path
                )
                .into(),
            ));
        }
    }
    Ok(())
}

pub(in crate::lockfile) fn validate_relative_path(path: &str) -> Result<()> {
    let candidate = Path::new(path);
    if path.is_empty()
        || candidate.is_absolute()
        || path.contains('\\')
        || candidate
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(CoreError::Validation(
            format!("invalid relative path in modde.lock: {path}").into(),
        ));
    }
    Ok(())
}

fn validate_path_component(value: &str, label: &str) -> Result<()> {
    if value.is_empty() || value.contains(['/', '\\', '\0']) || value == "." || value == ".." {
        return Err(CoreError::Validation(
            format!("invalid {label} in modde.lock: {value}").into(),
        ));
    }
    Ok(())
}
