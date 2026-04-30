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
use serde::Deserialize;
use smallvec::{SmallVec, smallvec};
use tokio::io::AsyncWriteExt;
use tracing::info;
use xxhash_rust::xxh64::Xxh64;

use super::{
    AppliedFiles, GameTool, ToolAvailability, ToolCategory, ToolConfig, ToolGameContext,
    ToolReleaseAsset, ToolReleaseInstallFuture, ToolReleaseListFuture, ToolReleaseSummary,
};

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
        _context: Option<&ToolGameContext>,
        config: &ToolConfig,
    ) -> Vec<super::ToolSettingSpec> {
        let mut specs = vec![
            super::ToolSettingSpec::select(
                "source_mode",
                "Source",
                "Where modde should get OptiScaler files from.",
                &["github_release", "goverlay_fgmod", "local_dir"],
            ),
            super::ToolSettingSpec::select(
                "release_tag",
                "Release tag",
                "OptiScaler GitHub release tag selected in the UI.",
                std::slice::from_ref(&config.get_str("release_tag").unwrap_or("latest")),
            ),
            super::ToolSettingSpec::select(
                "release_asset",
                "Release asset",
                "Release asset selected from GitHub.",
                std::slice::from_ref(&config.get_str("release_asset").unwrap_or("")),
            ),
            super::ToolSettingSpec::select(
                "proxy_dll",
                "Proxy DLL",
                "DLL name used to load OptiScaler for this game.",
                &[
                    "dxgi.dll",
                    "version.dll",
                    "dbghelp.dll",
                    "d3d12.dll",
                    "wininet.dll",
                    "winhttp.dll",
                    "winmm.dll",
                    "nvngx.dll",
                    "OptiScaler.asi",
                ],
            ),
            super::ToolSettingSpec::text(
                "dll_overrides",
                "DLL overrides",
                "Comma or whitespace separated Wine DLL override base names.",
            ),
            super::ToolSettingSpec::bool(
                "copy_companion_files",
                "Copy companion files",
                "Copy fakenvapi, nvngx wrapper, and other DLLs found next to OptiScaler.",
            ),
            super::ToolSettingSpec::read_only(
                "derived_executable_dir",
                "Executable directory",
                "Derived from the selected game's metadata.",
            ),
        ];

        if config.get_str("source_mode") == Some("local_dir") {
            specs.insert(
                1,
                super::ToolSettingSpec::path(
                    "local_source_dir",
                    "Local source directory",
                    "Directory containing OptiScaler.dll and companion files.",
                ),
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

    fn env_vars(&self, _config: &ToolConfig) -> SmallVec<[(String, String); 4]> {
        SmallVec::new()
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
            let rel = dest.strip_prefix(game_dir).unwrap_or(&dest).to_path_buf();
            applied.files.push(rel);
            info!(as_dll = %dll_name, "applied OptiScaler DLL");
        }

        // Copy OptiScaler.ini if present (or generate default)
        let ini_src = source_dir.join("OptiScaler.ini");
        let ini_dest = target_dir.join("OptiScaler.ini");
        if ini_src.exists() {
            let reset_reason = optiscaler_config_reset_reason(&existing, &ini_src, config);
            if reset_reason.is_some() {
                std::fs::copy(&ini_src, &ini_dest)?;
            } else {
                apply_ini_overrides_with_existing(
                    &ini_src,
                    ini_dest.exists().then_some(&ini_dest),
                    &ini_dest,
                    config,
                )?;
            }
            let rel = ini_dest
                .strip_prefix(game_dir)
                .unwrap_or(&ini_dest)
                .to_path_buf();
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
                    && name.to_ascii_lowercase().ends_with(".dll")
                {
                    let dest = target_dir.join(name);
                    std::fs::copy(&src, &dest)?;
                    let rel = dest.strip_prefix(game_dir).unwrap_or(&dest).to_path_buf();
                    applied.files.push(rel);
                }
            }
        }

        if source_dir.join("D3D12_OptiScaler").is_dir() {
            let dest = target_dir.join("D3D12_OptiScaler");
            copy_dir_recursive(&source_dir.join("D3D12_OptiScaler"), &dest)?;
            collect_relative_files(game_dir, &dest, &mut applied.files)?;
        }

        Ok(applied)
    }

    fn default_config(&self) -> ToolConfig {
        let mut config = ToolConfig::new("optiscaler");
        config.set("source_mode", serde_json::json!("goverlay_fgmod"));
        config.set("release_tag", serde_json::json!("latest"));
        config.set("release_asset", serde_json::json!(""));
        config.set("local_source_dir", serde_json::json!(""));
        config.set("proxy_dll", serde_json::json!("dxgi.dll"));
        config.set("dll_overrides", serde_json::json!(""));
        config.set("copy_companion_files", serde_json::json!(true));
        config.set("ini_overrides", serde_json::json!({}));
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
            config.set("source_mode", serde_json::json!("github_release"));
            config.set("release_tag", serde_json::json!(tag));
            config.set("release_asset", serde_json::json!(asset));
            Ok(config)
        })
    }
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: Option<String>,
    name: Option<String>,
    assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

async fn github_json<T: for<'de> Deserialize<'de>>(client: &Client, url: &str) -> Result<T> {
    let mut request = client.get(url).header("User-Agent", "modde");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    Ok(request.send().await?.error_for_status()?.json().await?)
}

pub async fn list_optiscaler_releases() -> Result<Vec<ToolReleaseSummary>> {
    let client = Client::new();
    let releases: Vec<GitHubRelease> = github_json(
        &client,
        "https://api.github.com/repos/optiscaler/OptiScaler/releases",
    )
    .await?;
    Ok(releases
        .into_iter()
        .filter_map(|release| {
            Some(ToolReleaseSummary {
                tag: release.tag_name?,
                name: release.name,
                assets: release
                    .assets
                    .into_iter()
                    .map(|asset| ToolReleaseAsset {
                        name: asset.name,
                        download_url: asset.browser_download_url,
                        size: asset.size,
                    })
                    .collect(),
            })
        })
        .collect())
}

pub async fn install_optiscaler_release_asset(tag: &str, asset_name: &str) -> Result<PathBuf> {
    let release = list_optiscaler_releases()
        .await?
        .into_iter()
        .find(|release| release.tag == tag)
        .ok_or_else(|| anyhow::anyhow!("OptiScaler release tag not found: {tag}"))?;
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .ok_or_else(|| anyhow::anyhow!("asset '{asset_name}' not found in release {tag}"))?;
    if !is_installable_release_asset(&asset.name) {
        anyhow::bail!(
            "selected asset '{}' is not a supported archive (.zip or .7z)",
            asset.name
        );
    }

    let cache_dir = cached_release_dir(tag);
    std::fs::create_dir_all(&cache_dir)?;
    let archive_path = cache_dir.join(&asset.name);
    download_release_asset(asset, &archive_path).await?;
    extract_optiscaler_archive_flat(&archive_path, &cache_dir)?;
    Ok(cache_dir)
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

#[must_use]
pub fn cached_release_dir(tag: &str) -> PathBuf {
    modde_core::paths::modde_data_dir()
        .join("tools")
        .join("optiscaler")
        .join(sanitize_tag(tag))
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
        "github_release" => {
            let tag = config.get_str("release_tag")?;
            let dir = cached_release_dir(tag);
            dir.join("OptiScaler.dll").exists().then_some(dir)
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

fn goverlay_optiscaler_ini_specs() -> Vec<super::ToolSettingSpec> {
    vec![
        super::ToolSettingSpec::text(
            "ini_overrides.Menu.ShortcutKey",
            "Menu shortcut",
            "OptiScaler [Menu] ShortcutKey override.",
        ),
        super::ToolSettingSpec::text(
            "ini_overrides.Menu.Scale",
            "Menu scale",
            "OptiScaler [Menu] Scale override.",
        ),
        super::ToolSettingSpec::text(
            "ini_overrides.OptiScaler.OverrideNvapiDll",
            "Override NVAPI DLL",
            "OptiScaler OverrideNvapiDll override.",
        ),
        super::ToolSettingSpec::text(
            "ini_overrides.OptiScaler.Dxgi",
            "DXGI mode",
            "OptiScaler Dxgi override.",
        ),
        super::ToolSettingSpec::text(
            "ini_overrides.OptiScaler.LoadAsiPlugins",
            "Load ASI plugins",
            "OptiScaler LoadAsiPlugins override.",
        ),
        super::ToolSettingSpec::text(
            "ini_overrides.OptiScaler.Fsr4Update",
            "FSR4 update",
            "OptiScaler Fsr4Update override.",
        ),
        super::ToolSettingSpec::text(
            "ini_overrides.fakenvapi.force_reflex",
            "Force Reflex",
            "fakenvapi force_reflex override.",
        ),
        super::ToolSettingSpec::text(
            "ini_overrides.fakenvapi.force_latencyflex",
            "Force LatencyFlex",
            "fakenvapi force_latencyflex override.",
        ),
        super::ToolSettingSpec::text(
            "ini_overrides.fakenvapi.latencyflex_mode",
            "LatencyFlex mode",
            "fakenvapi latencyflex_mode override.",
        ),
        super::ToolSettingSpec::text(
            "ini_overrides.fakenvapi.enable_trace_logs",
            "Trace logs",
            "fakenvapi enable_trace_logs override.",
        ),
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
        .take(24)
        .map(|key| {
            let leaked: &'static str = Box::leak(format!("ini_overrides.{key}").into_boxed_str());
            let label: &'static str = Box::leak(key.into_boxed_str());
            super::ToolSettingSpec::text(leaked, label, "OptiScaler.ini override.")
        })
        .collect()
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
        let Some(name) = entry
            .enclosed_name()
            .and_then(|path| path.file_name().map(ToOwned::to_owned))
        else {
            continue;
        };
        let Some(name_str) = name.to_str() else {
            continue;
        };
        let lower = name_str.to_ascii_lowercase();
        if !is_optiscaler_payload_file(&lower) {
            continue;
        }
        let out = dest_dir.join(name);
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
                std::fs::copy(&path, dest_dir.join(name))?;
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
        "optiscaler.dll" | "optiscaler.ini" | "fakenvapi.dll" | "nvngx-wrapper.dll"
    ) || lower_name.ends_with(".dll")
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
            content = set_ini_value(&content, &path, &value);
        }
    }
    std::fs::write(dest, content)?;
    Ok(())
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

pub fn scan_optiscaler_install(
    game_id: &str,
    game_dir: &Path,
    managed_paths: &BTreeSet<String>,
) -> Result<OptiScalerInstallState> {
    let executable_dir = crate::resolve_game_plugin(game_id)
        .map(|plugin| plugin.executable_dir(game_dir))
        .unwrap_or_else(|| game_dir.to_path_buf());
    let mut state = scan_optiscaler_install_in_dir(&executable_dir, managed_paths)?;
    state.latest_backup = latest_optiscaler_backup(Some(game_id));
    Ok(state)
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
            out.push(path.strip_prefix(game_dir).unwrap_or(&path).to_path_buf());
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
    let (target_section, target_key) = path.rsplit_once('.').unwrap_or(("", path));
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

    use super::*;

    #[test]
    fn parse_optiscaler_ini_preserves_section_paths() {
        let parsed = parse_optiscaler_ini(
            r#"
            ; comment
            [OptiScaler]
            Dxgi=auto
            LoadAsiPlugins=true
            [Menu]
            Scale=1.25
            "#,
        );
        assert_eq!(parsed.get("OptiScaler.Dxgi"), Some(&"auto".to_string()));
        assert_eq!(
            parsed.get("OptiScaler.LoadAsiPlugins"),
            Some(&"true".to_string())
        );
        assert_eq!(parsed.get("Menu.Scale"), Some(&"1.25".to_string()));
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
}
