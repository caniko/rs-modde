use anyhow::{Result, bail};
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
}

#[derive(Debug, Deserialize)]
pub struct NexusModFiles {
    pub files: Vec<NexusModFile>,
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
}
