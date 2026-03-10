use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::Deserialize;
use tracing::info;

const VALIDATE_URL: &str = "https://api.nexusmods.com/v1/users/validate.json";

#[derive(Debug, Deserialize)]
struct ValidateResponse {
    #[serde(default)]
    is_premium: bool,
    name: Option<String>,
}

/// Load Nexus API key from environment or secret-service keyring.
pub fn load_api_key() -> Result<String> {
    // Try environment variable first
    if let Ok(key) = std::env::var("NEXUS_API_KEY") {
        if !key.is_empty() {
            return Ok(key);
        }
    }

    // Try reading from file path (sops-nix compatible)
    if let Ok(path) = std::env::var("NEXUS_API_KEY_FILE") {
        let key = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read API key from {path}"))?
            .trim()
            .to_string();
        if !key.is_empty() {
            return Ok(key);
        }
    }

    // TODO: secret-service D-Bus keyring integration
    bail!("No Nexus API key found. Set NEXUS_API_KEY env var or run `modde nexus auth`.")
}

/// Check if the given API key belongs to a premium account.
pub async fn check_premium(client: &Client, api_key: &str) -> Result<bool> {
    let resp: ValidateResponse = client
        .get(VALIDATE_URL)
        .header("apikey", api_key)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    info!(
        user = resp.name.as_deref().unwrap_or("unknown"),
        premium = resp.is_premium,
        "Nexus account validated"
    );

    Ok(resp.is_premium)
}
