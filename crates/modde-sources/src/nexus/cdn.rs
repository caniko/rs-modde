use anyhow::{Result, bail};
use reqwest::Client;
use serde::Deserialize;

use super::auth;

const BASE_URL: &str = "https://api.nexusmods.com/v1";

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
    mod_id: u64,
    file_id: u64,
) -> Result<String> {
    // Verify premium status
    let is_premium = auth::check_premium(client, api_key).await?;
    if !is_premium {
        bail!(
            "Nexus Premium is required for automated downloads. \
             Please upgrade at https://next.nexusmods.com/premium"
        );
    }

    let url =
        format!("{BASE_URL}/games/{game_domain}/mods/{mod_id}/files/{file_id}/download_link.json");

    let links: Vec<DownloadLink> = client
        .get(&url)
        .header("apikey", api_key)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    links
        .into_iter()
        .next()
        .map(|l| l.uri)
        .ok_or_else(|| anyhow::anyhow!("no download links returned"))
}
