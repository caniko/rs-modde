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

#[derive(Debug, Clone, Deserialize)]
pub struct NexusMod {
    pub mod_id: u64,
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
    pub mod_id: u64,
    pub domain_name: String,
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
    #[must_use]
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
        if let Some(remaining) = resp.headers().get("x-rl-hourly-remaining")
            && let Ok(val) = remaining.to_str().unwrap_or("").parse::<u32>()
                && val < 10 {
                    warn!(remaining = val, "Nexus API hourly rate limit running low");
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

    // ── GraphQL v2 browse helpers ─────────────────────────────

    /// Fetch a trending or monthly-top browse feed via the v2 GraphQL
    /// endpoint. Falls back to the REST `trending_mods` path when the
    /// GraphQL response is malformed, so the UI still renders something
    /// even if the v2 schema changes shape out from under us.
    pub async fn browse_feed_gql(
        &self,
        game_domain: &str,
        kind: super::graphql::ModFeedKind,
    ) -> Result<Vec<super::graphql::GqlModTile>> {
        match super::graphql::browse_feed(&self.client, &self.api_key, game_domain, kind).await {
            Ok(tiles) => Ok(tiles),
            Err(e) => {
                warn!(error = %e, "GraphQL browse feed failed, falling back to REST");
                let mods = self.trending_mods(game_domain).await?;
                Ok(mods
                    .into_iter()
                    .map(|m| super::graphql::GqlModTile {
                        mod_id: m.mod_id,
                        name: m.name,
                        summary: m.summary,
                        version: Some(m.version),
                        author: Some(m.author),
                        picture_url: m.picture_url.clone(),
                        thumbnail_url: m.picture_url,
                        endorsements: Some(m.endorsement_count),
                        downloads: None,
                        uploaded_at: None,
                        game_domain: m.domain_name,
                    })
                    .collect())
            }
        }
    }

    /// Full-text search via the v2 GraphQL endpoint, with a REST
    /// fallback mirroring `browse_feed_gql`.
    pub async fn search_mods_gql(
        &self,
        game_domain: &str,
        term: &str,
        page: u32,
    ) -> Result<Vec<super::graphql::GqlModTile>> {
        match super::graphql::search_mods(&self.client, &self.api_key, game_domain, term, page)
            .await
        {
            Ok(tiles) => Ok(tiles),
            Err(e) => {
                warn!(error = %e, "GraphQL search failed, falling back to REST");
                let results = self.search_mods(game_domain, term, page).await?;
                Ok(results
                    .results
                    .into_iter()
                    .map(|m| super::graphql::GqlModTile {
                        mod_id: m.mod_id,
                        name: m.name,
                        summary: m.summary,
                        version: Some(m.version),
                        author: Some(m.author),
                        picture_url: m.picture_url.clone(),
                        thumbnail_url: m.picture_url,
                        endorsements: Some(m.endorsement_count),
                        downloads: None,
                        uploaded_at: None,
                        game_domain: m.domain_name,
                    })
                    .collect())
            }
        }
    }

    /// Collections browse / search via the v2 GraphQL endpoint. Falls
    /// back to the REST `search_collections` path.
    pub async fn collections_feed_gql(
        &self,
        game_domain: &str,
        term: Option<&str>,
    ) -> Result<Vec<super::graphql::GqlCollectionTile>> {
        match super::graphql::collections_feed(&self.client, &self.api_key, game_domain, term).await
        {
            Ok(tiles) => Ok(tiles),
            Err(e) => {
                warn!(error = %e, "GraphQL collections feed failed, falling back to REST");
                let results = self
                    .search_collections(game_domain, term.unwrap_or(""))
                    .await?;
                Ok(results
                    .into_iter()
                    .map(|c| super::graphql::GqlCollectionTile {
                        slug: c.slug,
                        name: c.name,
                        summary: c.summary,
                        tile_image: c.image_url,
                        game_domain: Some(c.game.domain_name),
                        endorsements: Some(c.endorsements),
                        downloads: None,
                    })
                    .collect())
            }
        }
    }

    /// Fetch raw bytes from a URL, reusing the client + apikey header.
    ///
    /// Used for downloading thumbnail / gallery images referenced by the v1 API.
    /// The apikey header is harmless on image CDN URLs (ignored by the CDN),
    /// but keeping it here means one code path with consistent auth.
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

    /// Fetch the full image gallery for a mod via the unofficial v2 GraphQL
    /// endpoint. Returns a list of image URLs (the main `picture_url` will
    /// typically be the first entry, but this is not guaranteed — the caller
    /// should merge with `picture_url` as a fallback).
    ///
    /// The GraphQL schema is undocumented and may change; on any error this
    /// function returns an `Err` and the caller should fall back to the
    /// single `picture_url` from the v1 `get_mod` response.
    pub async fn get_mod_media(&self, game_domain: &str, mod_id: u64) -> Result<Vec<String>> {
        let query = r"query ModMedia($modId: Int!, $gameDomain: String!) {
  mod(modId: $modId, gameDomain: $gameDomain) {
    modImages { url }
  }
}";
        let body = serde_json::json!({
            "query": query,
            "variables": {
                "modId": mod_id,
                "gameDomain": game_domain,
            },
        });

        let resp = self
            .client
            .post("https://api.nexusmods.com/v2/graphql")
            .header("apikey", &self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        let payload: serde_json::Value = resp.json().await?;
        if let Some(errors) = payload.get("errors") {
            bail!("Nexus GraphQL errors: {errors}");
        }
        let images = payload
            .get("data")
            .and_then(|d| d.get("mod"))
            .and_then(|m| m.get("modImages"))
            .and_then(|a| a.as_array())
            .ok_or_else(|| anyhow::anyhow!("unexpected GraphQL response shape"))?;

        let urls: Vec<String> = images
            .iter()
            .filter_map(|img| {
                img.get("url")
                    .and_then(|u| u.as_str())
                    .map(std::string::ToString::to_string)
            })
            .collect();
        Ok(urls)
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
        let url =
            format!("{BASE_URL}/games/{game_domain}/mods/search.json?search={query}&page={page}");
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
        let url = format!("{BASE_URL}/games/{game_domain}/collections.json?search={query}");
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
        let url =
            format!("{BASE_URL}/games/{game_domain}/collections/{slug}/revisions/{revision}.json");
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
    ///
    /// The v1 endpoint requires a `Version` form parameter — passing the
    /// installed mod version lets Nexus reject endorsements of obsolete
    /// installs. Callers should pass the version string from the currently
    /// loaded `NexusMod` response (not the local install, which may be
    /// stale).
    pub async fn endorse_mod(&self, game_domain: &str, mod_id: u64, version: &str) -> Result<()> {
        let url = format!("{BASE_URL}/games/{game_domain}/mods/{mod_id}/endorse.json");
        self.client
            .post(&url)
            .header("apikey", &self.api_key)
            .form(&[("Version", version)])
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// Abstain from endorsing (won't be asked again).
    pub async fn abstain_mod(&self, game_domain: &str, mod_id: u64, version: &str) -> Result<()> {
        let url = format!("{BASE_URL}/games/{game_domain}/mods/{mod_id}/abstain.json");
        self.client
            .post(&url)
            .header("apikey", &self.api_key)
            .form(&[("Version", version)])
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// Fetch the full list of mods the current user is tracking, across all
    /// games. The v1 endpoint is not filterable by domain, so callers that
    /// only care about one mod should filter the returned list themselves.
    pub async fn get_tracked_mods(&self) -> Result<Vec<NexusTrackedMod>> {
        let url = format!("{BASE_URL}/user/tracked_mods.json");
        self.get(&url).await
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
        let (game_domain, revision) = if let Some(rev) = version {
            // Still need the game domain; do step-1 but skip revision lookup
            let meta = self.get_collection_meta(slug).await?;
            (meta.game.domain_name, rev)
        } else {
            let meta = self.get_collection_meta(slug).await?;
            let rev = meta
                .latest_published_revision
                .map(|r| r.revision_number)
                .ok_or_else(|| {
                    anyhow::anyhow!("collection '{slug}' has no published revisions")
                })?;
            (meta.game.domain_name, rev)
        };

        self.get_collection_revision(&game_domain, slug, revision)
            .await
    }
}
