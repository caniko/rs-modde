//! `OptiScaler` — DLSS/FSR/XeSS upscaling and frame generation replacement.
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
    OptiScalerProfile, default_optiscaler_profile, resolve_optiscaler_profiles,
};

use super::{
    AppliedFiles, GameTool, ToolApplyPreview, ToolAvailability, ToolCategory, ToolConfig,
    ToolGameContext, ToolReleaseAsset, ToolReleaseInstallFuture, ToolReleaseListFuture,
    ToolReleaseSummary,
};

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
        let mut specs = vec![
            super::ToolSettingSpec::labeled_select(
                "source_mode",
                "Source",
                "Where modde should get OptiScaler files from.",
                &[
                    (OPTISCALER_SOURCE_OFFICIAL, "Official GitHub releases"),
                    (OPTISCALER_SOURCE_GOVERLAY_BUILDS, "GOverlay builds"),
                    (OPTISCALER_SOURCE_GOVERLAY_FGMOD, "GOverlay fgmod directory"),
                    ("local_dir", "Local OptiScaler directory"),
                ],
            )
            .section("Source"),
            super::ToolSettingSpec::labeled_select(
                "goverlay_channel",
                "GOverlay channel",
                "GOverlay OptiScaler builds channel.",
                &[
                    ("edge", "Bleeding-edge"),
                    ("stable", "Stable"),
                    ("master", "Master"),
                    ("any", "Any release branch"),
                ],
            )
            .section("Source"),
            super::ToolSettingSpec::select(
                "release_tag",
                "Release tag",
                "OptiScaler release tag selected in the UI.",
                std::slice::from_ref(&config.get_str("release_tag").unwrap_or("latest")),
            )
            .section("Source"),
            super::ToolSettingSpec::select(
                "release_asset",
                "Release asset",
                "Release asset selected from GitHub.",
                std::slice::from_ref(&config.get_str("release_asset").unwrap_or("")),
            )
            .section("Source"),
            super::ToolSettingSpec::labeled_select(
                "proxy_dll",
                "Proxy DLL",
                "DLL name used to load OptiScaler for this game.",
                &[
                    ("dxgi.dll", "dxgi.dll - DirectX graphics proxy"),
                    ("version.dll", "version.dll - Windows version proxy"),
                    ("dbghelp.dll", "dbghelp.dll - Debug helper proxy"),
                    ("d3d12.dll", "d3d12.dll - Direct3D 12 proxy"),
                    ("wininet.dll", "wininet.dll - WinINet proxy"),
                    ("winhttp.dll", "winhttp.dll - WinHTTP proxy"),
                    ("winmm.dll", "winmm.dll - Multimedia proxy"),
                    ("nvngx.dll", "nvngx.dll - NVIDIA NGX proxy"),
                    ("OptiScaler.asi", "OptiScaler.asi - ASI plugin"),
                ],
            )
            .section("Basic"),
            super::ToolSettingSpec::text(
                "dll_overrides",
                "DLL overrides",
                "Comma or whitespace separated Wine DLL override base names.",
            )
            .section("Basic"),
            super::ToolSettingSpec::bool(
                "copy_companion_files",
                "Copy companion files",
                "Copy fakenvapi, nvngx wrapper, and other DLLs found next to OptiScaler.",
            )
            .section("Basic"),
            super::ToolSettingSpec::labeled_select(
                "fsr4_variant",
                "FSR4 variant",
                "FSR4 payload copied as amd_fidelityfx_upscaler_dx12.dll.",
                &[
                    (FSR4_VARIANT_LATEST_FP8, "Latest (FP8)"),
                    (FSR4_VARIANT_INT8_402, "4.0.2c (INT8)"),
                ],
            )
            .section("Basic"),
            super::ToolSettingSpec::bool(
                "enable_optipatcher",
                "OptiPatcher",
                "Use OptiPatcher to unlock DLSS and DLSS frame generation inputs without whole-game spoofing in supported games.",
            )
            .section("Basic"),
            super::ToolSettingSpec::bool(
                "spoof_dlss",
                "Spoof DLSS fallback",
                "Fallback DXGI spoofing path for games that still need whole-game spoofing.",
            )
            .section("Basic"),
            super::ToolSettingSpec::read_only(
                "derived_executable_dir",
                "Executable directory",
                "Derived from the selected game's metadata.",
            )
            .section("Detected Game"),
        ];

        if let Some(context) = context {
            let profiles = resolve_optiscaler_profiles(&context.game_id);
            if !profiles.is_empty() {
                let profile_options = std::iter::once(super::ToolSelectOption::new(
                    CUSTOM_OPTISCALER_PROFILE,
                    "Custom / no community profile",
                ))
                .chain(
                    profiles
                        .iter()
                        .map(|profile| super::ToolSelectOption::new(profile.id, profile.name)),
                )
                .collect();
                specs.insert(
                    0,
                    super::ToolSettingSpec {
                        key: "optiscaler_profile",
                        label: "Profile",
                        description: "Community-tested OptiScaler profile to apply, or custom settings.",
                        section: "Profile",
                        advanced: false,
                        kind: super::ToolSettingKind::Select { options: profile_options },
                    },
                );
                specs.push(
                    super::ToolSettingSpec::read_only(
                        "optiscaler_profile_source_url",
                        "Profile source",
                        "Community compatibility source for the selected profile.",
                    )
                    .section("Profile"),
                );
                specs.push(
                    super::ToolSettingSpec::read_only(
                        "tested_optiscaler_version",
                        "Tested version",
                        "OptiScaler version reported by the selected profile.",
                    )
                    .section("Profile"),
                );
                specs.push(
                    super::ToolSettingSpec::read_only(
                        "optiscaler_profile_notes",
                        "Profile notes",
                        "Community notes for the selected profile.",
                    )
                    .section("Profile"),
                );
            }
        }

        if config.get_str("source_mode") == Some("local_dir") {
            specs.insert(
                1,
                super::ToolSettingSpec::path(
                    "local_source_dir",
                    "Local source directory",
                    "Directory containing OptiScaler.dll and companion files.",
                )
                .section("Source"),
            );
        }
        if config.get_str("source_mode") != Some(OPTISCALER_SOURCE_GOVERLAY_BUILDS) {
            specs.retain(|spec| spec.key != "goverlay_channel");
        }

        if config.get_str("fsr4_variant") == Some(FSR4_VARIANT_INT8_402) {
            specs.push(
                super::ToolSettingSpec::read_only(
                    "emulate_fp8",
                    "Emulate FP8",
                    "Only applies to the Latest (FP8) FSR4 variant.",
                )
                .section("Basic"),
            );
        } else {
            specs.push(
                super::ToolSettingSpec::bool(
                    "emulate_fp8",
                    "Emulate FP8",
                    "Set DXIL_SPIRV_CONFIG=wmma_rdna3_workaround for the Latest (FP8) FSR4 variant.",
                )
                .section("Basic"),
            );
        }

        specs.extend(goverlay_optiscaler_ini_specs());
        specs.extend(optiscaler_ini_specs(config));
        specs
    }

    fn detect_available(&self) -> ToolAvailability {
        // Check common locations for OptiScaler
        let candidates = [
            dirs::data_dir()
                .unwrap_or_default()
                .join("goverlay/fgmod/OptiScaler.dll"),
            dirs::home_dir()
                .unwrap_or_default()
                .join(".local/share/goverlay/fgmod/OptiScaler.dll"),
        ];

        for path in &candidates {
            if path.exists() {
                return ToolAvailability::Available {
                    version: Some("fgmod".into()),
                };
            }
        }

        // User can provide a custom path via settings
        ToolAvailability::Available {
            version: Some("user-provided".into()),
        }
    }

    fn env_vars(&self, config: &ToolConfig) -> SmallVec<[(String, String); 4]> {
        if config.get_bool("emulate_fp8")
            && config.get_str("fsr4_variant") == Some(FSR4_VARIANT_LATEST_FP8)
        {
            smallvec![(
                FP8_EMULATION_ENV_KEY.to_string(),
                FP8_EMULATION_ENV_VALUE.to_string()
            )]
        } else {
            SmallVec::new()
        }
    }

    fn wine_dll_overrides(&self, config: &ToolConfig) -> SmallVec<[String; 4]> {
        let primary = config
            .get_str("proxy_dll")
            .or_else(|| config.get_str("dll_name"))
            .unwrap_or("dxgi.dll")
            .trim_end_matches(".dll")
            .to_string();
        let mut overrides: SmallVec<[String; 4]> = smallvec![primary];

        if config.get_bool("needs_winmm") && !overrides.iter().any(|value| value == "winmm") {
            overrides.push("winmm".into());
        }
        if let Some(raw) = config.get_str("dll_overrides") {
            for part in raw.split([',', ';', ' ', '\n', '\t']) {
                let value = part.trim().trim_end_matches(".dll");
                if !value.is_empty() && !overrides.iter().any(|existing| existing == value) {
                    overrides.push(value.to_string());
                }
            }
        }

        overrides
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
        let source_dir = resolve_source_dir(config).context(
            "optiscaler: choose a GitHub release, local directory, or install fgmod/goverlay",
        )?;

        let dll_name = config
            .get_str("proxy_dll")
            .or_else(|| config.get_str("dll_name"))
            .unwrap_or("dxgi.dll");
        let target_dir = context
            .and_then(|context| context.executable_dir.clone())
            .unwrap_or_else(|| legacy_target_dir(game_dir, config));

        std::fs::create_dir_all(&target_dir)?;
        let applied_paths = managed_paths_from_config(config);
        let existing = scan_optiscaler_install_in_dir(&target_dir, &applied_paths)?;
        if matches!(
            existing.status,
            OptiScalerInstallStatus::Unmanaged
                | OptiScalerInstallStatus::PartiallyManaged
                | OptiScalerInstallStatus::Conflicted
        ) {
            backup_optiscaler_install(context.map(|context| context.game_id.as_str()), &existing)?;
        }

        let mut applied = AppliedFiles::default();

        // Copy OptiScaler as the requested DLL name
        let optiscaler_dll = source_dir.join("OptiScaler.dll");
        if optiscaler_dll.exists() {
            let dest = target_dir.join(dll_name);
            std::fs::copy(&optiscaler_dll, &dest)
                .with_context(|| format!("failed to copy OptiScaler to {}", dest.display()))?;
            let rel = relative_to_game(game_dir, &dest)?;
            applied.files.push(rel);
            info!(as_dll = %dll_name, "applied OptiScaler DLL");
        }

        // Copy OptiScaler.ini if present (or generate default)
        let ini_src = source_dir.join("OptiScaler.ini");
        let ini_dest = target_dir.join("OptiScaler.ini");
        if ini_src.exists() {
            let reset_reason = optiscaler_config_reset_reason(&existing, &ini_src, config);
            if reset_reason.is_some() {
                apply_ini_overrides_with_existing(&ini_src, None, &ini_dest, config)?;
            } else {
                apply_ini_overrides_with_existing(
                    &ini_src,
                    ini_dest.exists().then_some(&ini_dest),
                    &ini_dest,
                    config,
                )?;
            }
            let rel = relative_to_game(game_dir, &ini_dest)?;
            applied.files.push(rel);
        }

        // Copy additional DLLs from source (fakenvapi, nvngx-wrapper, etc.)
        if config.get_bool("copy_companion_files") {
            for entry in std::fs::read_dir(&source_dir)?.flatten() {
                let src = entry.path();
                if !src.is_file() {
                    continue;
                }
                let Some(name) = src.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if !name.eq_ignore_ascii_case("OptiScaler.dll")
                    && !name.eq_ignore_ascii_case("OptiScaler.ini")
                    && !name.eq_ignore_ascii_case(FSR4_DLL_NAME)
                    && name.to_ascii_lowercase().ends_with(".dll")
                {
                    let dest = target_dir.join(name);
                    std::fs::copy(&src, &dest)?;
                    let rel = relative_to_game(game_dir, &dest)?;
                    applied.files.push(rel);
                }
            }
        }

        if let Some(fsr4_src) = selected_fsr4_variant_source(&source_dir, config) {
            if !fsr4_src.is_file() {
                anyhow::bail!(
                    "optiscaler: selected FSR4 variant '{}' was not found at {}",
                    fsr4_variant(config),
                    fsr4_src.display()
                );
            }
            let dest = target_dir.join(FSR4_DLL_NAME);
            std::fs::copy(&fsr4_src, &dest)
                .with_context(|| format!("failed to copy FSR4 variant to {}", dest.display()))?;
            let rel = relative_to_game(game_dir, &dest)?;
            applied.files.push(rel);
        }

        if config.get_bool("enable_optipatcher") {
            let optipatcher_src = optipatcher_asi_source(&source_dir);
            if !optipatcher_src.is_file() {
                anyhow::bail!(
                    "optiscaler: OptiPatcher.asi is required but not cached; install or update the selected OptiScaler release first"
                );
            }
            let dest = target_dir.join("plugins").join(OPTIPATCHER_ASSET);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&optipatcher_src, &dest)
                .with_context(|| format!("failed to copy OptiPatcher.asi to {}", dest.display()))?;
            let rel = relative_to_game(game_dir, &dest)?;
            applied.files.push(rel);
        }

        if source_dir.join("D3D12_OptiScaler").is_dir() {
            let dest = target_dir.join("D3D12_OptiScaler");
            copy_dir_recursive(&source_dir.join("D3D12_OptiScaler"), &dest)?;
            collect_relative_files(game_dir, &dest, &mut applied.files)?;
        }

        Ok(applied)
    }

    fn preview_apply_for(
        &self,
        game_dir: &Path,
        context: Option<&ToolGameContext>,
        config: &ToolConfig,
    ) -> Result<ToolApplyPreview> {
        let Some(source_dir) = resolve_source_dir(config) else {
            return Ok(optiscaler_missing_preview(
                "optiscaler: choose a GitHub release, local directory, or install fgmod/goverlay",
            ));
        };

        let dll_name = config
            .get_str("proxy_dll")
            .or_else(|| config.get_str("dll_name"))
            .unwrap_or("dxgi.dll");
        let target_dir = context
            .and_then(|context| context.executable_dir.clone())
            .unwrap_or_else(|| legacy_target_dir(game_dir, config));
        let applied_paths = managed_paths_from_config(config);
        let existing = scan_optiscaler_install_in_dir(&target_dir, &applied_paths)?;
        let mut preview = ToolApplyPreview::default();

        let optiscaler_dll = source_dir.join("OptiScaler.dll");
        if optiscaler_dll.is_file() {
            preview_source_file(
                game_dir,
                &optiscaler_dll,
                &target_dir.join(dll_name),
                &mut preview,
            )?;
        } else {
            preview.missing_inputs.push(format!(
                "optiscaler: OptiScaler.dll not found in {}",
                source_dir.display()
            ));
        }

        let ini_src = source_dir.join("OptiScaler.ini");
        let ini_dest = target_dir.join("OptiScaler.ini");
        if ini_src.is_file() {
            let reset_reason = optiscaler_config_reset_reason(&existing, &ini_src, config);
            let content = if reset_reason.is_some() {
                build_ini_with_overrides(&ini_src, None, config)?.into_bytes()
            } else {
                build_ini_with_overrides(
                    &ini_src,
                    ini_dest.exists().then_some(ini_dest.as_path()),
                    config,
                )?
                .into_bytes()
            };
            preview_bytes(game_dir, &ini_dest, &content, &mut preview);
        } else {
            preview.missing_inputs.push(format!(
                "optiscaler: OptiScaler.ini not found in {}",
                source_dir.display()
            ));
        }

        if config.get_bool("copy_companion_files") {
            for entry in std::fs::read_dir(&source_dir)?.flatten() {
                let src = entry.path();
                if !src.is_file() {
                    continue;
                }
                let Some(name) = src.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if !name.eq_ignore_ascii_case("OptiScaler.dll")
                    && !name.eq_ignore_ascii_case("OptiScaler.ini")
                    && !name.eq_ignore_ascii_case(FSR4_DLL_NAME)
                    && name.to_ascii_lowercase().ends_with(".dll")
                {
                    preview_source_file(game_dir, &src, &target_dir.join(name), &mut preview)?;
                }
            }
        }

        if let Some(fsr4_src) = selected_fsr4_variant_source(&source_dir, config) {
            if fsr4_src.is_file() {
                preview_source_file(
                    game_dir,
                    &fsr4_src,
                    &target_dir.join(FSR4_DLL_NAME),
                    &mut preview,
                )?;
            } else {
                preview.missing_inputs.push(format!(
                    "optiscaler: selected FSR4 variant '{}' not found at {}",
                    fsr4_variant(config),
                    fsr4_src.display()
                ));
            }
        }

        if config.get_bool("enable_optipatcher") {
            let optipatcher_src = optipatcher_asi_source(&source_dir);
            if optipatcher_src.is_file() {
                preview_source_file(
                    game_dir,
                    &optipatcher_src,
                    &target_dir.join("plugins").join(OPTIPATCHER_ASSET),
                    &mut preview,
                )?;
            } else {
                preview.missing_inputs.push(
                    "optiscaler: OptiPatcher.asi is required but not cached; install or update the selected OptiScaler release first"
                        .to_string(),
                );
            }
        }

        let d3d12_src = source_dir.join("D3D12_OptiScaler");
        if d3d12_src.is_dir() {
            preview_dir_recursive(
                game_dir,
                &d3d12_src,
                &target_dir.join("D3D12_OptiScaler"),
                &mut preview,
            )?;
        }

        Ok(preview)
    }

    fn default_config(&self) -> ToolConfig {
        let mut config = ToolConfig::new("optiscaler");
        config.set(
            "source_mode",
            serde_json::json!(OPTISCALER_SOURCE_GOVERLAY_FGMOD),
        );
        config.set("goverlay_channel", serde_json::json!("edge"));
        config.set("release_tag", serde_json::json!("latest"));
        config.set("release_asset", serde_json::json!(""));
        config.set("local_source_dir", serde_json::json!(""));
        config.set("proxy_dll", serde_json::json!("dxgi.dll"));
        config.set("dll_overrides", serde_json::json!(""));
        config.set("copy_companion_files", serde_json::json!(true));
        config.set("enable_optipatcher", serde_json::json!(false));
        config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_LATEST_FP8));
        config.set("emulate_fp8", serde_json::json!(false));
        config.set("spoof_dlss", serde_json::json!(false));
        config.set("ini_overrides", serde_json::json!({}));
        config
    }

    fn default_config_for(&self, context: Option<&ToolGameContext>) -> ToolConfig {
        let mut config = self.default_config();
        apply_game_defaults(&mut config, context);
        config
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
}

pub async fn list_optiscaler_releases() -> Result<Vec<ToolReleaseSummary>> {
    let mut releases = official_optiscaler_releases().await?;
    releases.extend(goverlay_optiscaler_releases().await?);
    Ok(releases)
}

#[must_use]
pub fn normalize_optiscaler_release_config(config: &mut ToolConfig) -> bool {
    let mut changed = false;
    if let Some(tag) = config.get_str("release_tag").map(str::to_string)
        && !tag.trim().is_empty()
    {
        let normalized = normalize_optiscaler_release_tag(&tag);
        if normalized != tag {
            config.set("release_tag", serde_json::json!(normalized));
            changed = true;
        }
    }
    if let Some(tag) = config.get_str("release_tag").map(str::to_string) {
        if let Some(channel) = optiscaler_goverlay_channel_for_tag(&tag) {
            if config.get_str("source_mode") != Some(OPTISCALER_SOURCE_GOVERLAY_BUILDS) {
                config.set(
                    "source_mode",
                    serde_json::json!(OPTISCALER_SOURCE_GOVERLAY_BUILDS),
                );
                changed = true;
            }
            if config.get_str("goverlay_channel") != Some(channel) {
                config.set("goverlay_channel", serde_json::json!(channel));
                changed = true;
            }
        } else if !tag.trim().is_empty()
            && optiscaler_release_is_official(&tag)
            && config.get_str("source_mode").is_none()
        {
            config.set("source_mode", serde_json::json!(OPTISCALER_SOURCE_OFFICIAL));
            changed = true;
        }
    }
    changed
}

#[must_use]
pub fn optiscaler_release_matches_config(
    release: &ToolReleaseSummary,
    config: &ToolConfig,
) -> bool {
    match config
        .get_str("source_mode")
        .unwrap_or(OPTISCALER_SOURCE_GOVERLAY_FGMOD)
    {
        OPTISCALER_SOURCE_OFFICIAL => optiscaler_release_is_official(&release.tag),
        OPTISCALER_SOURCE_GOVERLAY_BUILDS => {
            optiscaler_goverlay_channel_for_tag(&release.tag)
                == Some(config.get_str("goverlay_channel").unwrap_or("edge"))
        }
        _ => false,
    }
}

#[must_use]
pub fn optiscaler_release_is_official(tag: &str) -> bool {
    normalize_optiscaler_release_tag(tag).starts_with("official:")
}

#[must_use]
pub fn optiscaler_goverlay_channel_for_tag(tag: &str) -> Option<&'static str> {
    let encoded = normalize_optiscaler_release_tag(tag);
    let channel = encoded
        .strip_prefix("goverlay-")
        .and_then(|rest| rest.split_once(':'))
        .map(|(channel, _)| channel)?;
    match channel {
        "edge" => Some("edge"),
        "stable" => Some("stable"),
        "master" => Some("master"),
        "any" => Some("any"),
        _ => None,
    }
}

pub async fn install_optiscaler_release_asset(tag: &str, asset_name: &str) -> Result<PathBuf> {
    let releases = list_optiscaler_releases().await?;
    let (normalized_tag, asset) = select_optiscaler_release_asset(&releases, tag, asset_name)?;
    if !is_installable_release_asset(&asset.name) {
        anyhow::bail!(
            "selected asset '{}' is not a supported archive (.zip or .7z)",
            asset.name
        );
    }

    let cache_dir = cached_release_dir(&normalized_tag);
    std::fs::create_dir_all(&cache_dir)?;
    let archive_path = cache_dir.join(&asset.name);
    download_release_asset(asset, &archive_path).await?;
    extract_optiscaler_archive_flat(&archive_path, &cache_dir)?;
    Ok(cache_dir)
}

pub async fn install_latest_optipatcher() -> Result<PathBuf> {
    let release = super::release::list_github_releases(OPTIPATCHER_REPO)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("OptiPatcher release not found"))?;
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name.eq_ignore_ascii_case(OPTIPATCHER_ASSET))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "OptiPatcher release {} did not contain {}",
                release.tag,
                OPTIPATCHER_ASSET
            )
        })?;
    let dest = cached_optipatcher_asi();
    download_release_asset(asset, &dest).await?;
    Ok(dest)
}

fn normalize_optiscaler_release_tag(tag: &str) -> String {
    if tag.contains(':') {
        tag.to_string()
    } else {
        encode_optiscaler_release_tag("official", tag)
    }
}

fn select_optiscaler_release_asset<'a>(
    releases: &'a [ToolReleaseSummary],
    tag: &str,
    asset_name: &str,
) -> Result<(String, &'a ToolReleaseAsset)> {
    let normalized_tag = normalize_optiscaler_release_tag(tag);
    let release = releases
        .iter()
        .find(|release| release.tag == normalized_tag)
        .ok_or_else(|| anyhow::anyhow!("OptiScaler release tag not found: {tag}"))?;
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .ok_or_else(|| anyhow::anyhow!("asset '{asset_name}' not found in release {tag}"))?;
    Ok((normalized_tag, asset))
}

async fn official_optiscaler_releases() -> Result<Vec<ToolReleaseSummary>> {
    Ok(
        super::release::list_github_releases("optiscaler/OptiScaler")
            .await?
            .into_iter()
            .map(|mut release| {
                release.tag = encode_optiscaler_release_tag("official", &release.tag);
                release
            })
            .collect(),
    )
}

async fn goverlay_optiscaler_releases() -> Result<Vec<ToolReleaseSummary>> {
    let mut releases: Vec<_> =
        super::release::list_github_releases("benjamimgois/OptiScaler-builds")
            .await?
            .into_iter()
            .filter_map(goverlay_release_summary)
            .collect();
    releases.sort_by(|left, right| right.published_at.cmp(&left.published_at));
    Ok(releases)
}

fn goverlay_release_summary(mut release: ToolReleaseSummary) -> Option<ToolReleaseSummary> {
    let channel = goverlay_release_channel(&release.tag)?;
    let assets = goverlay_installable_assets(channel, &release);
    if assets.is_empty() {
        return None;
    }
    release.tag = encode_optiscaler_release_tag(channel, &release.tag);
    release.assets = assets;
    Some(release)
}

fn encode_optiscaler_release_tag(source: &str, tag: &str) -> String {
    if tag.contains(':') {
        tag.to_string()
    } else {
        format!("{source}:{tag}")
    }
}

fn goverlay_release_channel(tag: &str) -> Option<&'static str> {
    if tag.starts_with("edge-") {
        Some("goverlay-edge")
    } else if tag.starts_with("master-") {
        Some("goverlay-master")
    } else if tag.starts_with("any-release-") {
        Some("goverlay-any")
    } else if is_goverlay_stable_tag(tag) {
        Some("goverlay-stable")
    } else {
        None
    }
}

fn is_goverlay_stable_tag(tag: &str) -> bool {
    let (version, patch) = tag
        .split_once('-')
        .map_or((tag, None), |(version, patch)| (version, Some(patch)));
    if let Some(patch) = patch
        && (patch.is_empty() || !patch.chars().all(|ch| ch.is_ascii_digit()))
    {
        return false;
    }
    let mut parts = version.split('.');
    let Some(major) = parts.next() else {
        return false;
    };
    let Some(minor) = parts.next() else {
        return false;
    };
    let Some(patch) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && [major, minor, patch]
            .into_iter()
            .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
}

fn goverlay_installable_assets(
    channel: &str,
    release: &ToolReleaseSummary,
) -> Vec<ToolReleaseAsset> {
    match channel {
        "goverlay-stable" => release_assets_named(release, "optiScaler-stable.7z"),
        "goverlay-edge" => release_assets_named(release, "optiscaler-edge.7z"),
        "goverlay-master" | "goverlay-any" => release
            .assets
            .iter()
            .filter(|asset| {
                let lower = asset.name.to_ascii_lowercase();
                lower.ends_with(".7z") && !lower.ends_with(".json")
            })
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}

fn release_assets_named(
    release: &ToolReleaseSummary,
    expected_name: &str,
) -> Vec<ToolReleaseAsset> {
    release
        .assets
        .iter()
        .filter(|asset| asset.name.eq_ignore_ascii_case(expected_name))
        .cloned()
        .collect()
}

async fn download_release_asset(asset: &ToolReleaseAsset, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let client = Client::new();
    let response = client
        .get(&asset.download_url)
        .header("User-Agent", "modde")
        .send()
        .await?
        .error_for_status()?;
    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        file.write_all(&chunk?).await?;
    }
    file.flush().await?;
    Ok(())
}

#[must_use]
pub fn is_installable_release_asset(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".zip") || lower.ends_with(".7z")
}

fn legacy_target_dir(game_dir: &Path, config: &ToolConfig) -> PathBuf {
    let exe_subdir = config.get_str("exe_subdir").unwrap_or("");
    if exe_subdir.is_empty() {
        game_dir.to_path_buf()
    } else {
        game_dir.join(exe_subdir)
    }
}

fn relative_to_game(game_dir: &Path, dest: &Path) -> Result<PathBuf> {
    dest.strip_prefix(game_dir)
        .map(Path::to_path_buf)
        .with_context(|| {
            format!(
                "optiscaler: destination {} is not under game dir {}",
                dest.display(),
                game_dir.display()
            )
        })
}

#[must_use]
pub fn cached_release_dir(tag: &str) -> PathBuf {
    modde_core::paths::modde_data_dir()
        .join("tools")
        .join("optiscaler")
        .join(sanitize_tag(tag))
}

#[must_use]
pub fn cached_optipatcher_dir() -> PathBuf {
    modde_core::paths::modde_data_dir()
        .join("tools")
        .join("optipatcher")
        .join("rolling")
}

#[must_use]
pub fn cached_optipatcher_asi() -> PathBuf {
    cached_optipatcher_dir().join(OPTIPATCHER_ASSET)
}

fn sanitize_tag(tag: &str) -> String {
    tag.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn resolve_source_dir(config: &ToolConfig) -> Option<PathBuf> {
    match config.get_str("source_mode").unwrap_or("goverlay_fgmod") {
        OPTISCALER_SOURCE_OFFICIAL | OPTISCALER_SOURCE_GOVERLAY_BUILDS => {
            let tag = config.get_str("release_tag")?;
            cached_release_source_dir(tag)
        }
        "local_dir" => config
            .get_str("local_source_dir")
            .or_else(|| config.get_str("source_dir"))
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from),
        _ => {
            let fgmod = dirs::home_dir()?.join(".local/share/goverlay/fgmod");
            fgmod.is_dir().then_some(fgmod)
        }
    }
}

fn cached_release_source_dir(tag: &str) -> Option<PathBuf> {
    let candidates = if let Some(official_tag) = tag.strip_prefix("official:") {
        vec![cached_release_dir(tag), cached_release_dir(official_tag)]
    } else {
        let normalized = normalize_optiscaler_release_tag(tag);
        if normalized == tag {
            vec![cached_release_dir(tag)]
        } else {
            vec![cached_release_dir(tag), cached_release_dir(&normalized)]
        }
    };
    candidates
        .into_iter()
        .find(|dir| dir.join("OptiScaler.dll").exists())
}

fn optiscaler_missing_preview(message: impl Into<String>) -> ToolApplyPreview {
    ToolApplyPreview {
        missing_inputs: vec![message.into()],
        ..ToolApplyPreview::default()
    }
}

fn fsr4_variant(config: &ToolConfig) -> &str {
    match config.get_str("fsr4_variant") {
        Some(FSR4_VARIANT_INT8_402) => FSR4_VARIANT_INT8_402,
        _ => FSR4_VARIANT_LATEST_FP8,
    }
}

fn selected_fsr4_variant_source(source_dir: &Path, config: &ToolConfig) -> Option<PathBuf> {
    let dir = match fsr4_variant(config) {
        FSR4_VARIANT_INT8_402 => FSR4_INT8_DIR,
        FSR4_VARIANT_LATEST_FP8 => FSR4_LATEST_DIR,
        _ => return None,
    };
    let selected = source_dir.join(dir).join(FSR4_DLL_NAME);
    let has_variant_payloads =
        source_dir.join(FSR4_LATEST_DIR).is_dir() || source_dir.join(FSR4_INT8_DIR).is_dir();
    let expects_goverlay_payloads = matches!(
        config.get_str("source_mode"),
        Some(OPTISCALER_SOURCE_GOVERLAY_BUILDS | OPTISCALER_SOURCE_GOVERLAY_FGMOD)
    );
    (selected.is_file() || has_variant_payloads || expects_goverlay_payloads).then_some(selected)
}

fn optipatcher_asi_source(source_dir: &Path) -> PathBuf {
    [
        source_dir.join("plugins").join(OPTIPATCHER_ASSET),
        source_dir.join(OPTIPATCHER_ASSET),
        cached_optipatcher_asi(),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .unwrap_or_else(cached_optipatcher_asi)
}

fn preview_source_file(
    game_dir: &Path,
    src: &Path,
    dest: &Path,
    preview: &mut ToolApplyPreview,
) -> Result<()> {
    let expected = std::fs::read(src)?;
    preview_bytes(game_dir, dest, &expected, preview);
    Ok(())
}

fn preview_bytes(game_dir: &Path, dest: &Path, expected: &[u8], preview: &mut ToolApplyPreview) {
    let changed = std::fs::read(dest).map_or(true, |current| current != expected);
    let rel = dest.strip_prefix(game_dir).unwrap_or(dest).to_path_buf();
    preview.record_file(rel, changed);
}

fn preview_dir_recursive(
    game_dir: &Path,
    src: &Path,
    dest: &Path,
    preview: &mut ToolApplyPreview,
) -> Result<()> {
    for entry in std::fs::read_dir(src)?.flatten() {
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if ty.is_dir() {
            preview_dir_recursive(game_dir, &src_path, &dest_path, preview)?;
        } else {
            preview_source_file(game_dir, &src_path, &dest_path, preview)?;
        }
    }
    Ok(())
}

fn goverlay_optiscaler_ini_specs() -> Vec<super::ToolSettingSpec> {
    vec![
        super::ToolSettingSpec::labeled_select(
            "ini_overrides.Menu.ShortcutKey",
            "Menu shortcut",
            "OptiScaler [Menu] ShortcutKey override.",
            &[
                ("auto", "Auto"),
                ("INSERT", "Insert"),
                ("HOME", "Home"),
                ("END", "End"),
                ("DELETE", "Delete"),
                ("BACKQUOTE", "Backquote"),
                ("F1", "F1"),
                ("F2", "F2"),
                ("F3", "F3"),
                ("F4", "F4"),
                ("F5", "F5"),
                ("F6", "F6"),
                ("F7", "F7"),
                ("F8", "F8"),
                ("F9", "F9"),
                ("F10", "F10"),
                ("F11", "F11"),
                ("F12", "F12"),
            ],
        )
        .section("Menu"),
        super::ToolSettingSpec::number(
            "ini_overrides.Menu.Scale",
            "Menu scale",
            "OptiScaler [Menu] Scale override.",
            0.5,
            2.0,
            0.1,
        )
        .section("Menu"),
        super::ToolSettingSpec::tri_state_bool(
            "ini_overrides.NvApi.OverrideNvapiDll",
            "Override NVAPI DLL",
            "OptiScaler OverrideNvapiDll override.",
        )
        .section("Fakenvapi"),
        super::ToolSettingSpec::labeled_select(
            "ini_overrides.FSR.UpscalerIndex",
            "FSR upscaler backend",
            "OptiScaler [FSR] UpscalerIndex override.",
            &[
                ("auto", "Auto"),
                ("0", "0 - FSR 4.0.2"),
                ("1", "1 - FSR 3.1.5"),
                ("2", "2 - FSR 2.3.4"),
            ],
        )
        .section("Basic"),
        super::ToolSettingSpec::labeled_select(
            "ini_overrides.FSR.FGIndex",
            "FSR frame generation backend",
            "OptiScaler [FSR] FGIndex override.",
            &[
                ("auto", "Auto"),
                ("0", "0 - FSR 4.0.0"),
                ("1", "1 - FSR 3.1.6"),
            ],
        )
        .section("Basic"),
        super::ToolSettingSpec::labeled_select(
            "ini_overrides.fakenvapi.force_reflex",
            "Force Reflex",
            "fakenvapi force_reflex override.",
            &[
                ("0", "0 - Follow in-game setting"),
                ("1", "1 - Force disable"),
                ("2", "2 - Force enable"),
            ],
        )
        .section("Fakenvapi"),
        super::ToolSettingSpec::tri_state_bool(
            "ini_overrides.fakenvapi.force_latencyflex",
            "Force LatencyFlex",
            "fakenvapi force_latencyflex override.",
        )
        .section("Fakenvapi"),
        super::ToolSettingSpec::labeled_select(
            "ini_overrides.fakenvapi.latencyflex_mode",
            "LatencyFlex mode",
            "fakenvapi latencyflex_mode override.",
            &[
                ("0", "0 - Conservative"),
                ("1", "1 - Aggressive"),
                ("2", "2 - Use Reflex frame IDs"),
            ],
        )
        .section("Fakenvapi"),
        super::ToolSettingSpec::tri_state_bool(
            "ini_overrides.fakenvapi.enable_trace_logs",
            "Trace logs",
            "fakenvapi enable_trace_logs override.",
        )
        .section("Fakenvapi"),
    ]
}

fn optiscaler_ini_specs(config: &ToolConfig) -> Vec<super::ToolSettingSpec> {
    let Some(source_dir) = resolve_source_dir(config) else {
        return Vec::new();
    };
    let ini = source_dir.join("OptiScaler.ini");
    let Ok(content) = std::fs::read_to_string(ini) else {
        return Vec::new();
    };
    parse_ini_keys(&content)
        .into_iter()
        .filter(|key| {
            !matches!(
                key.as_str(),
                "FSR.Fsr4Update" | "Spoofing.Dxgi" | "Plugins.LoadAsiPlugins"
            )
        })
        .take(24)
        .map(|key| {
            let leaked: &'static str = Box::leak(format!("ini_overrides.{key}").into_boxed_str());
            let label: &'static str = Box::leak(key.into_boxed_str());
            infer_optiscaler_ini_spec(leaked, label)
                .section("Advanced")
                .advanced()
        })
        .collect()
}

fn infer_optiscaler_ini_spec(key: &'static str, label: &'static str) -> super::ToolSettingSpec {
    let lower = label.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "fsr4update"
            | "dxgi"
            | "loadasiplugins"
            | "overridenvapidll"
            | "force_latencyflex"
            | "enable_trace_logs"
    ) {
        return super::ToolSettingSpec::tri_state_bool(key, label, "OptiScaler.ini override.");
    }
    if lower.ends_with("scale")
        || lower.ends_with("alpha")
        || lower.contains("sharpness")
        || lower.contains("bias")
    {
        return super::ToolSettingSpec::number(
            key,
            label,
            "OptiScaler.ini override.",
            0.0,
            10.0,
            0.05,
        );
    }
    super::ToolSettingSpec::text(key, label, "OptiScaler.ini override.")
}

pub fn extract_optiscaler_archive_flat(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    let lower = archive_path.to_string_lossy().to_ascii_lowercase();
    if lower.ends_with(".zip") {
        return extract_zip_flat(archive_path, dest_dir);
    }
    if lower.ends_with(".7z") {
        return extract_7z_flat(archive_path, dest_dir);
    }
    anyhow::bail!(
        "unsupported OptiScaler archive type: {}",
        archive_path.display()
    )
}

fn extract_zip_flat(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    let file = std::fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut copied = 0usize;
    let mut copied_optiscaler = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let Some(enclosed) = entry.enclosed_name() else {
            continue;
        };
        let Some(name) = enclosed.file_name().map(ToOwned::to_owned) else {
            continue;
        };
        let Some(name_str) = name.to_str() else {
            continue;
        };
        let lower = name_str.to_ascii_lowercase();
        if !is_optiscaler_payload_file(&lower) {
            continue;
        }
        let out = optiscaler_payload_dest(dest_dir, &enclosed, &name);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut output = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut output)?;
        copied += 1;
        copied_optiscaler |= lower == "optiscaler.dll";
    }
    if copied == 0 || !copied_optiscaler {
        anyhow::bail!("archive did not contain OptiScaler.dll");
    }
    Ok(())
}

fn extract_7z_flat(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    let tmp_base = modde_core::paths::modde_data_dir().join("tmp");
    std::fs::create_dir_all(&tmp_base)?;
    let extract_dir = tmp_base.join(format!(
        "modde-optiscaler-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir_all(&extract_dir)?;
    let out_arg = format!("-o{}", extract_dir.display());
    let archive_arg = archive_path.to_string_lossy().to_string();
    let mut extracted = false;
    for bin in ["7zz", "7z"] {
        let status = Command::new(bin)
            .args(["x", "-y", &out_arg, &archive_arg])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if status.is_ok_and(|status| status.success()) {
            extracted = true;
            break;
        }
    }
    if !extracted {
        anyhow::bail!(
            "failed to extract {} (tried 7zz and 7z)",
            archive_path.display()
        );
    }
    let result = copy_optiscaler_payload_flat(&extract_dir, dest_dir);
    let _ = std::fs::remove_dir_all(&extract_dir);
    result
}

fn copy_optiscaler_payload_flat(source_root: &Path, dest_dir: &Path) -> Result<()> {
    let mut stack = vec![source_root.to_path_buf()];
    let mut copied = 0usize;
    let mut copied_optiscaler = false;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = path.symlink_metadata()?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            if is_optiscaler_payload_file(&lower) {
                let relative = path.strip_prefix(source_root).unwrap_or(&path);
                let dest = optiscaler_payload_dest(dest_dir, relative, std::ffi::OsStr::new(name));
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&path, dest)?;
                copied += 1;
                copied_optiscaler |= lower == "optiscaler.dll";
            }
        }
    }
    if copied == 0 || !copied_optiscaler {
        anyhow::bail!("archive did not contain OptiScaler.dll");
    }
    Ok(())
}

fn is_optiscaler_payload_file(lower_name: &str) -> bool {
    matches!(
        lower_name,
        "optiscaler.dll"
            | "optiscaler.ini"
            | "fakenvapi.dll"
            | "nvngx-wrapper.dll"
            | "optipatcher.asi"
    ) || lower_name.ends_with(".dll")
}

fn optiscaler_payload_dest(
    dest_dir: &Path,
    relative_path: &Path,
    file_name: &std::ffi::OsStr,
) -> PathBuf {
    if file_name
        .to_str()
        .is_some_and(|name| name.eq_ignore_ascii_case(FSR4_DLL_NAME))
    {
        for component in relative_path.components() {
            let value = component.as_os_str().to_string_lossy();
            if value.eq_ignore_ascii_case(FSR4_LATEST_DIR) {
                return dest_dir.join(FSR4_LATEST_DIR).join(FSR4_DLL_NAME);
            }
            if value.eq_ignore_ascii_case(FSR4_INT8_DIR) {
                return dest_dir.join(FSR4_INT8_DIR).join(FSR4_DLL_NAME);
            }
        }
    }
    if file_name
        .to_str()
        .is_some_and(|name| name.eq_ignore_ascii_case(OPTIPATCHER_ASSET))
    {
        return dest_dir.join("plugins").join(OPTIPATCHER_ASSET);
    }
    dest_dir.join(file_name)
}

#[must_use]
pub fn parse_optiscaler_ini(content: &str) -> BTreeMap<String, String> {
    let mut section = String::new();
    let mut values = BTreeMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed[1..trimmed.len() - 1].trim().to_string();
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            if !key.is_empty() {
                let path = if section.is_empty() {
                    key.to_string()
                } else {
                    format!("{section}.{key}")
                };
                values.insert(path, value.trim().to_string());
            }
        }
    }
    values
}

fn parse_ini_keys(content: &str) -> Vec<String> {
    let mut section = String::new();
    let mut keys = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed[1..trimmed.len() - 1].trim().to_string();
            continue;
        }
        if let Some((key, _)) = trimmed.split_once('=') {
            let key = key.trim();
            if !key.is_empty() {
                if section.is_empty() {
                    keys.push(key.to_string());
                } else {
                    keys.push(format!("{section}.{key}"));
                }
            }
        }
    }
    keys
}

fn apply_ini_overrides_with_existing(
    src: &Path,
    existing: Option<&Path>,
    dest: &Path,
    config: &ToolConfig,
) -> Result<()> {
    let content = build_ini_with_overrides(src, existing, config)?;
    std::fs::write(dest, content)?;
    Ok(())
}

fn build_ini_with_overrides(
    src: &Path,
    existing: Option<&Path>,
    config: &ToolConfig,
) -> Result<String> {
    let mut content = std::fs::read_to_string(src)?;
    if let Some(existing) = existing
        && let Ok(existing_content) = std::fs::read_to_string(existing)
    {
        let source_keys: BTreeSet<String> = parse_ini_keys(&content).into_iter().collect();
        for (key, value) in parse_optiscaler_ini(&existing_content) {
            if source_keys.contains(&key) {
                content = set_ini_value(&content, &key, &value);
            }
        }
    }
    if let Some(overrides) = config
        .settings
        .get("ini_overrides")
        .and_then(serde_json::Value::as_object)
    {
        for (path, value) in flatten_ini_overrides(overrides) {
            let value = match value {
                serde_json::Value::String(value) => value,
                other => other.to_string(),
            };
            content = set_ini_value(&content, path.as_str(), value.as_str());
        }
    }
    for (path, value) in effective_optiscaler_ini_overrides(config) {
        content = set_ini_value(&content, path, value);
    }
    Ok(content)
}

fn effective_optiscaler_ini_overrides(config: &ToolConfig) -> Vec<(&'static str, &'static str)> {
    let mut overrides = Vec::new();
    overrides.push((
        "FSR.Fsr4Update",
        match fsr4_variant(config) {
            FSR4_VARIANT_INT8_402 => "auto",
            _ => "True",
        },
    ));
    overrides.push((
        "Spoofing.Dxgi",
        if config.get_bool("spoof_dlss") {
            "auto"
        } else {
            "false"
        },
    ));
    overrides.push((
        "Plugins.LoadAsiPlugins",
        if config.get_bool("enable_optipatcher") {
            "true"
        } else {
            "auto"
        },
    ));
    overrides
}

#[must_use]
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

fn selected_or_default_profile(
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

fn apply_custom_profile(config: &mut ToolConfig) {
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

fn apply_optiscaler_profile_metadata(config: &mut ToolConfig, profile: &OptiScalerProfile) {
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

fn apply_optiscaler_release_selection(config: &mut ToolConfig, normalized_tag: &str, asset: &str) {
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

pub fn scan_optiscaler_install(
    game_id: &str,
    game_dir: &Path,
    managed_paths: &BTreeSet<String>,
) -> Result<OptiScalerInstallState> {
    let executable_dir = crate::resolve_game_plugin(game_id)
        .map(|plugin| plugin.executable_dir(game_dir))
        .unwrap_or_else(|| game_dir.to_path_buf());
    let executable_managed_paths =
        managed_paths_for_executable_dir(game_dir, &executable_dir, managed_paths);
    let mut state = scan_optiscaler_install_in_dir(&executable_dir, &executable_managed_paths)?;
    state.latest_backup = latest_optiscaler_backup(Some(game_id));
    Ok(state)
}

fn managed_paths_for_executable_dir(
    game_dir: &Path,
    executable_dir: &Path,
    managed_paths: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut out = managed_paths.clone();
    let Ok(executable_rel) = executable_dir.strip_prefix(game_dir) else {
        return out;
    };
    let executable_prefix = normalize_rel_path(executable_rel.to_string_lossy());
    let executable_prefix = executable_prefix.trim_end_matches('/');
    if executable_prefix.is_empty() {
        return out;
    }
    let prefix = format!("{executable_prefix}/");
    for path in managed_paths {
        if let Some(stripped) = path.strip_prefix(&prefix) {
            out.insert(stripped.to_string());
        }
    }
    out
}

pub fn scan_optiscaler_install_in_dir(
    executable_dir: &Path,
    managed_paths: &BTreeSet<String>,
) -> Result<OptiScalerInstallState> {
    let mut recognized_files = Vec::new();
    let mut proxy_dlls = Vec::new();
    let mut companion_files = Vec::new();
    let config_path = executable_dir
        .join("OptiScaler.ini")
        .exists()
        .then(|| executable_dir.join("OptiScaler.ini"));

    for &name in OPTISCALER_PROXY_DLLS {
        let path = executable_dir.join(name);
        if path.is_file() && is_likely_optiscaler_proxy(&path, name) {
            proxy_dlls.push(name.to_string());
            push_detected_file(executable_dir, &path, managed_paths, &mut recognized_files);
        }
    }
    if let Some(path) = &config_path {
        push_detected_file(executable_dir, path, managed_paths, &mut recognized_files);
    }
    for entry in std::fs::read_dir(executable_dir)
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        let is_companion = OPTISCALER_COMPANION_FILES
            .iter()
            .any(|known| known.eq_ignore_ascii_case(name))
            || lower.starts_with("libxess")
            || lower.starts_with("amd_fidelityfx");
        if path.is_file() && is_companion {
            companion_files.push(path.clone());
            push_detected_file(executable_dir, &path, managed_paths, &mut recognized_files);
        }
        if path.is_dir()
            && OPTISCALER_COMPANION_DIRS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(name))
        {
            companion_files.push(path.clone());
            collect_detected_dir(executable_dir, &path, managed_paths, &mut recognized_files)?;
        }
    }

    recognized_files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    recognized_files.dedup_by(|a, b| a.rel_path == b.rel_path);
    proxy_dlls.sort();
    proxy_dlls.dedup();
    companion_files.sort();
    companion_files.dedup();

    let managed_count = recognized_files.iter().filter(|file| file.managed).count();
    let status = if proxy_dlls.len() > 1 {
        OptiScalerInstallStatus::Conflicted
    } else if recognized_files.is_empty() {
        OptiScalerInstallStatus::Absent
    } else if managed_count == recognized_files.len() {
        OptiScalerInstallStatus::Managed
    } else if managed_count == 0 {
        OptiScalerInstallStatus::Unmanaged
    } else {
        OptiScalerInstallStatus::PartiallyManaged
    };

    let ini_settings = config_path
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|content| parse_optiscaler_ini(&content))
        .unwrap_or_default();
    let version = identify_optiscaler_version(executable_dir, &recognized_files);
    let wine_dll_overrides = proxy_dlls
        .iter()
        .filter_map(|name| name.strip_suffix(".dll").map(ToOwned::to_owned))
        .collect();
    let latest_backup = latest_optiscaler_backup(None);

    Ok(OptiScalerInstallState {
        status,
        executable_dir: executable_dir.to_path_buf(),
        proxy_dlls,
        wine_dll_overrides,
        config_path,
        ini_settings,
        companion_files,
        recognized_files,
        version,
        latest_backup,
    })
}

pub fn backup_optiscaler_install(
    game_id: Option<&str>,
    state: &OptiScalerInstallState,
) -> Result<Option<PathBuf>> {
    if state.recognized_files.is_empty() {
        return Ok(None);
    }
    let backup_dir = optiscaler_backup_root(game_id).join(timestamp_slug());
    for file in &state.recognized_files {
        let src = state.executable_dir.join(&file.rel_path);
        let dst = backup_dir.join(&file.rel_path);
        if src.is_file() {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src, &dst)?;
        }
    }
    let manifest = serde_json::json!({
        "status": state.status.to_string(),
        "version": state.version.to_string(),
        "executable_dir": state.executable_dir.display().to_string(),
        "files": state.recognized_files.iter().map(|file| {
            serde_json::json!({
                "path": file.rel_path.to_string_lossy().replace('\\', "/"),
                "hash": file.hash,
                "managed": file.managed,
            })
        }).collect::<Vec<_>>(),
    });
    std::fs::create_dir_all(&backup_dir)?;
    std::fs::write(
        backup_dir.join("modde-optiscaler-backup.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    Ok(Some(backup_dir))
}

pub fn latest_optiscaler_backup(game_id: Option<&str>) -> Option<PathBuf> {
    let root = optiscaler_backup_root(game_id);
    let mut entries = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            entry.file_type().ok()?.is_dir().then_some(path)
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries.pop()
}

pub fn restore_latest_optiscaler_backup(game_id: &str, game_dir: &Path) -> Result<PathBuf> {
    let backup = latest_optiscaler_backup(Some(game_id))
        .ok_or_else(|| anyhow::anyhow!("no OptiScaler backup found for {game_id}"))?;
    let executable_dir = crate::resolve_game_plugin(game_id)
        .map(|plugin| plugin.executable_dir(game_dir))
        .unwrap_or_else(|| game_dir.to_path_buf());
    restore_dir_contents(&backup, &executable_dir)?;
    Ok(backup)
}

fn optiscaler_backup_root(game_id: Option<&str>) -> PathBuf {
    modde_core::paths::modde_data_dir()
        .join("tool-backups")
        .join("optiscaler")
        .join(game_id.unwrap_or("_unknown"))
}

fn timestamp_slug() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    format!("{secs}-{}", std::process::id())
}

fn is_likely_optiscaler_proxy(path: &Path, name: &str) -> bool {
    if name.eq_ignore_ascii_case("OptiScaler.asi") {
        return true;
    }
    let Ok(hash) = file_hash_hex(path) else {
        return true;
    };
    cached_release_dirs().into_iter().any(|dir| {
        file_hash_hex(&dir.join("OptiScaler.dll")).ok().as_deref() == Some(hash.as_str())
    }) || path.metadata().is_ok_and(|metadata| metadata.len() > 0)
}

fn identify_optiscaler_version(
    executable_dir: &Path,
    recognized_files: &[OptiScalerDetectedFile],
) -> OptiScalerVersionIdentity {
    let proxy_hashes = recognized_files
        .iter()
        .filter(|file| {
            file.rel_path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|file_name| {
                    OPTISCALER_PROXY_DLLS
                        .iter()
                        .any(|name| file_name.eq_ignore_ascii_case(name))
                })
        })
        .filter_map(|file| file.hash.as_deref())
        .collect::<Vec<_>>();
    for dir in cached_release_dirs() {
        let Ok(hash) = file_hash_hex(&dir.join("OptiScaler.dll")) else {
            continue;
        };
        if proxy_hashes.iter().any(|candidate| *candidate == hash)
            && let Some(tag) = dir.file_name().and_then(|name| name.to_str())
        {
            return OptiScalerVersionIdentity::CachedRelease(tag.to_string());
        }
    }
    let version_file = executable_dir.join("version.txt");
    if let Ok(version) = std::fs::read_to_string(version_file) {
        let trimmed = version.trim();
        if !trimmed.is_empty() {
            return OptiScalerVersionIdentity::FileMetadata(trimmed.to_string());
        }
    }
    if let Some(hash) = proxy_hashes.first() {
        return OptiScalerVersionIdentity::ContentHash((*hash).to_string());
    }
    OptiScalerVersionIdentity::Unknown
}

fn cached_release_dirs() -> Vec<PathBuf> {
    let root = modde_core::paths::modde_data_dir()
        .join("tools")
        .join("optiscaler");
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_type().ok()?.is_dir().then_some(entry.path()))
        .collect()
}

fn push_detected_file(
    root: &Path,
    path: &Path,
    managed_paths: &BTreeSet<String>,
    out: &mut Vec<OptiScalerDetectedFile>,
) {
    let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
    let normalized = normalize_rel_path(rel.to_string_lossy());
    out.push(OptiScalerDetectedFile {
        rel_path: rel,
        hash: file_hash_hex(path).ok(),
        managed: managed_paths.contains(&normalized),
    });
}

fn collect_detected_dir(
    root: &Path,
    dir: &Path,
    managed_paths: &BTreeSet<String>,
    out: &mut Vec<OptiScalerDetectedFile>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = path.symlink_metadata()?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_detected_dir(root, &path, managed_paths, out)?;
        } else if metadata.is_file() {
            push_detected_file(root, &path, managed_paths, out);
        }
    }
    Ok(())
}

fn collect_relative_files(game_dir: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_relative_files(game_dir, &path, out)?;
        } else if path.is_file() {
            out.push(relative_to_game(game_dir, &path)?);
        }
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if src_path.is_file() {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn restore_dir_contents(src: &Path, dst: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        if entry.file_name() == "modde-optiscaler-backup.json" {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            restore_dir_contents(&src_path, &dst_path)?;
        } else if src_path.is_file() {
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn normalize_rel_path(path: impl AsRef<str>) -> String {
    path.as_ref().replace('\\', "/").to_ascii_lowercase()
}

fn file_hash_hex(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Xxh64::new(0);
    let mut buf = [0_u8; 8192];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(format!("{:016x}", hasher.digest()))
}

fn optiscaler_config_reset_reason(
    existing: &OptiScalerInstallState,
    new_ini: &Path,
    config: &ToolConfig,
) -> Option<String> {
    if config.get_bool("force_config_reset") {
        return Some("forced by setting".to_string());
    }
    let Ok(new_content) = std::fs::read_to_string(new_ini) else {
        return None;
    };
    let new_keys: BTreeSet<String> = parse_ini_keys(&new_content).into_iter().collect();
    let old_unknown = existing
        .ini_settings
        .keys()
        .any(|key| !new_keys.contains(key));
    old_unknown.then_some("schema mismatch".to_string())
}

fn flatten_ini_overrides(
    overrides: &serde_json::Map<String, serde_json::Value>,
) -> Vec<(String, serde_json::Value)> {
    fn walk(prefix: &str, value: &serde_json::Value, out: &mut Vec<(String, serde_json::Value)>) {
        if let serde_json::Value::Object(map) = value {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                walk(&path, child, out);
            }
        } else {
            out.push((prefix.to_string(), value.clone()));
        }
    }

    let mut out = Vec::new();
    for (key, value) in overrides {
        walk(key, value, &mut out);
    }
    out
}

fn set_ini_value(content: &str, path: &str, value: &str) -> String {
    let Some((target_section, target_key)) = path.rsplit_once('.') else {
        tracing::warn!(
            ini_key = path,
            "optiscaler: ini override key has no section prefix; expected 'section.key'"
        );
        return content.to_string();
    };
    let mut current_section = "";
    let mut updated = false;
    let mut lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if !updated && !target_section.is_empty() && current_section == target_section {
                lines.push(format!("{target_key}={value}"));
                updated = true;
            }
            current_section = trimmed[1..trimmed.len() - 1].trim();
        }

        if current_section == target_section
            && let Some((key, _)) = trimmed.split_once('=')
            && key.trim() == target_key
        {
            lines.push(format!("{target_key}={value}"));
            updated = true;
            continue;
        }
        lines.push(line.to_string());
    }

    if !updated && !target_section.is_empty() && current_section == target_section {
        lines.push(format!("{target_key}={value}"));
        updated = true;
    }

    if !updated {
        if !target_section.is_empty() {
            lines.push(format!("[{target_section}]"));
        }
        lines.push(format!("{target_key}={value}"));
    }

    lines.join("\n") + "\n"
}

/// Build fgmod DLL restore commands for the launch wrapper.
///
/// Scans the staging mods directory for DLLs that fgmod will delete at launch,
/// and returns `(source, destination)` pairs for the wrapper to restore them.
#[must_use]
pub fn fgmod_restore_commands(game_dir: &Path, staging_dir: &Path) -> Vec<(String, String)> {
    let exe_dir = crate::resolve_game_plugin("cyberpunk2077")
        .map(|plugin| plugin.executable_dir(game_dir))
        .unwrap_or_else(|| game_dir.join("bin/x64"));
    fgmod_restore_commands_for_executable_dir(game_dir, staging_dir, &exe_dir)
}

/// Build fgmod DLL restore commands for a known executable directory.
#[must_use]
pub fn fgmod_restore_commands_for_executable_dir(
    _game_dir: &Path,
    staging_dir: &Path,
    exe_dir: &Path,
) -> Vec<(String, String)> {
    let mut restore = Vec::new();

    let mods_dir = staging_dir.join("mods");
    if !mods_dir.exists() {
        return restore;
    }

    for entry in std::fs::read_dir(&mods_dir).into_iter().flatten().flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }

        let mod_bin_x64 = entry.path().join("bin/x64");
        if !mod_bin_x64.exists() {
            continue;
        }

        for dll_entry in std::fs::read_dir(&mod_bin_x64)
            .into_iter()
            .flatten()
            .flatten()
        {
            let dll_name = dll_entry.file_name().to_string_lossy().to_lowercase();
            if FGMOD_DELETED_DLLS
                .iter()
                .any(|d| d.to_lowercase() == dll_name)
            {
                let src = dll_entry.path();
                let dest = exe_dir.join(&*dll_name);
                restore.push((
                    src.to_string_lossy().to_string(),
                    dest.to_string_lossy().to_string(),
                ));
            }
        }
    }

    restore
}

/// Shim for `dirs::data_dir()` / `dirs::home_dir()` — we use a minimal
/// vendored version to avoid adding the full `dirs` crate.
mod dirs {
    use std::path::PathBuf;

    pub fn data_dir() -> Option<PathBuf> {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|h| h.join(".local/share")))
    }

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    use crate::tools::ToolSettingKind;

    use super::*;

    #[test]
    fn parse_optiscaler_ini_preserves_section_paths() {
        let parsed = parse_optiscaler_ini(
            r"
            ; comment
            [OptiScaler]
            Dxgi=auto
            LoadAsiPlugins=true
            [Menu]
            Scale=1.25
            ",
        );
        assert_eq!(parsed.get("OptiScaler.Dxgi"), Some(&"auto".to_string()));
        assert_eq!(
            parsed.get("OptiScaler.LoadAsiPlugins"),
            Some(&"true".to_string())
        );
        assert_eq!(parsed.get("Menu.Scale"), Some(&"1.25".to_string()));
    }

    #[test]
    fn preview_reports_changed_when_proxy_or_ini_differ() {
        let source = tempfile::tempdir().expect("source");
        let game = tempfile::tempdir().expect("game");
        std::fs::write(source.path().join("OptiScaler.dll"), b"new dll").expect("source dll");
        std::fs::write(
            source.path().join("OptiScaler.ini"),
            "[FSR]\nFGIndex=auto\n",
        )
        .expect("source ini");
        std::fs::write(game.path().join("dxgi.dll"), b"old dll").expect("dest dll");
        std::fs::write(game.path().join("OptiScaler.ini"), "[FSR]\nFGIndex=1\n").expect("dest ini");
        let mut config = OptiScaler.default_config();
        config.set("source_mode", serde_json::json!("local_dir"));
        config.set(
            "local_source_dir",
            serde_json::json!(source.path().display().to_string()),
        );
        config.set(
            "ini_overrides",
            serde_json::json!({ "FSR": { "FGIndex": "2" } }),
        );

        let preview = OptiScaler
            .preview_apply_for(game.path(), None, &config)
            .expect("preview");

        assert!(preview.changed_files.contains(&PathBuf::from("dxgi.dll")));
        assert!(
            preview
                .changed_files
                .contains(&PathBuf::from("OptiScaler.ini"))
        );
        assert!(preview.missing_inputs.is_empty());
    }

    #[test]
    fn preview_reports_unchanged_and_does_not_create_target_dirs() {
        let source = tempfile::tempdir().expect("source");
        let game = tempfile::tempdir().expect("game");
        std::fs::write(source.path().join("OptiScaler.dll"), b"same dll").expect("source dll");
        std::fs::write(
            source.path().join("OptiScaler.ini"),
            "[FSR]\nFGIndex=auto\n",
        )
        .expect("source ini");
        let target = game.path().join("Bin");
        std::fs::create_dir(&target).expect("target");
        std::fs::write(target.join("dxgi.dll"), b"same dll").expect("dest dll");
        std::fs::write(
            target.join("OptiScaler.ini"),
            "[FSR]\nFGIndex=auto\nFsr4Update=True\n[Spoofing]\nDxgi=false\n[Plugins]\nLoadAsiPlugins=auto\n",
        )
        .expect("dest ini");
        let mut config = OptiScaler.default_config();
        config.set("source_mode", serde_json::json!("local_dir"));
        config.set(
            "local_source_dir",
            serde_json::json!(source.path().display().to_string()),
        );
        config.set("exe_subdir", serde_json::json!("Bin"));

        let preview = OptiScaler
            .preview_apply_for(game.path(), None, &config)
            .expect("preview");

        assert!(preview.changed_files.is_empty());
        assert!(
            preview
                .unchanged_files
                .contains(&PathBuf::from("Bin/dxgi.dll"))
        );
        assert!(
            preview
                .unchanged_files
                .contains(&PathBuf::from("Bin/OptiScaler.ini"))
        );
        assert!(!game.path().join("Other").exists());
    }

    #[test]
    fn preview_does_not_create_missing_target_directory() {
        let source = tempfile::tempdir().expect("source");
        let game = tempfile::tempdir().expect("game");
        std::fs::write(source.path().join("OptiScaler.dll"), b"dll").expect("source dll");
        std::fs::write(source.path().join("OptiScaler.ini"), "[FSR]\n").expect("source ini");
        let mut config = OptiScaler.default_config();
        config.set("source_mode", serde_json::json!("local_dir"));
        config.set(
            "local_source_dir",
            serde_json::json!(source.path().display().to_string()),
        );
        config.set("exe_subdir", serde_json::json!("MissingBin"));

        let preview = OptiScaler
            .preview_apply_for(game.path(), None, &config)
            .expect("preview");

        assert!(preview.has_changes());
        assert!(!game.path().join("MissingBin").exists());
    }

    #[test]
    fn goverlay_inspired_settings_expose_friendly_raw_value_selects() {
        let specs = OptiScaler.settings_schema();
        let shortcut = specs
            .iter()
            .find(|spec| spec.key == "ini_overrides.Menu.ShortcutKey")
            .expect("shortcut spec");
        assert_eq!(shortcut.section, "Menu");
        assert!(!shortcut.advanced);
        let ToolSettingKind::Select { options } = &shortcut.kind else {
            panic!("shortcut should be a select");
        };
        assert!(
            options
                .iter()
                .any(|option| option.value == "INSERT" && option.label == "Insert")
        );

        let upscaler = specs
            .iter()
            .find(|spec| spec.key == "ini_overrides.FSR.UpscalerIndex")
            .expect("FSR upscaler spec");
        assert_eq!(upscaler.section, "Basic");
        let ToolSettingKind::Select { options } = &upscaler.kind else {
            panic!("FSR upscaler should be a select");
        };
        assert_eq!(options[0].value, "auto");
        assert_eq!(options[0].label, "Auto");
        assert!(
            options
                .iter()
                .any(|option| option.value == "0" && option.label == "0 - FSR 4.0.2")
        );
        assert!(
            options
                .iter()
                .any(|option| option.value == "2" && option.label == "2 - FSR 2.3.4")
        );

        let fg = specs
            .iter()
            .find(|spec| spec.key == "ini_overrides.FSR.FGIndex")
            .expect("FSR FG spec");
        let ToolSettingKind::Select { options } = &fg.kind else {
            panic!("FSR FG should be a select");
        };
        assert!(
            options
                .iter()
                .any(|option| option.value == "1" && option.label == "1 - FSR 3.1.6")
        );
    }

    #[test]
    fn goverlay_inspired_settings_mark_only_risky_entries_advanced() {
        let specs = OptiScaler.settings_schema();
        let spoof = specs
            .iter()
            .find(|spec| spec.key == "spoof_dlss")
            .expect("spoof fallback spec");
        assert_eq!(spoof.label, "Spoof DLSS fallback");
        assert_eq!(spoof.section, "Basic");
        assert!(!spoof.advanced);

        let optipatcher = specs
            .iter()
            .find(|spec| spec.key == "enable_optipatcher")
            .expect("OptiPatcher spec");
        assert_eq!(optipatcher.section, "Basic");
        assert!(!optipatcher.advanced);

        assert!(
            !specs
                .iter()
                .any(|spec| spec.key == "ini_overrides.Plugins.LoadAsiPlugins")
        );
    }

    #[test]
    fn emulate_fp8_schema_is_read_only_for_int8_variant() {
        let mut config = OptiScaler.default_config();
        config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));

        let specs = OptiScaler.settings_schema_for(None, &config);
        let emulate = specs
            .iter()
            .find(|spec| spec.key == "emulate_fp8")
            .expect("emulate fp8 spec");

        assert_eq!(emulate.section, "Basic");
        assert!(matches!(emulate.kind, ToolSettingKind::ReadOnly));
    }

    #[test]
    fn goverlay_channel_is_only_exposed_for_goverlay_build_source() {
        let mut config = OptiScaler.default_config();
        config.set("source_mode", serde_json::json!("github_release"));
        let specs = OptiScaler.settings_schema_for(None, &config);
        assert!(!specs.iter().any(|spec| spec.key == "goverlay_channel"));

        config.set("source_mode", serde_json::json!("goverlay_builds"));
        let specs = OptiScaler.settings_schema_for(None, &config);
        assert!(specs.iter().any(|spec| spec.key == "goverlay_channel"));
    }

    #[test]
    fn fsr_backend_overrides_write_raw_ini_values() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("OptiScaler.ini");
        let dest = tmp.path().join("applied.ini");
        std::fs::write(
            &src,
            "[FSR]\nUpscalerIndex=auto\nFGIndex=auto\n[Menu]\nShortcutKey=auto\n",
        )
        .expect("source ini");
        let mut config = OptiScaler.default_config();
        config.set(
            "ini_overrides",
            serde_json::json!({
                "FSR": {
                    "UpscalerIndex": "0",
                    "FGIndex": "1"
                },
                "Menu": {
                    "ShortcutKey": "INSERT"
                }
            }),
        );

        apply_ini_overrides_with_existing(&src, None, &dest, &config).expect("apply overrides");

        let parsed = parse_optiscaler_ini(&std::fs::read_to_string(dest).expect("dest ini"));
        assert_eq!(parsed.get("FSR.UpscalerIndex"), Some(&"0".to_string()));
        assert_eq!(parsed.get("FSR.FGIndex"), Some(&"1".to_string()));
        assert_eq!(parsed.get("Menu.ShortcutKey"), Some(&"INSERT".to_string()));
    }

    #[test]
    fn optiscaler_high_level_controls_write_ini_values() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("OptiScaler.ini");
        let dest = tmp.path().join("applied.ini");
        std::fs::write(
            &src,
            "[FSR]\nFsr4Update=auto\n[Spoofing]\nDxgi=auto\n[Plugins]\nLoadAsiPlugins=auto\n",
        )
        .expect("source ini");
        let mut config = OptiScaler.default_config();
        config.set("enable_optipatcher", serde_json::json!(true));
        config.set("spoof_dlss", serde_json::json!(false));
        config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_LATEST_FP8));

        apply_ini_overrides_with_existing(&src, None, &dest, &config).expect("apply overrides");

        let parsed = parse_optiscaler_ini(&std::fs::read_to_string(dest).expect("dest ini"));
        assert_eq!(parsed.get("FSR.Fsr4Update"), Some(&"True".to_string()));
        assert_eq!(parsed.get("Spoofing.Dxgi"), Some(&"false".to_string()));
        assert_eq!(
            parsed.get("Plugins.LoadAsiPlugins"),
            Some(&"true".to_string())
        );
    }

    #[test]
    fn optiscaler_fp8_variant_copies_selected_fsr4_dll() {
        let source = tempfile::tempdir().expect("source");
        let game = tempfile::tempdir().expect("game");
        std::fs::write(source.path().join("OptiScaler.dll"), b"dll").expect("source dll");
        std::fs::write(source.path().join("OptiScaler.ini"), "[FSR]\n").expect("source ini");
        std::fs::create_dir_all(source.path().join(FSR4_LATEST_DIR)).expect("latest dir");
        std::fs::write(
            source.path().join(FSR4_LATEST_DIR).join(FSR4_DLL_NAME),
            b"fp8",
        )
        .expect("fp8 dll");
        let mut config = OptiScaler.default_config();
        config.set("source_mode", serde_json::json!("local_dir"));
        config.set(
            "local_source_dir",
            serde_json::json!(source.path().display().to_string()),
        );
        config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_LATEST_FP8));

        let applied = OptiScaler.apply(game.path(), &config).expect("apply");

        assert_eq!(
            std::fs::read(game.path().join(FSR4_DLL_NAME)).expect("deployed FSR4"),
            b"fp8"
        );
        assert!(applied.files.contains(&PathBuf::from(FSR4_DLL_NAME)));
    }

    #[test]
    fn optiscaler_int8_variant_copies_int8_and_ignores_fp8_env() {
        let source = tempfile::tempdir().expect("source");
        let game = tempfile::tempdir().expect("game");
        std::fs::write(source.path().join("OptiScaler.dll"), b"dll").expect("source dll");
        std::fs::write(source.path().join("OptiScaler.ini"), "[FSR]\n").expect("source ini");
        std::fs::create_dir_all(source.path().join(FSR4_INT8_DIR)).expect("int8 dir");
        std::fs::write(
            source.path().join(FSR4_INT8_DIR).join(FSR4_DLL_NAME),
            b"int8",
        )
        .expect("int8 dll");
        let mut config = OptiScaler.default_config();
        config.set("source_mode", serde_json::json!("local_dir"));
        config.set(
            "local_source_dir",
            serde_json::json!(source.path().display().to_string()),
        );
        config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));
        config.set("emulate_fp8", serde_json::json!(true));

        OptiScaler.apply(game.path(), &config).expect("apply");

        assert_eq!(
            std::fs::read(game.path().join(FSR4_DLL_NAME)).expect("deployed FSR4"),
            b"int8"
        );
        assert!(OptiScaler.env_vars(&config).is_empty());
    }

    #[test]
    fn optiscaler_missing_optipatcher_is_reported_in_preview() {
        let source = tempfile::tempdir().expect("source");
        let game = tempfile::tempdir().expect("game");
        std::fs::write(source.path().join("OptiScaler.dll"), b"dll").expect("source dll");
        std::fs::write(source.path().join("OptiScaler.ini"), "[Plugins]\n").expect("source ini");
        let mut config = OptiScaler.default_config();
        config.set("source_mode", serde_json::json!("local_dir"));
        config.set(
            "local_source_dir",
            serde_json::json!(source.path().display().to_string()),
        );
        config.set("enable_optipatcher", serde_json::json!(true));

        let preview = OptiScaler
            .preview_apply_for(game.path(), None, &config)
            .expect("preview");

        let optipatcher_rel = PathBuf::from("plugins").join(OPTIPATCHER_ASSET);
        assert!(
            preview
                .missing_inputs
                .iter()
                .any(|input| input.contains("OptiPatcher.asi"))
                || preview.changed_files.contains(&optipatcher_rel)
                || preview.unchanged_files.contains(&optipatcher_rel)
        );
    }

    #[test]
    fn optiscaler_uses_bundled_optipatcher_from_source_plugins() {
        let source = tempfile::tempdir().expect("source");
        let game = tempfile::tempdir().expect("game");
        std::fs::write(source.path().join("OptiScaler.dll"), b"dll").expect("source dll");
        std::fs::write(source.path().join("OptiScaler.ini"), "[Plugins]\n").expect("source ini");
        std::fs::create_dir_all(source.path().join("plugins")).expect("plugins dir");
        std::fs::write(
            source.path().join("plugins").join(OPTIPATCHER_ASSET),
            b"bundled asi",
        )
        .expect("source optipatcher");
        let mut config = OptiScaler.default_config();
        config.set("source_mode", serde_json::json!("local_dir"));
        config.set(
            "local_source_dir",
            serde_json::json!(source.path().display().to_string()),
        );
        config.set("enable_optipatcher", serde_json::json!(true));

        OptiScaler.apply(game.path(), &config).expect("apply");

        assert_eq!(
            std::fs::read(game.path().join("plugins").join(OPTIPATCHER_ASSET))
                .expect("deployed optipatcher"),
            b"bundled asi"
        );
    }

    #[test]
    fn optiscaler_archive_payload_keeps_optipatcher_in_plugins() {
        assert!(is_optiscaler_payload_file("optipatcher.asi"));
        assert_eq!(
            optiscaler_payload_dest(
                Path::new("/cache"),
                Path::new("plugins/OptiPatcher.asi"),
                std::ffi::OsStr::new("OptiPatcher.asi"),
            ),
            PathBuf::from("/cache/plugins/OptiPatcher.asi")
        );
    }

    #[test]
    fn optiscaler_fp8_env_only_for_latest_fp8_emulation() {
        let mut config = OptiScaler.default_config();
        config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_LATEST_FP8));
        config.set("emulate_fp8", serde_json::json!(true));

        assert_eq!(
            OptiScaler.env_vars(&config).as_slice(),
            [(
                FP8_EMULATION_ENV_KEY.to_string(),
                FP8_EMULATION_ENV_VALUE.to_string()
            )]
        );

        config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));
        assert!(OptiScaler.env_vars(&config).is_empty());
    }

    #[test]
    fn goverlay_release_classification_maps_supported_channels() {
        assert_eq!(
            goverlay_release_channel("edge-0.9.12.0323"),
            Some("goverlay-edge")
        );
        assert_eq!(
            goverlay_release_channel("master-3ce61922"),
            Some("goverlay-master")
        );
        assert_eq!(
            goverlay_release_channel("any-release-0.9-2083b274"),
            Some("goverlay-any")
        );
        assert_eq!(goverlay_release_channel("0.9.1-0"), Some("goverlay-stable"));
        assert_eq!(goverlay_release_channel("fsr-int8"), None);
    }

    #[test]
    fn goverlay_release_summary_requires_full_install_archive() {
        let edge = release_fixture("edge-0.9.12.0323", &["notes.json", "optiscaler-edge.7z"]);
        let edge = goverlay_release_summary(edge).expect("edge release");
        assert_eq!(edge.tag, "goverlay-edge:edge-0.9.12.0323");
        assert_eq!(edge.assets.len(), 1);
        assert_eq!(edge.assets[0].name, "optiscaler-edge.7z");

        let master = release_fixture(
            "master-3ce61922",
            &["master-3ce61922.json", "OptiScaler_master_3ce61922.7z"],
        );
        let master = goverlay_release_summary(master).expect("master release");
        assert_eq!(master.tag, "goverlay-master:master-3ce61922");
        assert_eq!(master.assets[0].name, "OptiScaler_master_3ce61922.7z");

        let fsr_int8 = release_fixture("fsr-int8", &["amd_fidelityfx_upscaler_dx12.dll"]);
        assert!(goverlay_release_summary(fsr_int8).is_none());
    }

    #[test]
    fn optiscaler_release_tags_are_encoded_by_source() {
        assert_eq!(
            encode_optiscaler_release_tag("official", "v0.9.1"),
            "official:v0.9.1"
        );
        assert_eq!(
            normalize_optiscaler_release_tag("v0.9.1"),
            "official:v0.9.1"
        );
        assert_eq!(
            normalize_optiscaler_release_tag("goverlay-edge:edge-0.9.12.0323"),
            "goverlay-edge:edge-0.9.12.0323"
        );
    }

    #[test]
    fn optiscaler_release_asset_selection_uses_encoded_source_keys() {
        let releases = vec![
            release_fixture("official:v0.9.1", &["Optiscaler_0.9.1-final.7z"]),
            release_fixture("goverlay-edge:edge-0.9.12.0323", &["optiscaler-edge.7z"]),
        ];

        let (tag, asset) =
            select_optiscaler_release_asset(&releases, "v0.9.1", "Optiscaler_0.9.1-final.7z")
                .expect("legacy official tag resolves");
        assert_eq!(tag, "official:v0.9.1");
        assert_eq!(asset.name, "Optiscaler_0.9.1-final.7z");

        let (tag, asset) = select_optiscaler_release_asset(
            &releases,
            "goverlay-edge:edge-0.9.12.0323",
            "optiscaler-edge.7z",
        )
        .expect("goverlay edge tag resolves");
        assert_eq!(tag, "goverlay-edge:edge-0.9.12.0323");
        assert_eq!(asset.name, "optiscaler-edge.7z");

        assert!(
            select_optiscaler_release_asset(&releases, "edge-0.9.12.0323", "optiscaler-edge.7z")
                .is_err()
        );
    }

    #[test]
    fn optiscaler_release_config_moves_legacy_goverlay_tag_to_goverlay_source() {
        let mut config = OptiScaler.default_config();
        config.set("source_mode", serde_json::json!("github_release"));
        config.set(
            "release_tag",
            serde_json::json!("goverlay-edge:edge-0.9.12.0323"),
        );

        assert!(normalize_optiscaler_release_config(&mut config));

        assert_eq!(config.get_str("source_mode"), Some("goverlay_builds"));
        assert_eq!(config.get_str("goverlay_channel"), Some("edge"));
        assert_eq!(
            config.get_str("release_tag"),
            Some("goverlay-edge:edge-0.9.12.0323")
        );
    }

    #[test]
    fn optiscaler_release_matching_respects_source_and_channel() {
        let official = release_fixture("official:v0.9.1", &["Optiscaler.7z"]);
        let edge = release_fixture("goverlay-edge:edge-0.9.12.0323", &["optiscaler-edge.7z"]);
        let master = release_fixture(
            "goverlay-master:master-3ce61922",
            &["OptiScaler_master_3ce61922.7z"],
        );
        let mut config = OptiScaler.default_config();

        config.set("source_mode", serde_json::json!("github_release"));
        assert!(optiscaler_release_matches_config(&official, &config));
        assert!(!optiscaler_release_matches_config(&edge, &config));

        config.set("source_mode", serde_json::json!("goverlay_builds"));
        config.set("goverlay_channel", serde_json::json!("edge"));
        assert!(optiscaler_release_matches_config(&edge, &config));
        assert!(!optiscaler_release_matches_config(&master, &config));

        config.set("goverlay_channel", serde_json::json!("master"));
        assert!(!optiscaler_release_matches_config(&edge, &config));
        assert!(optiscaler_release_matches_config(&master, &config));
    }

    #[test]
    fn goverlay_release_sorts_newest_first_by_publish_date() {
        let mut releases = [
            release_fixture_with_date(
                "goverlay-edge:edge-old",
                &["optiscaler-edge.7z"],
                "2026-03-20T01:08:18Z",
            ),
            release_fixture_with_date(
                "goverlay-edge:edge-new",
                &["optiscaler-edge.7z"],
                "2026-03-24T00:18:25Z",
            ),
        ];

        releases.sort_by(|left, right| right.published_at.cmp(&left.published_at));

        assert_eq!(releases[0].tag, "goverlay-edge:edge-new");
        assert_eq!(releases[1].tag, "goverlay-edge:edge-old");
    }

    #[test]
    fn scanner_reports_absent_install() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let state = scan_optiscaler_install_in_dir(tmp.path(), &BTreeSet::new()).expect("scan");
        assert_eq!(state.status, OptiScalerInstallStatus::Absent);
        assert!(state.recognized_files.is_empty());
    }

    #[test]
    fn scanner_reports_unmanaged_install_with_config_and_companions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("dxgi.dll"), b"optiscaler").expect("proxy");
        std::fs::write(
            tmp.path().join("OptiScaler.ini"),
            "[OptiScaler]\nDxgi=false\n",
        )
        .expect("ini");
        std::fs::write(tmp.path().join("fakenvapi.dll"), b"fake").expect("companion");

        let state = scan_optiscaler_install_in_dir(tmp.path(), &BTreeSet::new()).expect("scan");
        assert_eq!(state.status, OptiScalerInstallStatus::Unmanaged);
        assert_eq!(state.proxy_dlls, vec!["dxgi.dll".to_string()]);
        assert_eq!(state.wine_dll_overrides, vec!["dxgi".to_string()]);
        assert_eq!(
            state.ini_settings.get("OptiScaler.Dxgi"),
            Some(&"false".to_string())
        );
        assert_eq!(state.recognized_files.len(), 3);
    }

    #[test]
    fn scanner_distinguishes_managed_and_conflicted_installs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("dxgi.dll"), b"optiscaler").expect("proxy");
        let mut managed = BTreeSet::new();
        managed.insert("dxgi.dll".to_string());
        let state = scan_optiscaler_install_in_dir(tmp.path(), &managed).expect("scan");
        assert_eq!(state.status, OptiScalerInstallStatus::Managed);

        std::fs::write(tmp.path().join("winmm.dll"), b"optiscaler").expect("proxy");
        let state = scan_optiscaler_install_in_dir(tmp.path(), &managed).expect("scan");
        assert_eq!(state.status, OptiScalerInstallStatus::Conflicted);
    }

    #[test]
    fn scanner_matches_stellar_blade_root_relative_managed_manifest() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let exe_dir = tmp.path().join("SB/Binaries/Win64");
        std::fs::create_dir_all(&exe_dir).expect("exe dir");
        std::fs::write(exe_dir.join("dxgi.dll"), b"optiscaler").expect("proxy");
        std::fs::write(exe_dir.join("OptiScaler.ini"), "[OptiScaler]\nDxgi=auto\n").expect("ini");
        std::fs::write(exe_dir.join("version.txt"), "v0.9.1\n").expect("version");

        let unmanaged =
            scan_optiscaler_install("stellar-blade", tmp.path(), &BTreeSet::new()).expect("scan");
        assert_eq!(unmanaged.status, OptiScalerInstallStatus::Unmanaged);
        assert_eq!(
            unmanaged.summary(),
            "unmanaged; version v0.9.1; proxy dxgi.dll"
        );
        assert_eq!(unmanaged.wine_dll_overrides, vec!["dxgi".to_string()]);

        let mut managed = BTreeSet::new();
        managed.insert("sb/binaries/win64/dxgi.dll".to_string());
        managed.insert("sb/binaries/win64/optiscaler.ini".to_string());
        let managed_state =
            scan_optiscaler_install("stellar-blade", tmp.path(), &managed).expect("scan");
        assert_eq!(managed_state.status, OptiScalerInstallStatus::Managed);
        assert_eq!(
            managed_state.summary(),
            "managed; version v0.9.1; proxy dxgi.dll"
        );
    }

    #[test]
    fn stellar_blade_default_config_adds_community_metadata_only() {
        let context = ToolGameContext::from_parts(
            "stellar-blade",
            "Stellar Blade",
            Some(PathBuf::from("/fake/StellarBlade")),
            None,
        );
        let config = OptiScaler.default_config_for(Some(&context));

        assert_eq!(
            config.get_str("source_mode"),
            Some(OPTISCALER_SOURCE_GOVERLAY_FGMOD)
        );
        assert_eq!(config.get_str("goverlay_channel"), Some("edge"));
        assert_eq!(config.get_str("release_tag"), Some("latest"));
        assert_eq!(config.get_str("release_asset"), Some(""));
        assert_eq!(config.get_str("proxy_dll"), Some("dxgi.dll"));
        assert_eq!(config.get_str("dll_overrides"), Some(""));
        assert!(config.get_bool("copy_companion_files"));
        assert!(!config.get_bool("enable_optipatcher"));
        assert_eq!(
            config.get_str("fsr4_variant"),
            Some(FSR4_VARIANT_LATEST_FP8)
        );
        assert!(!config.get_bool("emulate_fp8"));
        assert!(!config.get_bool("spoof_dlss"));
        assert_eq!(config.get_str("optiscaler_profile"), Some("community-dxgi"));
        assert_eq!(config.get_str("tested_optiscaler_version"), Some("0.9"));
        assert_eq!(
            config.get_str("optiscaler_profile_source_url"),
            Some("https://github.com/optiscaler/OptiScaler/wiki/Stellar-Blade")
        );
        assert_eq!(
            config.settings.get("ini_overrides"),
            Some(&serde_json::json!({}))
        );
    }

    #[test]
    fn games_without_optiscaler_profiles_keep_generic_defaults() {
        let context =
            ToolGameContext::from_parts("skyrim-se", "Skyrim Special Edition", None, None);
        let config = OptiScaler.default_config_for(Some(&context));

        assert_eq!(config.get_str("source_mode"), Some("goverlay_fgmod"));
        assert_eq!(config.get_str("release_tag"), Some("latest"));
        assert_eq!(config.get_str("proxy_dll"), Some("dxgi.dll"));
        assert_eq!(config.get_str("dll_overrides"), Some(""));
        assert_eq!(config.get_str("optiscaler_profile"), None);
    }

    #[test]
    fn custom_profile_opt_out_prevents_community_defaults() {
        let context = ToolGameContext::from_parts(
            "stellar-blade",
            "Stellar Blade",
            Some(PathBuf::from("/fake/StellarBlade")),
            None,
        );
        let mut config = OptiScaler.default_config();
        config.set("optiscaler_profile", serde_json::json!("custom"));
        config.set("source_mode", serde_json::json!("local_dir"));
        config.set("release_tag", serde_json::json!("latest"));
        config.set("proxy_dll", serde_json::json!("winmm.dll"));

        apply_game_defaults(&mut config, Some(&context));

        assert_eq!(config.get_str("optiscaler_profile"), Some("custom"));
        assert_eq!(config.get_str("source_mode"), Some("local_dir"));
        assert_eq!(config.get_str("release_tag"), Some("latest"));
        assert_eq!(config.get_str("proxy_dll"), Some("winmm.dll"));
        assert_eq!(config.get_str("tested_optiscaler_version"), None);
    }

    #[test]
    fn stellar_blade_game_defaults_preserve_custom_settings_for_profile() {
        let context = ToolGameContext::from_parts(
            "stellar-blade",
            "Stellar Blade",
            Some(PathBuf::from("/fake/StellarBlade")),
            None,
        );
        let mut config = OptiScaler.default_config();
        config.set("optiscaler_profile", serde_json::json!("community-dxgi"));
        config.set("source_mode", serde_json::json!("goverlay_builds"));
        config.set("goverlay_channel", serde_json::json!("edge"));
        config.set(
            "release_tag",
            serde_json::json!("goverlay-edge:edge-0.9.12.0323"),
        );
        config.set("release_asset", serde_json::json!("optiscaler-edge.7z"));
        config.set("proxy_dll", serde_json::json!("winmm.dll"));
        config.set("dll_overrides", serde_json::json!("winmm,nvngx"));
        config.set("copy_companion_files", serde_json::json!(false));
        config.set("enable_optipatcher", serde_json::json!(false));
        config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));
        config.set("emulate_fp8", serde_json::json!(true));
        config.set("spoof_dlss", serde_json::json!(true));
        config.set(
            "ini_overrides",
            serde_json::json!({"Spoofing": {"Dxgi": "false"}}),
        );

        apply_game_defaults(&mut config, Some(&context));

        assert_eq!(config.get_str("optiscaler_profile"), Some("community-dxgi"));
        assert_eq!(config.get_str("source_mode"), Some("goverlay_builds"));
        assert_eq!(config.get_str("goverlay_channel"), Some("edge"));
        assert_eq!(
            config.get_str("release_tag"),
            Some("goverlay-edge:edge-0.9.12.0323")
        );
        assert_eq!(config.get_str("release_asset"), Some("optiscaler-edge.7z"));
        assert_eq!(config.get_str("proxy_dll"), Some("winmm.dll"));
        assert_eq!(config.get_str("dll_overrides"), Some("winmm,nvngx"));
        assert!(!config.get_bool("copy_companion_files"));
        assert!(!config.get_bool("enable_optipatcher"));
        assert_eq!(config.get_str("fsr4_variant"), Some(FSR4_VARIANT_INT8_402));
        assert!(config.get_bool("emulate_fp8"));
        assert!(config.get_bool("spoof_dlss"));
        assert_eq!(
            config
                .settings
                .pointer("/ini_overrides/Spoofing/Dxgi")
                .and_then(serde_json::Value::as_str),
            Some("false")
        );
        assert_eq!(config.get_str("tested_optiscaler_version"), Some("0.9"));
    }

    #[test]
    fn stellar_blade_game_defaults_preserve_existing_release_without_profile_marker() {
        let context = ToolGameContext::from_parts(
            "stellar-blade",
            "Stellar Blade",
            Some(PathBuf::from("/fake/StellarBlade")),
            None,
        );
        let mut config = OptiScaler.default_config();
        config.set("source_mode", serde_json::json!("github_release"));
        config.set("release_tag", serde_json::json!("official:v0.9.11"));
        config.set("release_asset", serde_json::json!("OptiScaler_0.9.11.7z"));

        apply_game_defaults(&mut config, Some(&context));

        assert_eq!(config.get_str("optiscaler_profile"), Some("community-dxgi"));
        assert_eq!(config.get_str("source_mode"), Some("github_release"));
        assert_eq!(config.get_str("release_tag"), Some("official:v0.9.11"));
        assert_eq!(
            config.get_str("release_asset"),
            Some("OptiScaler_0.9.11.7z")
        );
        assert_eq!(config.get_str("proxy_dll"), Some("dxgi.dll"));
        assert!(!config.get_bool("enable_optipatcher"));
    }

    #[test]
    fn selecting_profile_after_custom_applies_metadata_only() {
        let mut config = OptiScaler.default_config();
        assert!(apply_profile_by_id(&mut config, "stellar-blade", "custom"));
        assert_eq!(config.get_str("optiscaler_profile"), Some("custom"));
        config.set("source_mode", serde_json::json!("local_dir"));
        config.set("release_tag", serde_json::json!("custom-build"));
        config.set("release_asset", serde_json::json!("CustomOptiScaler.7z"));
        config.set("proxy_dll", serde_json::json!("winmm.dll"));
        config.set("dll_overrides", serde_json::json!("winmm,nvngx"));
        config.set("copy_companion_files", serde_json::json!(false));
        config.set("enable_optipatcher", serde_json::json!(false));
        config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));
        config.set("emulate_fp8", serde_json::json!(true));
        config.set("spoof_dlss", serde_json::json!(true));

        assert!(apply_profile_by_id(
            &mut config,
            "stellar-blade",
            "community-dxgi"
        ));

        assert_eq!(config.get_str("optiscaler_profile"), Some("community-dxgi"));
        assert_eq!(config.get_str("source_mode"), Some("local_dir"));
        assert_eq!(config.get_str("goverlay_channel"), Some("edge"));
        assert_eq!(config.get_str("release_tag"), Some("custom-build"));
        assert_eq!(config.get_str("release_asset"), Some("CustomOptiScaler.7z"));
        assert_eq!(config.get_str("proxy_dll"), Some("winmm.dll"));
        assert_eq!(config.get_str("dll_overrides"), Some("winmm,nvngx"));
        assert!(!config.get_bool("copy_companion_files"));
        assert!(!config.get_bool("enable_optipatcher"));
        assert_eq!(config.get_str("fsr4_variant"), Some(FSR4_VARIANT_INT8_402));
        assert!(config.get_bool("emulate_fp8"));
        assert!(config.get_bool("spoof_dlss"));
        assert_eq!(config.get_str("tested_optiscaler_version"), Some("0.9"));
    }

    #[test]
    fn optiscaler_profile_metadata_does_not_change_settings() {
        let profile = crate::optiscaler::OptiScalerProfile {
            id: "test-profile",
            name: "Test Profile",
            source_url: "https://example.test/profile",
            tested_optiscaler_version: "1.2.3",
            source_mode: None,
            goverlay_channel: None,
            proxy_dll: "winmm.dll",
            release_tag: Some("v1.2.3"),
            release_asset: Some("OptiScaler.7z"),
            wine_dll_overrides: &["winmm", "nvngx"],
            copy_companion_files: false,
            enable_optipatcher: true,
            fsr4_variant: Some(FSR4_VARIANT_INT8_402),
            emulate_fp8: true,
            spoof_dlss: true,
            ini_overrides: &[crate::optiscaler::OptiScalerIniOverride {
                key: "Spoofing.Dxgi",
                value: "false",
            }],
            notes: "Test notes",
        };
        let mut config = OptiScaler.default_config();
        config.set("source_mode", serde_json::json!("local_dir"));
        config.set("release_tag", serde_json::json!("custom-build"));
        config.set("release_asset", serde_json::json!("CustomOptiScaler.7z"));
        config.set("proxy_dll", serde_json::json!("dxgi.dll"));
        config.set("dll_overrides", serde_json::json!("dxgi"));
        config.set("copy_companion_files", serde_json::json!(true));
        config.set("enable_optipatcher", serde_json::json!(false));
        config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_LATEST_FP8));
        config.set("emulate_fp8", serde_json::json!(false));
        config.set("spoof_dlss", serde_json::json!(false));
        config.set(
            "ini_overrides",
            serde_json::json!({"OptiScaler": {"Dxgi": "auto"}}),
        );

        apply_optiscaler_profile_metadata(&mut config, &profile);

        assert_eq!(config.get_str("optiscaler_profile"), Some("test-profile"));
        assert_eq!(config.get_str("source_mode"), Some("local_dir"));
        assert_eq!(config.get_str("release_tag"), Some("custom-build"));
        assert_eq!(config.get_str("release_asset"), Some("CustomOptiScaler.7z"));
        assert_eq!(config.get_str("proxy_dll"), Some("dxgi.dll"));
        assert_eq!(config.get_str("dll_overrides"), Some("dxgi"));
        assert!(config.get_bool("copy_companion_files"));
        assert!(!config.get_bool("enable_optipatcher"));
        assert_eq!(
            config.get_str("fsr4_variant"),
            Some(FSR4_VARIANT_LATEST_FP8)
        );
        assert!(!config.get_bool("emulate_fp8"));
        assert!(!config.get_bool("spoof_dlss"));
        assert_eq!(
            config
                .settings
                .pointer("/ini_overrides/OptiScaler/Dxgi")
                .and_then(serde_json::Value::as_str),
            Some("auto")
        );
        assert_eq!(config.get_str("tested_optiscaler_version"), Some("1.2.3"));
        assert_eq!(
            config.get_str("optiscaler_profile_source_url"),
            Some("https://example.test/profile")
        );
    }

    #[test]
    fn optiscaler_release_selection_preserves_custom_profile_deployment() {
        let mut config = OptiScaler.default_config();
        config.set("optiscaler_profile", serde_json::json!("community-dxgi"));
        config.set("proxy_dll", serde_json::json!("winmm.dll"));
        config.set("dll_overrides", serde_json::json!("winmm,nvngx"));
        config.set("copy_companion_files", serde_json::json!(false));
        config.set("enable_optipatcher", serde_json::json!(false));
        config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));
        config.set("emulate_fp8", serde_json::json!(true));
        config.set("spoof_dlss", serde_json::json!(true));
        config.set(
            "ini_overrides",
            serde_json::json!({"Spoofing": {"Dxgi": "false"}}),
        );

        apply_optiscaler_release_selection(&mut config, "official:v0.9.11", "OptiScaler_0.9.11.7z");

        assert_eq!(config.get_str("optiscaler_profile"), Some("community-dxgi"));
        assert_eq!(config.get_str("source_mode"), Some("github_release"));
        assert_eq!(config.get_str("release_tag"), Some("official:v0.9.11"));
        assert_eq!(
            config.get_str("release_asset"),
            Some("OptiScaler_0.9.11.7z")
        );
        assert_eq!(config.get_str("proxy_dll"), Some("winmm.dll"));
        assert_eq!(config.get_str("dll_overrides"), Some("winmm,nvngx"));
        assert!(!config.get_bool("copy_companion_files"));
        assert!(!config.get_bool("enable_optipatcher"));
        assert_eq!(config.get_str("fsr4_variant"), Some(FSR4_VARIANT_INT8_402));
        assert!(config.get_bool("emulate_fp8"));
        assert!(config.get_bool("spoof_dlss"));
        assert_eq!(
            config
                .settings
                .pointer("/ini_overrides/Spoofing/Dxgi")
                .and_then(serde_json::Value::as_str),
            Some("false")
        );
    }

    #[test]
    fn incompatible_existing_ini_triggers_reset_reason() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let new_ini = tmp.path().join("new.ini");
        std::fs::write(&new_ini, "[OptiScaler]\nDxgi=auto\n").expect("new ini");
        let state = OptiScalerInstallState {
            status: OptiScalerInstallStatus::Unmanaged,
            executable_dir: tmp.path().to_path_buf(),
            proxy_dlls: vec![],
            wine_dll_overrides: vec![],
            config_path: None,
            ini_settings: BTreeMap::from([("Removed.SectionKey".to_string(), "true".to_string())]),
            companion_files: vec![],
            recognized_files: vec![],
            version: OptiScalerVersionIdentity::Unknown,
            latest_backup: None,
        };
        let config = OptiScaler.default_config();
        assert_eq!(
            optiscaler_config_reset_reason(&state, &new_ini, &config).as_deref(),
            Some("schema mismatch")
        );
    }

    fn release_fixture(tag: &str, asset_names: &[&str]) -> ToolReleaseSummary {
        ToolReleaseSummary {
            tag: tag.to_string(),
            name: None,
            published_at: None,
            assets: asset_names
                .iter()
                .map(|name| ToolReleaseAsset {
                    name: (*name).to_string(),
                    download_url: format!("https://example.test/{name}"),
                    size: 1,
                })
                .collect(),
        }
    }

    fn release_fixture_with_date(
        tag: &str,
        asset_names: &[&str],
        published_at: &str,
    ) -> ToolReleaseSummary {
        let mut release = release_fixture(tag, asset_names);
        release.published_at = Some(published_at.to_string());
        release
    }

    // ── set_ini_value ─────────────────────────────────────────────────

    #[test]
    fn set_ini_value_updates_existing_key_in_section() {
        let content = "[OptiScaler]\nDxgi=auto\nLoadAsiPlugins=false\n";
        let updated = set_ini_value(content, "OptiScaler.Dxgi", "manual");
        assert!(updated.contains("Dxgi=manual"));
        assert!(updated.contains("LoadAsiPlugins=false"));
    }

    #[test]
    fn set_ini_value_appends_section_when_missing() {
        let updated = set_ini_value("", "Menu.Scale", "1.25");
        assert!(updated.contains("[Menu]"));
        assert!(updated.contains("Scale=1.25"));
    }

    #[test]
    fn set_ini_value_returns_content_unchanged_when_key_has_no_section() {
        let content = "[OptiScaler]\nDxgi=auto\n";
        let updated = set_ini_value(content, "no_section_prefix", "x");
        assert_eq!(updated, content, "malformed key should not mutate content");
    }

    // ── relative_to_game ──────────────────────────────────────────────

    #[test]
    fn relative_to_game_strips_game_prefix() {
        let game = PathBuf::from("/games/skyrim");
        let dest = PathBuf::from("/games/skyrim/Data/SKSE/plugins/foo.dll");
        let rel = relative_to_game(&game, &dest).expect("strips prefix");
        assert_eq!(rel, PathBuf::from("Data/SKSE/plugins/foo.dll"));
    }

    #[test]
    fn relative_to_game_errors_when_dest_outside_game_dir() {
        let game = PathBuf::from("/games/skyrim");
        let dest = PathBuf::from("/elsewhere/foo.dll");
        let err = relative_to_game(&game, &dest).expect_err("must fail");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("/elsewhere/foo.dll") && msg.contains("/games/skyrim"),
            "error must mention both paths: {msg}"
        );
    }
}
