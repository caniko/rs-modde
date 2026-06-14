use super::*;
use crate::nexus::base_url;

impl NexusApi {
    /// Search collections for a game.
    pub async fn search_collections(
        &self,
        game_domain: &str,
        query: &str,
    ) -> Result<Vec<CollectionManifest>> {
        let url = format!(
            "{}/games/{game_domain}/collections.json?search={query}",
            base_url()
        );
        self.get(&url).await
    }

    /// Get a specific collection by slug.
    pub async fn get_collection(
        &self,
        game_domain: &str,
        slug: &str,
    ) -> Result<CollectionManifest> {
        let url = format!("{}/games/{game_domain}/collections/{slug}.json", base_url());
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
            "{}/games/{game_domain}/collections/{slug}/revisions/{revision}.json",
            base_url()
        );
        self.get(&url).await
    }

    /// Discover a collection's game domain (and latest revision) by slug alone.
    ///
    /// Step 1 of the two-step collection install flow.
    pub async fn get_collection_meta(&self, slug: &str) -> Result<NexusCollectionMeta> {
        // The collections endpoint accepts a slug without game_domain:
        //   GET /v1/collections/{slug}.json
        let url = format!("{}/collections/{slug}.json", base_url());
        self.get(&url).await
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
                .ok_or_else(|| anyhow::anyhow!("collection '{slug}' has no published revisions"))?;
            (meta.game.domain_name, rev)
        };

        self.get_collection_revision(&game_domain, slug, revision)
            .await
    }
}
