//! Queries the GitHub releases API to enumerate tool releases and their assets,
//! mapping them into [`ToolReleaseSummary`] values for tool installation.

use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;

use super::{ToolReleaseAsset, ToolReleaseSummary};

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: Option<String>,
    name: Option<String>,
    published_at: Option<String>,
    assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

pub(crate) async fn github_json<T: for<'de> Deserialize<'de>>(
    client: &Client,
    url: &str,
) -> Result<T> {
    let mut request = client.get(url).header("User-Agent", "modde");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    Ok(request.send().await?.error_for_status()?.json().await?)
}

pub async fn list_github_releases(repo: &str) -> Result<Vec<ToolReleaseSummary>> {
    let client = Client::new();
    let releases: Vec<GitHubRelease> = github_json(
        &client,
        &format!("https://api.github.com/repos/{repo}/releases?per_page=100"),
    )
    .await?;
    Ok(releases
        .into_iter()
        .filter_map(|release| {
            Some(ToolReleaseSummary {
                tag: release.tag_name?,
                name: release.name,
                published_at: release.published_at,
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

#[must_use]
pub fn prepend_latest_dedup<I>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut out = vec!["latest".to_string()];
    for value in values {
        if value.trim().is_empty() || out.iter().any(|existing| existing == &value) {
            continue;
        }
        out.push(value);
    }
    out
}
