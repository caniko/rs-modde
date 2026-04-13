use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::Deserialize;
use tracing::{info, debug, warn};

const VALIDATE_URL: &str = "https://api.nexusmods.com/v1/users/validate.json";
const KEYRING_SERVICE: &str = "modde";
const KEYRING_KEY: &str = "nexus-api-key";

#[derive(Debug, Deserialize)]
struct ValidateResponse {
    #[serde(default)]
    is_premium: bool,
    name: Option<String>,
}

/// Store the Nexus API key in the system keyring.
pub fn store_api_key(api_key: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_KEY)
        .context("failed to create keyring entry")?;
    entry
        .set_password(api_key)
        .context("failed to store API key in keyring")?;
    info!("Nexus API key stored in system keyring");
    Ok(())
}

/// Delete the Nexus API key from the system keyring.
pub fn delete_api_key() -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_KEY)
        .context("failed to create keyring entry")?;
    entry
        .delete_credential()
        .context("failed to delete API key from keyring")?;
    info!("Nexus API key removed from system keyring");
    Ok(())
}

/// Retrieve the API key from the system keyring, returning `None` if unavailable.
fn load_from_keyring() -> Option<String> {
    let entry = match keyring::Entry::new(KEYRING_SERVICE, KEYRING_KEY) {
        Ok(e) => e,
        Err(e) => {
            debug!("keyring unavailable: {e}");
            return None;
        }
    };
    match entry.get_password() {
        Ok(key) if !key.is_empty() => {
            debug!("loaded API key from system keyring");
            Some(key)
        }
        Ok(_) => None,
        Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            warn!("failed to read from keyring: {e}");
            None
        }
    }
}

/// Load Nexus API key from environment, keyring, or file fallback.
///
/// Lookup chain:
/// 1. `NEXUS_API_KEY` environment variable
/// 2. System keyring (secret-service D-Bus)
/// 3. `NEXUS_API_KEY_FILE` file path (sops-nix compatible)
pub fn load_api_key() -> Result<String> {
    // 0. Try OAuth token first
    if let Some(token) = super::oauth::load_token() {
        if !token.is_expired() {
            debug!("using OAuth token for Nexus authentication");
            return Ok(token.access_token);
        }
        debug!("OAuth token expired, falling back to API key");
    }

    // 1. Try environment variable first
    if let Ok(key) = std::env::var("NEXUS_API_KEY") {
        if !key.is_empty() {
            return Ok(key);
        }
    }

    // 2. Try system keyring
    if let Some(key) = load_from_keyring() {
        return Ok(key);
    }

    // 3. Try reading from file path (sops-nix compatible)
    if let Ok(path) = std::env::var("NEXUS_API_KEY_FILE") {
        let key = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read API key from {path}"))?
            .trim()
            .to_string();
        if !key.is_empty() {
            return Ok(key);
        }
    }

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
