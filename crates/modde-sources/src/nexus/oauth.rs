use anyhow::Result;
use keyring_core::Entry;
use tracing::info;

/// Nexus `OAuth2` application credentials.
/// These would be registered at <https://www.nexusmods.com/oauth/applications>
const CLIENT_ID: &str = "modde";
const AUTH_URL: &str = "https://users.nexusmods.com/oauth/authorize";
const TOKEN_URL: &str = "https://users.nexusmods.com/oauth/token";
const REDIRECT_URI: &str = "http://localhost:8024/callback";

/// `OAuth2` token pair.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>, // Unix timestamp
    pub token_type: String,
}

impl OAuthToken {
    /// Check if the token has expired (with 60-second buffer).
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|exp| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            now + 60 >= exp
        })
    }
}

/// Generate a PKCE code verifier and challenge.
#[must_use]
pub fn generate_pkce() -> (String, String) {
    use base64::Engine;
    use sha2::{Digest, Sha256};

    // Generate random verifier (43-128 chars, A-Z, a-z, 0-9, -._~)
    let verifier: String = (0..64)
        .map(|_| {
            let idx = rand_byte() % 66;
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~"[idx as usize]
                as char
        })
        .collect();

    // S256 challenge
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash);

    (verifier, challenge)
}

fn rand_byte() -> u8 {
    // Simple entropy from system time for PKCE
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (t.subsec_nanos() as u8)
        .wrapping_mul(37)
        .wrapping_add(t.as_secs() as u8)
}

/// Build the authorization URL that the user opens in their browser.
#[must_use]
pub fn authorization_url(challenge: &str, state: &str) -> String {
    format!(
        "{AUTH_URL}?client_id={CLIENT_ID}&redirect_uri={}&response_type=code&scope=public&code_challenge={challenge}&code_challenge_method=S256&state={state}",
        urlencoding_simple(REDIRECT_URI),
    )
}

/// Exchange an authorization code for tokens.
pub async fn exchange_code(
    client: &reqwest::Client,
    code: &str,
    verifier: &str,
) -> Result<OAuthToken> {
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("redirect_uri", REDIRECT_URI),
            ("code", code),
            ("code_verifier", verifier),
        ])
        .send()
        .await?
        .error_for_status()?;

    let token: OAuthToken = resp.json().await?;
    info!("OAuth2 token obtained");
    Ok(token)
}

/// Refresh an expired token.
pub async fn refresh_token(client: &reqwest::Client, refresh: &str) -> Result<OAuthToken> {
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh),
        ])
        .send()
        .await?
        .error_for_status()?;

    let token: OAuthToken = resp.json().await?;
    info!("OAuth2 token refreshed");
    Ok(token)
}

/// Store OAuth token in keyring.
pub fn store_token(token: &OAuthToken) -> Result<()> {
    let json = serde_json::to_string(token)?;
    let entry = keyring_entry()?;
    entry.set_password(&json)?;
    Ok(())
}

/// Load OAuth token from keyring.
#[must_use]
pub fn load_token() -> Option<OAuthToken> {
    let entry = keyring_entry().ok()?;
    let json = entry.get_password().ok()?;
    serde_json::from_str(&json).ok()
}

/// Delete stored token.
pub fn delete_token() -> Result<()> {
    let entry = keyring_entry()?;
    entry.delete_credential()?;
    Ok(())
}

fn keyring_entry() -> Result<Entry> {
    keyring::use_native_store(false)?;
    Ok(Entry::new("modde", "nexus-oauth-token")?)
}

fn urlencoding_simple(s: &str) -> String {
    s.replace(':', "%3A").replace('/', "%2F")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_generation() {
        let (verifier, challenge) = generate_pkce();
        assert!(!verifier.is_empty(), "verifier should not be empty");
        assert!(!challenge.is_empty(), "challenge should not be empty");
        assert_ne!(verifier, challenge, "verifier and challenge should differ");
        assert_eq!(verifier.len(), 64, "verifier should be 64 chars");
    }

    #[test]
    fn test_token_expiry() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Token that expired in the past
        let expired = OAuthToken {
            access_token: "test".into(),
            refresh_token: None,
            expires_at: Some(now - 120),
            token_type: "Bearer".into(),
        };
        assert!(expired.is_expired(), "past token should be expired");

        // Token that expires far in the future
        let fresh = OAuthToken {
            access_token: "test".into(),
            refresh_token: None,
            expires_at: Some(now + 3600),
            token_type: "Bearer".into(),
        };
        assert!(!fresh.is_expired(), "future token should not be expired");

        // Token with no expiry
        let no_expiry = OAuthToken {
            access_token: "test".into(),
            refresh_token: None,
            expires_at: None,
            token_type: "Bearer".into(),
        };
        assert!(
            !no_expiry.is_expired(),
            "token with no expiry should not be expired"
        );
    }

    #[test]
    fn test_authorization_url() {
        let url = authorization_url("test_challenge", "test_state");
        assert!(url.contains("client_id=modde"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge=test_challenge"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=test_state"));
        assert!(url.contains("scope=public"));
        assert!(url.starts_with("https://users.nexusmods.com/oauth/authorize"));
    }
}
