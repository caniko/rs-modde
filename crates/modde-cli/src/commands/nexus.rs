use std::io::Write;

use anyhow::{Context, Result};
use tracing::info;

use modde_core::paths;
use modde_sources::nexus::auth;

use crate::NexusAction;

pub async fn handle(action: NexusAction) -> Result<()> {
    match action {
        NexusAction::Auth => handle_auth().await,
        NexusAction::Status => handle_status().await,
    }
}

/// Build a reqwest client with Nexus-appropriate timeouts.
fn nexus_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_mins(5))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client")
}

/// Map Nexus API errors to user-friendly messages with actionable advice.
fn map_nexus_error(e: anyhow::Error, rejected_advice: &str) -> anyhow::Error {
    let msg = e.to_string();
    if msg.contains("401") || msg.contains("403") {
        anyhow::anyhow!(
            "API key rejected by Nexus (HTTP 401/403). {rejected_advice}\n\
             Cause: {e}"
        )
    } else if msg.contains("429") {
        anyhow::anyhow!(
            "Nexus API rate limit exceeded (HTTP 429). Wait a few minutes and try again.\n\
             Cause: {e}"
        )
    } else {
        anyhow::anyhow!("API key validation failed: {e}")
    }
}

async fn handle_auth() -> Result<()> {
    // Prompt user for API key
    print!("Enter your Nexus API key: ");
    std::io::stdout().flush()?;

    let mut api_key = String::new();
    std::io::stdin()
        .read_line(&mut api_key)
        .context("failed to read API key from stdin")?;
    let api_key = api_key.trim().to_string();

    if api_key.is_empty() {
        anyhow::bail!("API key cannot be empty");
    }

    let client = nexus_client()?;
    let is_premium = auth::check_premium(&client, &api_key).await.map_err(|e| {
        map_nexus_error(
            e,
            "Verify your key at https://www.nexusmods.com/users/myaccount?tab=api+access",
        )
    })?;

    // Store the key at XDG config path
    let key_path = config_key_path();
    if let Some(parent) = key_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&key_path, &api_key)
        .await
        .context("failed to write API key to config")?;

    // Restrict file permissions to owner-only
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
    }

    info!(key_path = %key_path.display(), "API key stored");

    println!("API key validated and saved to {}", key_path.display());
    if is_premium {
        println!("Account type: Premium (automated downloads enabled)");
    } else {
        println!("Account type: Free (manual download links only)");
    }

    Ok(())
}

async fn handle_status() -> Result<()> {
    let api_key =
        auth::load_api_key().context("no API key configured; run `modde nexus auth` first")?;

    let client = nexus_client()?;
    let is_premium = auth::check_premium(&client, &api_key).await.map_err(|e| {
        map_nexus_error(
            e,
            "Your stored key may have been revoked. Re-run `modde nexus auth` to set a new key.",
        )
    })?;

    println!("Nexus API key: valid");
    if is_premium {
        println!("Account type: Premium");
    } else {
        println!("Account type: Free");
    }

    Ok(())
}

/// Path to store the API key: `~/.config/modde/nexus_api_key`.
fn config_key_path() -> std::path::PathBuf {
    paths::modde_config_dir().join("nexus_api_key")
}
