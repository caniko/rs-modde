//! Nexus API-key acquisition and storage: resolving a key from the configured
//! sources (`OAuth`, config file, environment, system keyring) and validating
//! it against the Nexus API.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use keyring_core::{Entry, Error as KeyringError};
use modde_core::paths;
use modde_core::settings::AppSettings;
use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, info, warn};

use crate::error::{SourceResult, status_error};

const KEYRING_SERVICE: &str = "modde";
const KEYRING_KEY: &str = "nexus-api-key";

#[derive(Debug, Deserialize)]
struct ValidateResponse {
    #[serde(default)]
    is_premium: bool,
    name: Option<String>,
}

/// Where a resolved Nexus API key was loaded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiKeySource {
    OAuth,
    ModdeConfigFile,
    Environment,
    Keyring,
    EnvironmentFile,
    LegacySettingsToml,
}

impl ApiKeySource {
    /// A short human-readable label naming this key source.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::OAuth => "OAuth token",
            Self::ModdeConfigFile => "modde config file",
            Self::Environment => "NEXUS_API_KEY",
            Self::Keyring => "system keyring",
            Self::EnvironmentFile => "NEXUS_API_KEY_FILE",
            Self::LegacySettingsToml => "legacy settings.toml",
        }
    }
}

/// A resolved Nexus API key together with the source it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedApiKey {
    pub key: String,
    pub source: ApiKeySource,
}

/// Store the Nexus API key in the system keyring.
pub fn store_api_key(api_key: &str) -> Result<()> {
    let entry = keyring_entry(KEYRING_SERVICE, KEYRING_KEY)?;
    entry
        .set_password(api_key)
        .context("failed to store API key in keyring")?;
    info!("Nexus API key stored in system keyring");
    Ok(())
}

/// Delete the Nexus API key from the system keyring.
pub fn delete_api_key() -> Result<()> {
    let entry = keyring_entry(KEYRING_SERVICE, KEYRING_KEY)?;
    entry
        .delete_credential()
        .context("failed to delete API key from keyring")?;
    info!("Nexus API key removed from system keyring");
    Ok(())
}

/// Retrieve the API key from the system keyring, returning `None` if unavailable.
fn load_from_keyring() -> Option<String> {
    let entry = match keyring_entry(KEYRING_SERVICE, KEYRING_KEY) {
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
        Err(KeyringError::NoEntry) => None,
        Err(e) => {
            warn!("failed to read from keyring: {e}");
            None
        }
    }
}

fn keyring_entry(service: &str, key: &str) -> Result<Entry> {
    keyring::use_native_store(false).context("failed to initialize system keyring store")?;
    Entry::new(service, key).context("failed to create keyring entry")
}

/// Load Nexus API key from the configured auth sources.
///
/// Lookup chain:
/// 1. OAuth token
/// 2. `modde nexus auth` config file
/// 3. `NEXUS_API_KEY` environment variable
/// 4. System keyring (secret-service D-Bus)
/// 5. `NEXUS_API_KEY_FILE` file path (sops-nix compatible)
/// 6. Legacy `settings.toml` key
pub fn load_api_key() -> Result<String> {
    load_api_key_with_source().map(|loaded| loaded.key)
}

/// Load Nexus API key and report which source supplied it.
pub fn load_api_key_with_source() -> Result<LoadedApiKey> {
    if let Some(token) = super::oauth::load_token() {
        if !token.is_expired() {
            debug!("using OAuth token for Nexus authentication");
            return Ok(LoadedApiKey {
                key: token.access_token,
                source: ApiKeySource::OAuth,
            });
        }
        debug!("OAuth token expired, falling back to API key");
    }

    resolve_api_key_from_sources(
        &config_api_key_path(),
        std::env::var("NEXUS_API_KEY").ok(),
        load_from_keyring(),
        std::env::var("NEXUS_API_KEY_FILE").ok().map(PathBuf::from),
        AppSettings::load().nexus_api_key,
    )
}

/// Path to the API key file written by `modde nexus auth`.
#[must_use]
pub fn config_api_key_path() -> std::path::PathBuf {
    paths::modde_config_dir().join("nexus_api_key")
}

/// Does the modde-owned API key file exist?
#[must_use]
pub fn config_api_key_exists() -> bool {
    config_api_key_path().exists()
}

/// Write the Nexus API key to modde's own config file.
pub fn write_config_api_key(api_key: &str) -> Result<()> {
    write_config_api_key_to(&config_api_key_path(), api_key)
}

fn write_config_api_key_to(path: &Path, api_key: &str) -> Result<()> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        bail!("API key cannot be empty");
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(path, api_key).with_context(|| {
        format!(
            "failed to write Nexus API key to modde config file {}",
            path.display()
        )
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to restrict permissions on {}", path.display()))?;
    }

    Ok(())
}

/// Delete only modde's own Nexus API key file.
pub fn delete_config_api_key() -> Result<()> {
    let path = config_api_key_path();
    if let Err(e) = std::fs::remove_file(&path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        return Err(e).with_context(|| format!("failed to delete {}", path.display()));
    }
    Ok(())
}

fn load_from_config_file(path: &std::path::Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    let key = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read API key from {}", path.display()))?
        .trim()
        .to_string();
    if key.is_empty() {
        Ok(None)
    } else {
        Ok(Some(key))
    }
}

fn resolve_api_key_from_sources(
    config_path: &Path,
    env_key: Option<String>,
    keyring_key: Option<String>,
    env_file_path: Option<PathBuf>,
    legacy_settings_key: String,
) -> Result<LoadedApiKey> {
    if let Some(key) = load_from_config_file(config_path)? {
        return Ok(LoadedApiKey {
            key,
            source: ApiKeySource::ModdeConfigFile,
        });
    }

    if let Some(key) = env_key.map(|key| key.trim().to_string())
        && !key.is_empty()
    {
        return Ok(LoadedApiKey {
            key,
            source: ApiKeySource::Environment,
        });
    }

    if let Some(key) = keyring_key.map(|key| key.trim().to_string())
        && !key.is_empty()
    {
        return Ok(LoadedApiKey {
            key,
            source: ApiKeySource::Keyring,
        });
    }

    if let Some(path) = env_file_path {
        let key = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read API key from {}", path.display()))?
            .trim()
            .to_string();
        if !key.is_empty() {
            return Ok(LoadedApiKey {
                key,
                source: ApiKeySource::EnvironmentFile,
            });
        }
    }

    let legacy_settings_key = legacy_settings_key.trim().to_string();
    if !legacy_settings_key.is_empty() {
        return Ok(LoadedApiKey {
            key: legacy_settings_key,
            source: ApiKeySource::LegacySettingsToml,
        });
    }

    bail!("No Nexus API key found. Set NEXUS_API_KEY env var or run `modde nexus auth`.")
}

/// Check if the given API key belongs to a premium account.
pub async fn check_premium(client: &Client, api_key: &str) -> Result<bool> {
    Ok(check_premium_source(client, api_key).await?)
}

/// Check premium status, preserving typed HTTP failures for download sources.
pub async fn check_premium_source(client: &Client, api_key: &str) -> SourceResult<bool> {
    let validate_url = format!("{}/users/validate.json", super::base_url());
    let resp: ValidateResponse = status_error(
        client
            .get(&validate_url)
            .header("apikey", api_key)
            .send()
            .await?,
    )?
    .json()
    .await?;

    info!(
        user = resp.name.as_deref().unwrap_or("unknown"),
        premium = resp.is_premium,
        "Nexus account validated"
    );

    Ok(resp.is_premium)
}

#[cfg(test)]
mod tests {
    use super::{
        ApiKeySource, load_from_config_file, resolve_api_key_from_sources, write_config_api_key_to,
    };
    use std::path::PathBuf;

    #[test]
    fn load_from_config_file_trims_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nexus_api_key");
        std::fs::write(&path, "  test-key\n").unwrap();

        let key = load_from_config_file(&path).unwrap();
        assert_eq!(key.as_deref(), Some("test-key"));
    }

    #[test]
    fn load_from_config_file_ignores_missing_or_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        assert!(load_from_config_file(&missing).unwrap().is_none());

        let empty = dir.path().join("nexus_api_key");
        std::fs::write(&empty, "\n").unwrap();
        assert!(load_from_config_file(&empty).unwrap().is_none());
    }

    #[test]
    fn config_file_overrides_environment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nexus_api_key");
        std::fs::write(&path, " config-key \n").unwrap();

        let loaded = resolve_api_key_from_sources(
            &path,
            Some("env-key".to_string()),
            None,
            None,
            String::new(),
        )
        .unwrap();

        assert_eq!(loaded.key, "config-key");
        assert_eq!(loaded.source, ApiKeySource::ModdeConfigFile);
    }

    #[test]
    fn missing_config_file_falls_back_to_environment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nexus_api_key");

        let loaded = resolve_api_key_from_sources(
            &path,
            Some("env-key".to_string()),
            None,
            None,
            String::new(),
        )
        .unwrap();

        assert_eq!(loaded.key, "env-key");
        assert_eq!(loaded.source, ApiKeySource::Environment);
    }

    #[test]
    fn replacement_writes_only_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nexus_api_key");

        write_config_api_key_to(&path, " replacement-key \n").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement-key");
        let loaded = resolve_api_key_from_sources(
            &path,
            Some("env-key".to_string()),
            None,
            None,
            String::new(),
        )
        .unwrap();

        assert_eq!(loaded.key, "replacement-key");
        assert_eq!(loaded.source, ApiKeySource::ModdeConfigFile);
    }

    #[test]
    fn deleting_config_file_falls_back_to_environment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nexus_api_key");
        write_config_api_key_to(&path, "config-key").unwrap();
        std::fs::remove_file(&path).unwrap();

        let loaded = resolve_api_key_from_sources(
            &path,
            Some("env-key".to_string()),
            None,
            None,
            String::new(),
        )
        .unwrap();

        assert_eq!(loaded.key, "env-key");
        assert_eq!(loaded.source, ApiKeySource::Environment);
    }

    #[test]
    fn environment_file_precedes_legacy_settings() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("nexus_api_key");
        let env_file = dir.path().join("env-key");
        std::fs::write(&env_file, "file-key").unwrap();

        let loaded = resolve_api_key_from_sources(
            &config_path,
            None,
            None,
            Some(PathBuf::from(&env_file)),
            "legacy-key".to_string(),
        )
        .unwrap();

        assert_eq!(loaded.key, "file-key");
        assert_eq!(loaded.source, ApiKeySource::EnvironmentFile);
    }
}
