use super::*;
use crate::nexus::base_url;

impl NexusApi {
    /// Endorse a mod on Nexus.
    ///
    /// The v1 endpoint requires a `Version` form parameter — passing the
    /// installed mod version lets Nexus reject endorsements of obsolete
    /// installs. Callers should pass the version string from the currently
    /// loaded `NexusMod` response (not the local install, which may be
    /// stale).
    pub async fn endorse_mod(
        &self,
        game_domain: &str,
        mod_id: NexusModId,
        version: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/games/{game_domain}/mods/{mod_id}/endorse.json",
            base_url()
        );
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
    pub async fn abstain_mod(
        &self,
        game_domain: &str,
        mod_id: NexusModId,
        version: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/games/{game_domain}/mods/{mod_id}/abstain.json",
            base_url()
        );
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
        let url = format!("{}/user/tracked_mods.json", base_url());
        self.get(&url).await
    }

    /// Track a mod (receive Nexus notifications).
    pub async fn track_mod(&self, game_domain: &str, mod_id: NexusModId) -> Result<()> {
        let url = format!("{}/user/tracked_mods.json", base_url());
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
    pub async fn untrack_mod(&self, game_domain: &str, mod_id: NexusModId) -> Result<()> {
        let url = format!("{}/user/tracked_mods.json", base_url());
        self.delete_req(
            &url,
            &[
                ("domain_name", game_domain),
                ("mod_id", &mod_id.to_string()),
            ],
        )
        .await
    }
}
