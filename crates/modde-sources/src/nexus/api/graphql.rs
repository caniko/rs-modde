#![allow(clippy::wildcard_imports)]
use super::*;
use crate::nexus::graphql::{
    GqlCollectionTile, GqlModTile, ModFeedKind, browse_feed, collections_feed,
    search_mods as search_mods_gql_impl,
};

impl NexusApi {
    pub async fn browse_feed_gql(
        &self,
        game_domain: &str,
        kind: ModFeedKind,
    ) -> Result<Vec<GqlModTile>> {
        match browse_feed(&self.client, &self.api_key, game_domain, kind).await {
            Ok(tiles) => Ok(tiles),
            Err(e) => {
                warn!(error = %e, "GraphQL browse feed failed, falling back to REST");
                let mods = self.trending_mods(game_domain).await?;
                Ok(mods
                    .into_iter()
                    .map(|m| GqlModTile {
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
    ) -> Result<Vec<GqlModTile>> {
        match search_mods_gql_impl(&self.client, &self.api_key, game_domain, term, page).await {
            Ok(tiles) => Ok(tiles),
            Err(e) => {
                warn!(error = %e, "GraphQL search failed, falling back to REST");
                let results = self.search_mods(game_domain, term, page).await?;
                Ok(results
                    .results
                    .into_iter()
                    .map(|m| GqlModTile {
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
    ) -> Result<Vec<GqlCollectionTile>> {
        match collections_feed(&self.client, &self.api_key, game_domain, term).await {
            Ok(tiles) => Ok(tiles),
            Err(e) => {
                warn!(error = %e, "GraphQL collections feed failed, falling back to REST");
                let results = self
                    .search_collections(game_domain, term.unwrap_or(""))
                    .await?;
                Ok(results
                    .into_iter()
                    .map(|c| GqlCollectionTile {
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
}
