use super::*;
use crate::nexus::base_url;

impl NexusApi {
    /// Get files for a mod.
    pub async fn get_mod_files(
        &self,
        game_domain: &str,
        mod_id: NexusModId,
    ) -> Result<NexusModFiles> {
        let url = format!(
            "{}/games/{game_domain}/mods/{mod_id}/files.json",
            base_url()
        );
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
            "{}/games/{game_domain}/mods/search.json?search={query}&page={page}",
            base_url()
        );
        self.get(&url).await
    }

    /// Get trending mods for a game.
    pub async fn trending_mods(&self, game_domain: &str) -> Result<Vec<NexusMod>> {
        let url = format!("{}/games/{game_domain}/mods/trending.json", base_url());
        self.get(&url).await
    }

    /// Get recently updated mods. Period must be `"1d"`, `"1w"`, or `"1m"`.
    pub async fn updated_mods(
        &self,
        game_domain: &str,
        period: &str,
    ) -> Result<Vec<NexusUpdatedMod>> {
        let url = format!(
            "{}/games/{game_domain}/mods/updated.json?period={period}",
            base_url()
        );
        self.get(&url).await
    }
}
