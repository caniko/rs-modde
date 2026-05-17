use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use reqwest::Client;

use modde_core::manifest::wabbajack::DownloadDirective;

use crate::common::ensure_parent;
use crate::traits::{DownloadHandle, DownloadSource, ProgressCallback, VerifiedFile};

use super::catalog::download_authored_file_to_path;
use super::preflight::preflight_authored_files;

/// Wabbajack-authored CDN archives served through the authored-files chunk API.
pub struct WabbajackCdnSource {
    client: Client,
}

impl WabbajackCdnSource {
    #[must_use]
    pub const fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn preflight_archives(
        &self,
        archives: &[modde_core::manifest::wabbajack::ArchiveEntry],
    ) -> Result<()> {
        preflight_authored_files(&self.client, archives).await
    }
}

impl DownloadSource for WabbajackCdnSource {
    fn can_handle(&self, directive: &DownloadDirective) -> bool {
        matches!(directive, DownloadDirective::WabbajackCdn { .. })
    }

    async fn resolve(&self, directive: &DownloadDirective) -> Result<DownloadHandle> {
        let DownloadDirective::WabbajackCdn { url, hash } = directive else {
            anyhow::bail!("not a WabbajackCdn directive");
        };

        Ok(DownloadHandle {
            url: url.clone(),
            candidate_urls: Vec::new(),
            headers: HashMap::new(),
            expected_hash: *hash,
            size_hint: None,
        })
    }

    async fn download_with_progress(
        &self,
        handle: DownloadHandle,
        dest: &Path,
        progress: ProgressCallback,
    ) -> Result<VerifiedFile> {
        ensure_parent(dest).await?;
        download_authored_file_to_path(
            &self.client,
            &handle.url,
            dest,
            Some(handle.expected_hash),
            Some(&progress),
        )
        .await?;
        Ok(VerifiedFile {
            path: dest.to_path_buf(),
            hash: handle.expected_hash,
        })
    }
}
