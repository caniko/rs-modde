pub mod api;
pub mod auth;
pub mod cdn;
pub mod oauth;
pub mod updates;

pub use api::NexusApi;

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use reqwest::Client;
use tracing::debug;

use modde_core::manifest::wabbajack::DownloadDirective;

use crate::common::simple_download;
use crate::traits::{DownloadHandle, DownloadSource, ProgressCallback, VerifiedFile};

/// NexusMods download source.
///
/// Requires a Nexus Premium account and API key.
pub struct NexusSource {
    client: Client,
    api_key: String,
}

impl NexusSource {
    /// Create a new NexusSource, loading the API key from environment or keyring.
    pub fn new(client: Client) -> Result<Self> {
        let api_key = auth::load_api_key()?;
        Ok(Self { client, api_key })
    }

    /// Create a new NexusSource with an explicit API key.
    pub fn with_api_key(client: Client, api_key: String) -> Self {
        Self { client, api_key }
    }
}

impl DownloadSource for NexusSource {
    fn can_handle(&self, directive: &DownloadDirective) -> bool {
        matches!(directive, DownloadDirective::Nexus { .. })
    }

    async fn resolve(&self, directive: &DownloadDirective) -> Result<DownloadHandle> {
        let DownloadDirective::Nexus {
            game_id,
            mod_id,
            file_id,
            hash,
        } = directive
        else {
            anyhow::bail!("not a Nexus directive");
        };

        let download_url = cdn::generate_download_link(
            &self.client,
            &self.api_key,
            game_id.as_str(),
            *mod_id,
            *file_id,
        )
        .await?;

        debug!(url = %download_url, "resolved Nexus CDN download URL");

        Ok(DownloadHandle {
            url: download_url,
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
        simple_download(&self.client, &handle, dest, &progress).await
    }
}
