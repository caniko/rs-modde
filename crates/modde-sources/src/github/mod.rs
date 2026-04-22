use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use tracing::info;

use modde_core::manifest::wabbajack::DownloadDirective;

use crate::common::{simple_download, with_retry};
use crate::traits::{DownloadHandle, DownloadSource, ProgressCallback, VerifiedFile};

/// GitHub Releases download source.
pub struct GitHubSource {
    client: Client,
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Release {
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

impl GitHubSource {
    pub fn new(client: Client) -> Self {
        let token = std::env::var("GITHUB_TOKEN").ok();
        Self { client, token }
    }
}

impl DownloadSource for GitHubSource {
    fn can_handle(&self, directive: &DownloadDirective) -> bool {
        matches!(directive, DownloadDirective::GitHub { .. })
    }

    async fn resolve(&self, directive: &DownloadDirective) -> Result<DownloadHandle> {
        let DownloadDirective::GitHub {
            user,
            repo,
            tag,
            asset,
            hash,
        } = directive
        else {
            anyhow::bail!("not a GitHub directive");
        };

        let url = format!("https://api.github.com/repos/{user}/{repo}/releases/tags/{tag}");

        let mut req = self.client.get(&url).header("User-Agent", "modde");
        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }

        let release: Release = req.send().await?.error_for_status()?.json().await?;

        let found = release
            .assets
            .iter()
            .find(|a| a.name == *asset)
            .ok_or_else(|| anyhow::anyhow!("asset '{asset}' not found in release {tag}"))?;

        info!(repo = %format!("{user}/{repo}"), tag, asset, "resolved GitHub release asset");

        Ok(DownloadHandle {
            url: found.browser_download_url.clone(),
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
    ) -> Result<VerifiedFile> {
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
