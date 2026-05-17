use std::fs;
use std::sync::OnceLock;

use dashmap::DashMap;
use tracing::warn;

use crate::generic::leak::{slice as leak_slice, str as leak_str};

use super::spec::{IniOverrideSpec, OptiScalerProfileSpec, OptiScalerProfilesSpec};
use super::{OptiScalerIniOverride, OptiScalerProfile};

static USER_PROFILE_CACHE: OnceLock<DashMap<String, &'static [OptiScalerProfile]>> =
    OnceLock::new();

fn cache() -> &'static DashMap<String, &'static [OptiScalerProfile]> {
    USER_PROFILE_CACHE.get_or_init(DashMap::new)
}

#[must_use]
pub fn load_user_profiles(game_id: &str) -> &'static [OptiScalerProfile] {
    if let Some(profiles) = cache().get(game_id) {
        return *profiles;
    }

    let path = modde_core::paths::modde_data_dir()
        .join("games")
        .join(format!("{game_id}.optiscaler.toml"));

    let profiles = match fs::read_to_string(&path) {
        Ok(content) => match toml::from_str::<OptiScalerProfilesSpec>(&content) {
            Ok(spec) => load_profiles_from_spec(spec),
            Err(error) => {
                warn!(path = %path.display(), error = %error, "skipping user OptiScaler profile spec");
                &[]
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => &[],
        Err(error) => {
            warn!(path = %path.display(), error = %error, "skipping user OptiScaler profile spec");
            &[]
        }
    };

    *cache().entry(game_id.to_string()).or_insert(profiles)
}

fn load_profiles_from_spec(spec: OptiScalerProfilesSpec) -> &'static [OptiScalerProfile] {
    let profiles = spec
        .profile
        .into_iter()
        .map(convert_profile)
        .collect::<Vec<_>>();
    leak_slice(profiles)
}

fn convert_profile(spec: OptiScalerProfileSpec) -> OptiScalerProfile {
    OptiScalerProfile {
        id: leak_str(spec.id),
        name: leak_str(spec.name),
        source_url: leak_str(spec.source_url),
        tested_optiscaler_version: leak_str(spec.tested_optiscaler_version),
        source_mode: spec.source_mode.map(leak_str),
        goverlay_channel: spec.goverlay_channel.map(leak_str),
        proxy_dll: leak_str(spec.proxy_dll),
        release_tag: spec.release_tag.map(leak_str),
        release_asset: spec.release_asset.map(leak_str),
        wine_dll_overrides: leak_slice(
            spec.wine_dll_overrides
                .into_iter()
                .map(leak_str)
                .collect::<Vec<_>>(),
        ),
        copy_companion_files: spec.copy_companion_files,
        enable_optipatcher: spec.enable_optipatcher,
        fsr4_variant: spec.fsr4_variant.map(leak_str),
        emulate_fp8: spec.emulate_fp8,
        spoof_dlss: spec.spoof_dlss,
        ini_overrides: leak_slice(
            spec.ini_overrides
                .into_iter()
                .map(convert_ini_override)
                .collect::<Vec<_>>(),
        ),
        notes: leak_str(spec.notes),
    }
}

fn convert_ini_override(spec: IniOverrideSpec) -> OptiScalerIniOverride {
    OptiScalerIniOverride {
        key: leak_str(spec.key),
        value: leak_str(spec.value),
    }
}
