//! Nexus Mods integration: the typed REST/GraphQL client, API-key and OAuth
//! authentication, CDN download links, update checks, and collection installs.

pub mod api;
pub mod auth;
pub mod cdn;
pub mod graphql;
pub mod install;
pub mod oauth;
pub mod updates;

pub use api::NexusApi;

const DEFAULT_BASE_URL: &str = "https://api.nexusmods.com/v1";
const DEFAULT_GRAPHQL_URL: &str = "https://api.nexusmods.com/v2/graphql";

/// Base URL for the v1 REST API.
///
/// Honours `MODDE_NEXUS_BASE_URL` so integration tests can point the
/// client at a local mock server (e.g. wiremock). Production code never
/// sets the var, so it falls through to the official endpoint.
#[must_use]
pub fn base_url() -> String {
    std::env::var("MODDE_NEXUS_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string())
}

/// Base URL for the v2 GraphQL API. Same override semantics as
/// [`base_url`] but via `MODDE_NEXUS_GRAPHQL_URL`.
#[must_use]
pub fn graphql_url() -> String {
    std::env::var("MODDE_NEXUS_GRAPHQL_URL").unwrap_or_else(|_| DEFAULT_GRAPHQL_URL.to_string())
}

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use reqwest::Client;
use tracing::debug;

use modde_core::manifest::wabbajack::DownloadDirective;

use crate::common::simple_download;
use crate::error::{SourceError, SourceResult};
use crate::traits::{DownloadHandle, DownloadSource, ProgressCallback, VerifiedFile};

/// `NexusMods` download source.
///
/// Requires a Nexus Premium account and API key.
pub struct NexusSource {
    client: Client,
    api_key: String,
}

impl NexusSource {
    /// Create a new `NexusSource`, loading the API key from environment or keyring.
    pub fn new(client: Client) -> Result<Self> {
        let api_key = auth::load_api_key()?;
        Ok(Self { client, api_key })
    }

    /// Create a new `NexusSource` with an explicit API key.
    #[must_use]
    pub fn with_api_key(client: Client, api_key: String) -> Self {
        Self { client, api_key }
    }
}

impl DownloadSource for NexusSource {
    fn can_handle(&self, directive: &DownloadDirective) -> bool {
        matches!(directive, DownloadDirective::Nexus { .. })
    }

    async fn resolve(&self, directive: &DownloadDirective) -> SourceResult<DownloadHandle> {
        let DownloadDirective::Nexus {
            game_id,
            mod_id,
            file_id,
            hash,
        } = directive
        else {
            return Err(SourceError::other(anyhow::anyhow!("not a Nexus directive")));
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
    ) -> SourceResult<VerifiedFile> {
        simple_download(&self.client, &handle, dest, &progress).await
    }
}
