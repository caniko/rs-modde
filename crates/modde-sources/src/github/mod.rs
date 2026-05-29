//! GitHub Releases download source: resolves release assets and downloads
//! them, honouring `GITHUB_TOKEN` when present.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use tracing::info;

use modde_core::manifest::wabbajack::DownloadDirective;

use crate::common::{ensure_parent, simple_download, with_retry};
use crate::error::{SourceError, SourceResult, status_error};
use crate::traits::{DownloadHandle, DownloadSource, ProgressCallback, VerifiedFile};

/// GitHub Releases download source.
pub struct GitHubSource {
    client: Client,
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: Option<String>,
    name: Option<String>,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// Summary of a single GitHub release: its tag, optional name, and assets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubReleaseSummary {
    pub tag: String,
    pub name: Option<String>,
    pub assets: Vec<GitHubReleaseAsset>,
}

/// A downloadable asset attached to a GitHub release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

impl GitHubSource {
    /// Create a source over the given HTTP `client`, picking up `GITHUB_TOKEN`
    /// from the environment for authenticated requests.
    #[must_use]
    pub fn new(client: Client) -> Self {
        let token = std::env::var("GITHUB_TOKEN").ok();
        Self { client, token }
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> SourceResult<T> {
        let mut req = self.client.get(url).header("User-Agent", "modde");
        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        Ok(status_error(req.send().await?)?.json().await?)
    }

    pub async fn list_releases(&self, user: &str, repo: &str) -> Result<Vec<GitHubReleaseSummary>> {
        let url = format!("https://api.github.com/repos/{user}/{repo}/releases");
        let releases: Vec<Release> = self.get_json(&url).await?;
        Ok(releases.into_iter().filter_map(release_summary).collect())
    }

    pub async fn release_by_tag(
        &self,
        user: &str,
        repo: &str,
        tag: &str,
    ) -> Result<GitHubReleaseSummary> {
        let url = format!("https://api.github.com/repos/{user}/{repo}/releases/tags/{tag}");
        let release: Release = self.get_json(&url).await?;
        release_summary(release).ok_or_else(|| anyhow::anyhow!("release {tag} has no tag name"))
    }

    pub async fn download_release_asset(
        &self,
        user: &str,
        repo: &str,
        tag: &str,
        asset: &str,
        dest: &Path,
    ) -> Result<VerifiedFile> {
        let release = self.release_by_tag(user, repo, tag).await?;
        let found = release
            .assets
            .iter()
            .find(|candidate| candidate.name == asset)
            .ok_or_else(|| anyhow::anyhow!("asset '{asset}' not found in release {tag}"))?;
        let handle = DownloadHandle {
            url: found.browser_download_url.clone(),
            candidate_urls: Vec::new(),
            headers: HashMap::new(),
            expected_hash: 0,
            size_hint: Some(found.size),
        };
        ensure_parent(dest).await?;
        let progress: ProgressCallback = Arc::new(|_, _| {});
        let resp = status_error(self.client.get(&handle.url).send().await?)?;
        let total = handle.size_hint.unwrap_or(0);
        let mut file = tokio::fs::File::create(dest).await?;
        let mut stream = resp.bytes_stream();
        let mut downloaded = 0;
        use futures::StreamExt;
        use tokio::io::AsyncWriteExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            progress(downloaded, total);
        }
        file.flush().await?;
        let hash = modde_core::hash::hash_file_xxhash(dest).await?;
        Ok(VerifiedFile {
            path: dest.to_path_buf(),
            hash,
        })
    }
}

fn release_summary(release: Release) -> Option<GitHubReleaseSummary> {
    Some(GitHubReleaseSummary {
        tag: release.tag_name?,
        name: release.name,
        assets: release
            .assets
            .into_iter()
            .map(|asset| GitHubReleaseAsset {
                name: asset.name,
                browser_download_url: asset.browser_download_url,
                size: asset.size,
            })
            .collect(),
    })
}

impl DownloadSource for GitHubSource {
    fn can_handle(&self, directive: &DownloadDirective) -> bool {
        matches!(directive, DownloadDirective::GitHub { .. })
    }

    async fn resolve(&self, directive: &DownloadDirective) -> SourceResult<DownloadHandle> {
        let DownloadDirective::GitHub {
            user,
            repo,
            tag,
            asset,
            hash,
        } = directive
        else {
            return Err(SourceError::other(anyhow::anyhow!(
                "not a GitHub directive"
            )));
        };

        let url = format!("https://api.github.com/repos/{user}/{repo}/releases/tags/{tag}");

        let mut req = self.client.get(&url).header("User-Agent", "modde");
        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }

        let release: Release = status_error(req.send().await?)?.json().await?;

        let found = release
            .assets
            .iter()
            .find(|a| a.name == *asset)
            .ok_or_else(|| {
                SourceError::other(anyhow::anyhow!(
                    "asset '{asset}' not found in release {tag}"
                ))
            })?;

        info!(repo = %format!("{user}/{repo}"), tag, asset, "resolved GitHub release asset");

        Ok(DownloadHandle {
            url: found.browser_download_url.clone(),
            candidate_urls: Vec::new(),
            headers: HashMap::new(),
            expected_hash: *hash,
            size_hint: Some(found.size),
        })
    }

    async fn download_with_progress(
        &self,
        handle: DownloadHandle,
        dest: &Path,
        progress: ProgressCallback,
    ) -> SourceResult<VerifiedFile> {
        let client = self.client.clone();
        let handle_ref = &handle;
        let dest_ref = dest;
        let progress_ref = &progress;

        with_retry("GitHub download", || async {
            simple_download(&client, handle_ref, dest_ref, progress_ref).await
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_release_summary_preserves_tag_and_asset_names() {
        let release: Release = serde_json::from_str(
            r#"{
                "tag_name": "v0.7.7",
                "name": "OptiScaler v0.7.7",
                "assets": [
                    {
                        "name": "OptiScaler_v0.7.7.zip",
                        "browser_download_url": "https://example.test/OptiScaler.zip",
                        "size": 1234
                    }
                ]
            }"#,
        )
        .expect("release json parses");
        let summary = release_summary(release).expect("release has tag");
        assert_eq!(summary.tag, "v0.7.7");
        assert_eq!(summary.assets[0].name, "OptiScaler_v0.7.7.zip");
        assert_eq!(summary.assets[0].size, 1234);
    }
}
