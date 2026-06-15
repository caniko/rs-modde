#![allow(clippy::wildcard_imports)]
use super::*;

pub fn managed_manifest_json(game_dir: &Path, applied: &AppliedFiles) -> serde_json::Value {
    let files = applied
        .files
        .iter()
        .map(|rel| {
            let abs = game_dir.join(rel);
            serde_json::json!({
                "path": rel.to_string_lossy().replace('\\', "/"),
                "hash": file_hash_hex(&abs).ok(),
                "size": abs.metadata().ok().map(|metadata| metadata.len()),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!(files)
}

#[must_use]
pub fn managed_paths_from_config(config: &ToolConfig) -> BTreeSet<String> {
    config
        .settings
        .get("managed_manifest")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("path").and_then(serde_json::Value::as_str))
        .map(normalize_rel_path)
        .collect()
}

/// Apply game-specific `OptiScaler` defaults from community compatibility data.
pub fn apply_game_defaults(config: &mut ToolConfig, context: Option<&ToolGameContext>) {
    let Some(context) = context else {
        return;
    };
    let Some(profile) = selected_or_default_profile(&context.game_id, config) else {
        return;
    };
    apply_optiscaler_profile_metadata(config, profile);
}

/// Apply a community-tested `OptiScaler` profile to a config.
pub fn apply_profile_by_id(config: &mut ToolConfig, game_id: &str, profile_id: &str) -> bool {
    if profile_id == CUSTOM_OPTISCALER_PROFILE {
        apply_custom_profile(config);
        return true;
    }
    let Some(profile) = resolve_optiscaler_profiles(game_id)
        .iter()
        .find(|profile| profile.id == profile_id)
    else {
        return false;
    };
    apply_optiscaler_profile_metadata(config, profile);
    true
}

pub(super) fn selected_or_default_profile(
    game_id: &str,
    config: &ToolConfig,
) -> Option<&'static OptiScalerProfile> {
    if config.get_str("optiscaler_profile") == Some(CUSTOM_OPTISCALER_PROFILE) {
        return None;
    }
    config
        .get_str("optiscaler_profile")
        .and_then(|selected| {
            resolve_optiscaler_profiles(game_id)
                .iter()
                .find(|profile| profile.id == selected)
        })
        .or_else(|| default_optiscaler_profile(game_id))
}

pub(super) fn apply_custom_profile(config: &mut ToolConfig) {
    config.set(
        "optiscaler_profile",
        serde_json::json!(CUSTOM_OPTISCALER_PROFILE),
    );
    config.set(
        "optiscaler_profile_name",
        serde_json::json!("Custom / no community profile"),
    );
    config.set("optiscaler_profile_source_url", serde_json::json!(""));
    config.set("tested_optiscaler_version", serde_json::json!(""));
    config.set(
        "optiscaler_profile_notes",
        serde_json::json!("Community profile guidance is not applied."),
    );
}

pub(super) fn apply_optiscaler_profile_metadata(
    config: &mut ToolConfig,
    profile: &OptiScalerProfile,
) {
    config.set("optiscaler_profile", serde_json::json!(profile.id));
    config.set("optiscaler_profile_name", serde_json::json!(profile.name));
    config.set(
        "optiscaler_profile_source_url",
        serde_json::json!(profile.source_url),
    );
    config.set(
        "tested_optiscaler_version",
        serde_json::json!(profile.tested_optiscaler_version),
    );
    config.set("optiscaler_profile_notes", serde_json::json!(profile.notes));
}

pub(super) fn apply_optiscaler_release_selection(
    config: &mut ToolConfig,
    normalized_tag: &str,
    asset: &str,
) {
    if let Some(channel) = optiscaler_goverlay_channel_for_tag(normalized_tag) {
        config.set(
            "source_mode",
            serde_json::json!(OPTISCALER_SOURCE_GOVERLAY_BUILDS),
        );
        config.set("goverlay_channel", serde_json::json!(channel));
    } else {
        config.set("source_mode", serde_json::json!(OPTISCALER_SOURCE_OFFICIAL));
    }
    config.set("release_tag", serde_json::json!(normalized_tag));
    config.set("release_asset", serde_json::json!(asset));
}
