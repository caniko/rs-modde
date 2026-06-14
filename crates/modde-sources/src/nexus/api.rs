//! Typed Nexus Mods v1 REST API client and the response types it deserializes.

use anyhow::{Result, bail};
use modde_core::manifest::collection::CollectionManifest;
use modde_core::{NexusFileId, NexusModId};
use reqwest::Client;
use serde::Deserialize;
use tracing::warn;

/// Typed Nexus API client.
#[derive(Clone)]
pub struct NexusApi {
    client: Client,
    api_key: String,
}

/// A mod's metadata as returned by the Nexus v1 mod endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct NexusMod {
    pub mod_id: NexusModId,
    pub name: String,
    pub summary: Option<String>,
    pub version: String,
    pub author: String,
    /// Primary thumbnail URL (full-size picture shown at the top of the mod page).
    #[serde(default)]
    pub picture_url: Option<String>,
    /// Long-form HTML description. May contain BBCode-derived markup.
    #[serde(default)]
    pub description: Option<String>,
    /// Nexus game domain the mod belongs to (e.g. `"skyrimspecialedition"`).
    #[serde(default)]
    pub domain_name: Option<String>,
    /// The current user's endorsement relationship to this mod. Only
    /// populated on authenticated requests. Absent otherwise.
    #[serde(default)]
    pub endorsement: Option<NexusEndorsement>,
    /// Total endorsements the mod has received (not user-specific).
    #[serde(default)]
    pub endorsement_count: u64,
}

/// The current user's endorsement status for a mod.
///
/// `endorse_status` values returned by Nexus v1: `"Undecided"`, `"Abstained"`,
/// `"Endorsed"`. See `node-nexus-api/lib/types.d.ts` (`EndorsedStatus`) for the
/// canonical enum.
#[derive(Debug, Clone, Deserialize)]
pub struct NexusEndorsement {
    pub endorse_status: String,
    #[serde(default)]
    pub timestamp: Option<u64>,
    #[serde(default)]
    pub version: Option<String>,
}

/// A single entry in the user's tracked-mods list.
#[derive(Debug, Clone, Deserialize)]
pub struct NexusTrackedMod {
    pub mod_id: NexusModId,
    pub domain_name: String,
}

/// Metadata for a single downloadable file attached to a mod.
#[derive(Debug, Deserialize)]
pub struct NexusModFile {
    pub file_id: NexusFileId,
    pub name: String,
    pub version: Option<String>,
    pub size_kb: Option<u64>,
    pub file_name: String,
    /// File category: `"MAIN"`, `"UPDATE"`, `"OPTIONAL"`, `"OLD_VERSION"`, `"MISCELLANEOUS"`.
    #[serde(default)]
    pub category_name: Option<String>,
    /// Upload timestamp (Unix epoch seconds). Used to pick the most-recent MAIN file.
    #[serde(default)]
    pub uploaded_timestamp: Option<u64>,
}

/// Minimal collection metadata returned by the slug-based lookup endpoint.
///
/// Used in the two-step collection install flow to discover the game domain
/// before fetching the full revision manifest.
#[derive(Debug, Deserialize)]
pub struct NexusCollectionMeta {
    pub game: NexusCollectionGame,
    #[serde(default)]
    pub latest_published_revision: Option<NexusCollectionRevision>,
}

/// The game a collection belongs to.
#[derive(Debug, Deserialize)]
pub struct NexusCollectionGame {
    pub domain_name: String,
}

/// A published revision of a collection.
#[derive(Debug, Deserialize)]
pub struct NexusCollectionRevision {
    pub revision_number: u64,
}

/// The file listing for a mod.
#[derive(Debug, Deserialize)]
pub struct NexusModFiles {
    pub files: Vec<NexusModFile>,
}

/// A page of mod search results plus the total match count.
#[derive(Debug, Deserialize)]
pub struct NexusSearchResults {
    pub results: Vec<NexusMod>,
    pub total: u64,
}

/// An entry from the "recently updated mods" feed.
#[derive(Debug, Deserialize)]
pub struct NexusUpdatedMod {
    pub mod_id: NexusModId,
    pub latest_file_update: u64,
    pub latest_mod_activity: u64,
}

mod collections;
mod graphql;
mod media;
mod mods;
mod social;

impl NexusApi {
    /// Create a client authenticated with the given `api_key`.
    #[must_use]
    pub fn new(client: Client, api_key: String) -> Self {
        Self { client, api_key }
    }

    pub(super) async fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let resp = self
            .client
            .get(url)
            .header("apikey", &self.api_key)
            .send()
            .await?;

        // Check rate limit headers
        if let Some(remaining) = resp.headers().get("x-rl-hourly-remaining")
            && let Ok(val) = remaining.to_str().unwrap_or("").parse::<u32>()
            && val < 10
        {
            warn!(remaining = val, "Nexus API hourly rate limit running low");
        }

        if resp.status() == 429 {
            bail!("Nexus API rate limit exceeded. Please wait before retrying.");
        }

        let body = resp.error_for_status()?.json().await?;
        Ok(body)
    }

    pub(super) async fn delete_req(&self, url: &str, form: &[(&str, &str)]) -> Result<()> {
        self.client
            .delete(url)
            .header("apikey", &self.api_key)
            .form(form)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// Get mod details.
    pub async fn get_mod(&self, game_domain: &str, mod_id: NexusModId) -> Result<NexusMod> {
        let url = format!(
            "{}/games/{game_domain}/mods/{mod_id}.json",
            super::base_url()
        );
        self.get(&url).await
    }

    // ── GraphQL v2 browse helpers ─────────────────────────────

    /// Fetch a trending or monthly-top browse feed via the v2 GraphQL
    /// endpoint. Falls back to the REST `trending_mods` path when the
    /// GraphQL response is malformed, so the UI still renders something
    /// even if the v2 schema changes shape out from under us.
    pub async fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self
            .client
            .get(url)
            .header("apikey", &self.api_key)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.bytes().await?.to_vec())
    }
}
