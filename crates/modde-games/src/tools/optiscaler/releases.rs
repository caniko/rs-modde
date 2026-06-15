#![allow(clippy::wildcard_imports)]
use super::*;
use crate::tools::release;

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
    std::fs::create_dir_all(&cache_dir)
        .with_context(|| format!("failed to create {}", cache_dir.display()))?;
    let archive_path = cache_dir.join(&asset.name);
    download_release_asset(asset, &archive_path).await?;
    extract_optiscaler_archive_flat(&archive_path, &cache_dir)?;
    Ok(cache_dir)
}

pub fn install_optiscaler_release_asset_from_path(
    tag: &str,
    asset_name: &str,
    path: &Path,
) -> Result<PathBuf> {
    let normalized_tag = normalize_optiscaler_release_tag(tag);
    if !is_installable_release_asset(asset_name) {
        anyhow::bail!("selected asset '{asset_name}' is not a supported archive (.zip or .7z)");
    }

    let cache_dir = cached_release_dir(&normalized_tag);
    std::fs::create_dir_all(&cache_dir)
        .with_context(|| format!("failed to create {}", cache_dir.display()))?;
    let archive_path = cache_dir.join(asset_name);
    std::fs::copy(path, &archive_path).with_context(|| {
        format!(
            "failed to copy release asset {} to {}",
            path.display(),
            archive_path.display()
        )
    })?;
    extract_optiscaler_archive_flat(&archive_path, &cache_dir)?;
    Ok(cache_dir)
}

pub async fn install_latest_optipatcher() -> Result<PathBuf> {
    let release = release::list_github_releases(OPTIPATCHER_REPO)
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

pub(super) fn normalize_optiscaler_release_tag(tag: &str) -> String {
    if tag.contains(':') {
        tag.to_string()
    } else {
        encode_optiscaler_release_tag("official", tag)
    }
}

pub(super) fn select_optiscaler_release_asset<'a>(
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

pub(super) async fn official_optiscaler_releases() -> Result<Vec<ToolReleaseSummary>> {
    Ok(release::list_github_releases("optiscaler/OptiScaler")
        .await?
        .into_iter()
        .map(|mut release| {
            release.tag = encode_optiscaler_release_tag("official", &release.tag);
            release
        })
        .collect())
}

pub(super) async fn goverlay_optiscaler_releases() -> Result<Vec<ToolReleaseSummary>> {
    let mut releases: Vec<_> = release::list_github_releases("benjamimgois/OptiScaler-builds")
        .await?
        .into_iter()
        .filter_map(goverlay_release_summary)
        .collect();
    releases.sort_by(|left, right| right.published_at.cmp(&left.published_at));
    Ok(releases)
}

pub(super) fn goverlay_release_summary(
    mut release: ToolReleaseSummary,
) -> Option<ToolReleaseSummary> {
    let channel = goverlay_release_channel(&release.tag)?;
    let assets = goverlay_installable_assets(channel, &release);
    if assets.is_empty() {
        return None;
    }
    release.tag = encode_optiscaler_release_tag(channel, &release.tag);
    release.assets = assets;
    Some(release)
}

pub(super) fn encode_optiscaler_release_tag(source: &str, tag: &str) -> String {
    if tag.contains(':') {
        tag.to_string()
    } else {
        format!("{source}:{tag}")
    }
}

pub(super) fn goverlay_release_channel(tag: &str) -> Option<&'static str> {
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

pub(super) fn is_goverlay_stable_tag(tag: &str) -> bool {
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

pub(super) fn goverlay_installable_assets(
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

pub(super) fn release_assets_named(
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

pub(super) async fn download_release_asset(asset: &ToolReleaseAsset, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let client = Client::new();
    let response = client
        .get(&asset.download_url)
        .header("User-Agent", "modde")
        .send()
        .await?
        .error_for_status()?;
    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("failed to create {}", dest.display()))?;
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
