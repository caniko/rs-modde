use anyhow::{Result, bail};
use modde_core::manifest::collection::CollectionManifest;
use reqwest::Client;
use serde::Deserialize;
use tracing::warn;

const BASE_URL: &str = "https://api.nexusmods.com/v1";

/// Typed Nexus API client.
pub struct NexusApi {
    client: Client,
    api_key: String,
}

#[derive(Debug, Deserialize)]
pub struct NexusMod {
    pub mod_id: u64,
    pub name: String,
    pub summary: Option<String>,
    pub version: String,
    pub author: String,
}

#[derive(Debug, Deserialize)]
pub struct NexusModFile {
    pub file_id: u64,
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

#[derive(Debug, Deserialize)]
pub struct NexusCollectionGame {
    pub domain_name: String,
}

#[derive(Debug, Deserialize)]
pub struct NexusCollectionRevision {
    pub revision_number: u64,
}

#[derive(Debug, Deserialize)]
pub struct NexusModFiles {
    pub files: Vec<NexusModFile>,
}

#[derive(Debug, Deserialize)]
pub struct NexusSearchResults {
    pub results: Vec<NexusMod>,
    pub total: u64,
}

#[derive(Debug, Deserialize)]
pub struct NexusUpdatedMod {
    pub mod_id: u64,
    pub latest_file_update: u64,
    pub latest_mod_activity: u64,
}

impl NexusApi {
    pub fn new(client: Client, api_key: String) -> Self {
        Self { client, api_key }
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let resp = self
            .client
            .get(url)
            .header("apikey", &self.api_key)
            .send()
            .await?;

        // Check rate limit headers
        if let Some(remaining) = resp.headers().get("x-rl-hourly-remaining") {
            if let Ok(val) = remaining.to_str().unwrap_or("").parse::<u32>() {
                if val < 10 {
                    warn!(remaining = val, "Nexus API hourly rate limit running low");
                }
            }
        }

        if resp.status() == 429 {
            bail!("Nexus API rate limit exceeded. Please wait before retrying.");
        }

        let body = resp.error_for_status()?.json().await?;
        Ok(body)
    }

    async fn post<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let resp = self
            .client
            .post(url)
            .header("apikey", &self.api_key)
            .send()
            .await?;

        if let Some(remaining) = resp.headers().get("x-rl-hourly-remaining") {
            if let Ok(val) = remaining.to_str().unwrap_or("").parse::<u32>() {
                if val < 10 {
                    warn!(remaining = val, "Nexus API hourly rate limit running low");
                }
            }
        }

        if resp.status() == 429 {
            bail!("Nexus API rate limit exceeded. Please wait before retrying.");
        }

        let body = resp.error_for_status()?.json().await?;
        Ok(body)
    }

    async fn delete_req(&self, url: &str, form: &[(&str, &str)]) -> Result<()> {
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
    pub async fn get_mod(&self, game_domain: &str, mod_id: u64) -> Result<NexusMod> {
        let url = format!("{BASE_URL}/games/{game_domain}/mods/{mod_id}.json");
        self.get(&url).await
    }

    /// Get files for a mod.
    pub async fn get_mod_files(&self, game_domain: &str, mod_id: u64) -> Result<NexusModFiles> {
        let url = format!("{BASE_URL}/games/{game_domain}/mods/{mod_id}/files.json");
        self.get(&url).await
    }

    /// Search mods by query string.
    pub async fn search_mods(
        &self,
        game_domain: &str,
        query: &str,
        page: u32,
    ) -> Result<NexusSearchResults> {
        let url = format!(
            "{BASE_URL}/games/{game_domain}/mods/search.json?search={query}&page={page}",
        );
        self.get(&url).await
    }

    /// Get trending mods for a game.
    pub async fn trending_mods(&self, game_domain: &str) -> Result<Vec<NexusMod>> {
        let url = format!("{BASE_URL}/games/{game_domain}/mods/trending.json");
        self.get(&url).await
    }

    /// Get recently updated mods. Period must be `"1d"`, `"1w"`, or `"1m"`.
    pub async fn updated_mods(
        &self,
        game_domain: &str,
        period: &str,
    ) -> Result<Vec<NexusUpdatedMod>> {
        let url = format!("{BASE_URL}/games/{game_domain}/mods/updated.json?period={period}");
        self.get(&url).await
    }

    /// Search collections for a game.
    pub async fn search_collections(
        &self,
        game_domain: &str,
        query: &str,
    ) -> Result<Vec<CollectionManifest>> {
        let url = format!(
            "{BASE_URL}/games/{game_domain}/collections.json?search={query}",
        );
        self.get(&url).await
    }

    /// Get a specific collection by slug.
    pub async fn get_collection(
        &self,
        game_domain: &str,
        slug: &str,
    ) -> Result<CollectionManifest> {
        let url = format!("{BASE_URL}/games/{game_domain}/collections/{slug}.json");
        self.get(&url).await
    }

    /// Get a specific revision of a collection.
    pub async fn get_collection_revision(
        &self,
        game_domain: &str,
        slug: &str,
        revision: u64,
    ) -> Result<CollectionManifest> {
        let url = format!(
            "{BASE_URL}/games/{game_domain}/collections/{slug}/revisions/{revision}.json"
        );
        self.get(&url).await
    }

    /// Discover a collection's game domain (and latest revision) by slug alone.
    ///
    /// Step 1 of the two-step collection install flow.
    pub async fn get_collection_meta(&self, slug: &str) -> Result<NexusCollectionMeta> {
        // The collections endpoint accepts a slug without game_domain:
        //   GET /v1/collections/{slug}.json
        let url = format!("{BASE_URL}/collections/{slug}.json");
        self.get(&url).await
    }

    /// Endorse a mod on Nexus.
    pub async fn endorse_mod(&self, game_domain: &str, mod_id: u64) -> Result<()> {
        let url = format!("{BASE_URL}/games/{game_domain}/mods/{mod_id}/endorse.json");
        let _: serde_json::Value = self.post(&url).await?;
        Ok(())
    }

    /// Abstain from endorsing (won't be asked again).
    pub async fn abstain_mod(&self, game_domain: &str, mod_id: u64) -> Result<()> {
        let url = format!("{BASE_URL}/games/{game_domain}/mods/{mod_id}/abstain.json");
        let _: serde_json::Value = self.post(&url).await?;
        Ok(())
    }

    /// Track a mod (receive Nexus notifications).
    pub async fn track_mod(&self, game_domain: &str, mod_id: u64) -> Result<()> {
        let url = format!("{BASE_URL}/user/tracked_mods.json");
        self.client
            .post(&url)
            .header("apikey", &self.api_key)
            .form(&[
                ("domain_name", game_domain),
                ("mod_id", &mod_id.to_string()),
            ])
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// Stop tracking a mod.
    pub async fn untrack_mod(&self, game_domain: &str, mod_id: u64) -> Result<()> {
        let url = format!("{BASE_URL}/user/tracked_mods.json");
        self.delete_req(
            &url,
            &[
                ("domain_name", game_domain),
                ("mod_id", &mod_id.to_string()),
            ],
        )
        .await
    }

    /// Fetch a collection manifest, discovering the game domain automatically.
    ///
    /// If `version` is `Some`, that revision number is used directly.
    /// Otherwise the latest published revision is queried first (two-step fetch).
    pub async fn get_collection_by_slug(
        &self,
        slug: &str,
        version: Option<u64>,
    ) -> Result<CollectionManifest> {
        let (game_domain, revision) = match version {
            Some(rev) => {
                // Still need the game domain; do step-1 but skip revision lookup
                let meta = self.get_collection_meta(slug).await?;
                (meta.game.domain_name, rev)
            }
            None => {
                let meta = self.get_collection_meta(slug).await?;
                let rev = meta
                    .latest_published_revision
                    .map(|r| r.revision_number)
                    .ok_or_else(|| anyhow::anyhow!(
                        "collection '{slug}' has no published revisions"
                    ))?;
                (meta.game.domain_name, rev)
            }
        };

        self.get_collection_revision(&game_domain, slug, revision).await
    }
}
