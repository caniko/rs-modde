//! `OptiScaler` — DLSS/FSR/XeSS upscaling and frame generation replacement.
#![allow(clippy::wildcard_imports)]
//!
//! `OptiScaler` hooks into a game via proxy DLLs (typically `dxgi.dll` or
//! `winmm.dll`). Some games need additional DLLs like `nvngx.dll`.
//!
//! This implementation also subsumes the old fgmod DLL restoration logic:
//! when fgmod deletes certain DLLs at launch time, the launch wrapper
//! restores them.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use futures::StreamExt;
use reqwest::Client;
use smallvec::{SmallVec, smallvec};
use tokio::io::AsyncWriteExt;
use tracing::info;
use xxhash_rust::xxh64::Xxh64;

use crate::optiscaler::{
    OptiScalerIniOverride, OptiScalerProfile, default_optiscaler_profile,
    resolve_optiscaler_profiles,
};

use super::{
    AppliedFiles, GameTool, ToolApplyPreview, ToolAvailability, ToolCategory, ToolConfig,
    ToolGameContext, ToolReleaseAsset, ToolReleaseInstallFuture, ToolReleaseListFuture,
    ToolReleaseSummary,
};
mod apply;
mod archive;
mod backup;
mod config;
mod fgmod;
mod hardware;
mod ini;
mod paths;
mod profiles;
mod releases;
mod runtime;
mod scan;
mod settings;

#[cfg(test)]
mod tests;

pub use archive::extract_optiscaler_archive_flat;
pub use backup::{
    backup_optiscaler_install, latest_optiscaler_backup, restore_latest_optiscaler_backup,
};
pub use fgmod::{fgmod_restore_commands, fgmod_restore_commands_for_executable_dir};
pub use hardware::apply_hardware_defaults;
pub use ini::parse_optiscaler_ini;
pub use paths::{cached_optipatcher_asi, cached_optipatcher_dir, cached_release_dir};
pub use profiles::{
    apply_game_defaults, apply_profile_by_id, managed_manifest_json, managed_paths_from_config,
};
pub use releases::{
    install_latest_optipatcher, install_optiscaler_release_asset,
    install_optiscaler_release_asset_from_path, is_installable_release_asset,
    list_optiscaler_releases, normalize_optiscaler_release_config,
    optiscaler_goverlay_channel_for_tag, optiscaler_release_is_official,
    optiscaler_release_matches_config,
};
pub use scan::{scan_optiscaler_install, scan_optiscaler_install_in_dir};

use apply::*;
use hardware::*;
use ini::*;
use paths::*;
use profiles::*;
use releases::*;
use runtime::*;
use scan::*;
use settings::*;

const CUSTOM_OPTISCALER_PROFILE: &str = "custom";
pub const OPTISCALER_SOURCE_OFFICIAL: &str = "github_release";
pub const OPTISCALER_SOURCE_GOVERLAY_BUILDS: &str = "goverlay_builds";
pub const OPTISCALER_SOURCE_GOVERLAY_FGMOD: &str = "goverlay_fgmod";
pub const FSR4_VARIANT_LATEST_FP8: &str = "latest_fp8";
pub const FSR4_VARIANT_INT8_402: &str = "int8_402";

const FSR4_DLL_NAME: &str = "amd_fidelityfx_upscaler_dx12.dll";
const FSR4_LATEST_DIR: &str = "FSR4_LATEST";
const FSR4_INT8_DIR: &str = "FSR4_INT8";
const OPTIPATCHER_REPO: &str = "optiscaler/OptiPatcher";
const OPTIPATCHER_ASSET: &str = "OptiPatcher.asi";
const FP8_EMULATION_ENV_KEY: &str = "DXIL_SPIRV_CONFIG";
const FP8_EMULATION_ENV_VALUE: &str = "wmma_rdna3_workaround";
pub(crate) const PROTON_FSR4_ENV_KEY: &str = "PROTON_FSR4_UPGRADE";
pub(crate) const PROTON_FSR4_ENV_VALUE: &str = "1";

pub static OPTISCALER: OptiScaler = OptiScaler;

pub struct OptiScaler;

pub const OPTISCALER_PROXY_DLLS: &[&str] = &[
    "dxgi.dll",
    "winmm.dll",
    "d3d12.dll",
    "dbghelp.dll",
    "version.dll",
    "wininet.dll",
    "winhttp.dll",
    "OptiScaler.asi",
];

pub const OPTISCALER_COMPANION_FILES: &[&str] = &[
    "fakenvapi.dll",
    "nvngx-wrapper.dll",
    "nvngx.dll",
    "_nvngx.dll",
    "nvngx_dlss.dll",
];

pub const OPTISCALER_COMPANION_DIRS: &[&str] = &["D3D12_OptiScaler", "plugins"];

/// DLLs that fgmod deletes at launch time. If any of these are deployed by
/// mods or by `OptiScaler` itself, the launch wrapper must restore them.
pub const FGMOD_DELETED_DLLS: &[&str] = &[
    "dxgi.dll",
    "winmm.dll",
    "nvngx.dll",
    "_nvngx.dll",
    "nvngx-wrapper.dll",
    "dlss-enabler.dll",
    "OptiScaler.dll",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptiScalerInstallStatus {
    Absent,
    Managed,
    Unmanaged,
    PartiallyManaged,
    Conflicted,
}

impl std::fmt::Display for OptiScalerInstallStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Absent => write!(f, "absent"),
            Self::Managed => write!(f, "managed"),
            Self::Unmanaged => write!(f, "unmanaged"),
            Self::PartiallyManaged => write!(f, "partially managed"),
            Self::Conflicted => write!(f, "conflicted"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptiScalerVersionIdentity {
    CachedRelease(String),
    FileMetadata(String),
    ContentHash(String),
    Unknown,
}

impl std::fmt::Display for OptiScalerVersionIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CachedRelease(tag) => write!(f, "{tag}"),
            Self::FileMetadata(version) => write!(f, "{version}"),
            Self::ContentHash(hash) => write!(f, "hash:{hash}"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptiScalerDetectedFile {
    pub rel_path: PathBuf,
    pub hash: Option<String>,
    pub managed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptiScalerInstallState {
    pub status: OptiScalerInstallStatus,
    pub executable_dir: PathBuf,
    pub proxy_dlls: Vec<String>,
    pub wine_dll_overrides: Vec<String>,
    pub config_path: Option<PathBuf>,
    pub ini_settings: BTreeMap<String, String>,
    pub companion_files: Vec<PathBuf>,
    pub recognized_files: Vec<OptiScalerDetectedFile>,
    pub version: OptiScalerVersionIdentity,
    pub latest_backup: Option<PathBuf>,
}

impl OptiScalerInstallState {
    #[must_use]
    pub fn summary(&self) -> String {
        let proxy = if self.proxy_dlls.is_empty() {
            "none".to_string()
        } else {
            self.proxy_dlls.join(", ")
        };
        format!("{}; version {}; proxy {}", self.status, self.version, proxy)
    }
}

impl GameTool for OptiScaler {
    fn tool_id(&self) -> &'static str {
        "optiscaler"
    }

    fn display_name(&self) -> &'static str {
        "OptiScaler"
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Upscaler
    }

    fn description(&self) -> &'static str {
        "OptiScaler / fgmod deployment for upscaling and frame-generation proxy DLLs."
    }

    fn settings_schema(&self) -> Vec<super::ToolSettingSpec> {
        let config = self.default_config();
        self.settings_schema_for(None, &config)
    }

    fn settings_schema_for(
        &self,
        context: Option<&ToolGameContext>,
        config: &ToolConfig,
    ) -> Vec<super::ToolSettingSpec> {
        settings_schema_for(context, config)
    }

    fn detect_available(&self) -> ToolAvailability {
        detect_available()
    }

    fn env_vars(&self, config: &ToolConfig) -> SmallVec<[(String, String); 4]> {
        env_vars(config)
    }

    fn wine_dll_overrides(&self, config: &ToolConfig) -> SmallVec<[String; 4]> {
        wine_dll_overrides(config)
    }

    fn apply(&self, game_dir: &Path, config: &ToolConfig) -> Result<AppliedFiles> {
        self.apply_for(game_dir, None, config)
    }

    fn apply_for(
        &self,
        game_dir: &Path,
        context: Option<&ToolGameContext>,
        config: &ToolConfig,
    ) -> Result<AppliedFiles> {
        apply_for(game_dir, context, config)
    }

    fn preview_apply_for(
        &self,
        game_dir: &Path,
        context: Option<&ToolGameContext>,
        config: &ToolConfig,
    ) -> Result<ToolApplyPreview> {
        preview_apply_for(game_dir, context, config)
    }

    fn default_config(&self) -> ToolConfig {
        default_config()
    }

    fn default_config_for(&self, context: Option<&ToolGameContext>) -> ToolConfig {
        default_config_for(context)
    }

    fn supports_releases(&self) -> bool {
        true
    }

    fn list_releases(&self) -> ToolReleaseListFuture<'_> {
        Box::pin(async { list_optiscaler_releases().await })
    }

    fn installable_release_assets(&self, release: &ToolReleaseSummary) -> Vec<String> {
        release
            .assets
            .iter()
            .filter(|asset| is_installable_release_asset(&asset.name))
            .map(|asset| asset.name.clone())
            .collect()
    }

    fn install_release<'a>(
        &'a self,
        _game_id: &'a str,
        mut config: ToolConfig,
        tag: &'a str,
        asset: &'a str,
    ) -> ToolReleaseInstallFuture<'a> {
        Box::pin(async move {
            install_optiscaler_release_asset(tag, asset).await?;
            let normalized_tag = normalize_optiscaler_release_tag(tag);
            apply_optiscaler_release_selection(&mut config, &normalized_tag, asset);
            if config.get_bool("enable_optipatcher") {
                install_latest_optipatcher().await?;
            }
            Ok(config)
        })
    }

    fn install_release_from_path<'a>(
        &'a self,
        _game_id: &'a str,
        mut config: ToolConfig,
        tag: &'a str,
        asset: &'a str,
        path: PathBuf,
    ) -> ToolReleaseInstallFuture<'a> {
        Box::pin(async move {
            install_optiscaler_release_asset_from_path(tag, asset, &path)?;
            let normalized_tag = normalize_optiscaler_release_tag(tag);
            apply_optiscaler_release_selection(&mut config, &normalized_tag, asset);
            if config.get_bool("enable_optipatcher") {
                install_latest_optipatcher().await?;
            }
            Ok(config)
        })
    }
}
