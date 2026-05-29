use reqwest::Client;
use serde::Deserialize;

use modde_core::{NexusFileId, NexusModId};

use super::auth;
use crate::error::{SourceError, SourceResult, status_error};
use crate::wabbajack::acquire::normalize_nexus_game_domain;

#[derive(Debug, Deserialize)]
struct DownloadLink {
    #[serde(rename = "URI")]
    uri: String,
}

/// Generate a CDN download link for a mod file.
///
/// Requires a Nexus Premium account.
pub async fn generate_download_link(
    client: &Client,
    api_key: &str,
    game_domain: &str,
    mod_id: NexusModId,
    file_id: NexusFileId,
) -> SourceResult<String> {
    // Verify premium status
    let is_premium = auth::check_premium_source(client, api_key).await?;
    if !is_premium {
        return Err(SourceError::other(anyhow::anyhow!(
            "Nexus Premium is required for automated downloads. \
             Please upgrade at https://next.nexusmods.com/premium"
        )));
    }

    let game_domain = normalize_nexus_game_domain(game_domain);
    let url = format!(
        "{}/games/{game_domain}/mods/{mod_id}/files/{file_id}/download_link.json",
        super::base_url()
    );

    let links: Vec<DownloadLink> =
        status_error(client.get(&url).header("apikey", api_key).send().await?)?
            .json()
            .await?;

    links
        .into_iter()
        .next()
        .map(|l| l.uri)
        .ok_or_else(|| SourceError::other(anyhow::anyhow!("no download links returned")))
}
