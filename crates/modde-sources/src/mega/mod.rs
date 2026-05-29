//! Mega.nz download source, including client-side AES-128-CTR decryption of
//! the encrypted payload.

use std::collections::HashMap;
use std::path::Path;

use aes::Aes128;
use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ctr::Ctr128BE;
use ctr::cipher::{KeyIvInit, StreamCipher};
use futures::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tracing::debug;

use modde_core::manifest::wabbajack::DownloadDirective;

use crate::common::{ensure_parent, verify_and_wrap};
use crate::error::{SourceError, SourceResult, status_error};
use crate::traits::{DownloadHandle, DownloadSource, ProgressCallback, VerifiedFile};

const MEGA_API_URL: &str = "https://g.api.mega.co.nz/cs";

/// Mega.nz download source.
///
/// Handles Mega's client-side AES-128-CTR decryption protocol.
pub struct MegaSource {
    client: Client,
}

#[derive(Debug, Deserialize)]
struct MegaFileResponse {
    /// Download URL
    g: String,
    /// File size
    s: u64,
}

/// Parse a Mega URL to extract the file handle and key.
/// Supports both new format `/file/HANDLE#KEY` and old format `/#!HANDLE!KEY`.
fn parse_mega_url(url: &str) -> Result<(String, String)> {
    // New format: https://mega.nz/file/HANDLE#KEY
    if let Some(rest) = url
        .strip_prefix("https://mega.nz/file/")
        .or_else(|| url.strip_prefix("http://mega.nz/file/"))
    {
        let parts: Vec<&str> = rest.splitn(2, '#').collect();
        if parts.len() == 2 {
            return Ok((parts[0].to_string(), parts[1].to_string()));
        }
    }

    // Old format: https://mega.nz/#!HANDLE!KEY
    if let Some(rest) = url.find("#!") {
        let fragment = &url[rest + 2..];
        let parts: Vec<&str> = fragment.splitn(2, '!').collect();
        if parts.len() == 2 {
            return Ok((parts[0].to_string(), parts[1].to_string()));
        }
    }

    bail!("invalid Mega URL format: {url}")
}

/// Decode a Mega key from base64url, XOR first/second halves for AES-128 key, extract IV.
fn decode_mega_key(key_b64: &str) -> Result<([u8; 16], [u8; 16])> {
    let key_bytes = URL_SAFE_NO_PAD
        .decode(key_b64)
        .context("failed to decode Mega key from base64url")?;

    if key_bytes.len() != 32 {
        bail!("expected 32-byte Mega key, got {} bytes", key_bytes.len());
    }

    // XOR first 16 bytes with second 16 bytes to get AES key
    let mut aes_key = [0u8; 16];
    for i in 0..16 {
        aes_key[i] = key_bytes[i] ^ key_bytes[i + 16];
    }

    // IV is bytes 16..24, zero-padded to 16 bytes (counter starts at 0)
    let mut iv = [0u8; 16];
    iv[..8].copy_from_slice(&key_bytes[16..24]);
    // bytes 8..16 of IV are zero (counter)

    Ok((aes_key, iv))
}

impl MegaSource {
    /// Create a source that downloads over the given HTTP `client`.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl DownloadSource for MegaSource {
    fn can_handle(&self, directive: &DownloadDirective) -> bool {
        matches!(directive, DownloadDirective::Mega { .. })
    }

    async fn resolve(&self, directive: &DownloadDirective) -> SourceResult<DownloadHandle> {
        let DownloadDirective::Mega { url, hash } = directive else {
            return Err(SourceError::other(anyhow::anyhow!("not a Mega directive")));
        };

        let (handle_id, key_b64) = parse_mega_url(url).map_err(SourceError::other)?;

        // Call Mega API to get download URL
        let api_url = format!("{MEGA_API_URL}?id=0");
        let payload = serde_json::json!([{"a": "g", "g": 1, "p": handle_id}]);

        let resp = status_error(self.client.post(&api_url).json(&payload).send().await?)?;

        let body: Vec<MegaFileResponse> = resp.json().await?;
        let file_info = body
            .into_iter()
            .next()
            .ok_or_else(|| SourceError::other(anyhow::anyhow!("empty response from Mega API")))?;

        debug!(download_url = %file_info.g, size = file_info.s, "resolved Mega download URL");

        let mut headers = HashMap::new();
        headers.insert("x-mega-key".to_string(), key_b64);

        Ok(DownloadHandle {
            url: file_info.g,
            candidate_urls: Vec::new(),
            headers,
            expected_hash: *hash,
            size_hint: Some(file_info.s),
        })
    }

    async fn download_with_progress(
        &self,
        handle: DownloadHandle,
        dest: &Path,
        progress: ProgressCallback,
    ) -> SourceResult<VerifiedFile> {
        ensure_parent(dest).await?;

        let key_b64 = handle
            .headers
            .get("x-mega-key")
            .ok_or_else(|| {
                SourceError::other(anyhow::anyhow!(
                    "missing x-mega-key header in download handle"
                ))
            })?
            .clone();

        let (aes_key, iv) = decode_mega_key(&key_b64).map_err(SourceError::other)?;

        let resp = status_error(self.client.get(&handle.url).send().await?)?;

        let total = resp.content_length().or(handle.size_hint).unwrap_or(0);
        let mut file = tokio::fs::File::create(dest).await?;
        let mut downloaded: u64 = 0;

        let mut cipher = Ctr128BE::<Aes128>::new(&aes_key.into(), &iv.into());

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let mut chunk = chunk?.to_vec();
            cipher.apply_keystream(&mut chunk);
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            progress(downloaded, total);
        }

        file.flush().await?;
        debug!(bytes = downloaded, "Mega download complete");

        verify_and_wrap(dest, handle.expected_hash).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use modde_core::GameId;

    // ── parse_mega_url: new format /file/HANDLE#KEY ──────────────────────

    #[test]
    fn parse_new_format_https() {
        let (handle, key) = parse_mega_url("https://mega.nz/file/ABC123#some_key_base64").unwrap();
        assert_eq!(handle, "ABC123");
        assert_eq!(key, "some_key_base64");
    }

    #[test]
    fn parse_new_format_http() {
        let (handle, key) = parse_mega_url("http://mega.nz/file/XYZ789#another_key").unwrap();
        assert_eq!(handle, "XYZ789");
        assert_eq!(key, "another_key");
    }

    #[test]
    fn parse_new_format_long_handle_and_key() {
        let (handle, key) = parse_mega_url(
            "https://mega.nz/file/AbCdEfGhIjKlMnOp#AAAAAAAAAAAABBBBBBBBBBBBCCCCCCCCCCCCDDDDDDDDDDDD",
        )
        .unwrap();
        assert_eq!(handle, "AbCdEfGhIjKlMnOp");
        assert_eq!(key, "AAAAAAAAAAAABBBBBBBBBBBBCCCCCCCCCCCCDDDDDDDDDDDD");
    }

    #[test]
    fn parse_new_format_key_with_special_base64url_chars() {
        // base64url uses - and _ instead of + and /
        let (handle, key) =
            parse_mega_url("https://mega.nz/file/HANDLE#a-b_c-d_e-f_g-h_i-j_k").unwrap();
        assert_eq!(handle, "HANDLE");
        assert_eq!(key, "a-b_c-d_e-f_g-h_i-j_k");
    }

    // ── parse_mega_url: old format /#!HANDLE!KEY ─────────────────────────

    #[test]
    fn parse_old_format_https() {
        let (handle, key) = parse_mega_url("https://mega.nz/#!ABC123!some_key_base64").unwrap();
        assert_eq!(handle, "ABC123");
        assert_eq!(key, "some_key_base64");
    }

    #[test]
    fn parse_old_format_http() {
        let (handle, key) = parse_mega_url("http://mega.nz/#!OldHandle!OldKey123").unwrap();
        assert_eq!(handle, "OldHandle");
        assert_eq!(key, "OldKey123");
    }

    #[test]
    fn parse_old_format_with_extra_prefix() {
        // The old-format parser uses find("#!"), so it works even with odd prefixes
        let (handle, key) = parse_mega_url("https://mega.co.nz/#!HANDLE!KEY").unwrap();
        assert_eq!(handle, "HANDLE");
        assert_eq!(key, "KEY");
    }

    // ── parse_mega_url: invalid URLs ─────────────────────────────────────

    #[test]
    fn parse_url_no_hash_new_format() {
        // Missing # separator in new format
        assert!(parse_mega_url("https://mega.nz/file/ABCnohash").is_err());
    }

    #[test]
    fn parse_url_random_url() {
        assert!(parse_mega_url("https://example.com/file").is_err());
    }

    #[test]
    fn parse_url_empty_string() {
        assert!(parse_mega_url("").is_err());
    }

    #[test]
    fn parse_url_only_domain() {
        assert!(parse_mega_url("https://mega.nz").is_err());
    }

    #[test]
    fn parse_url_no_key_after_hash() {
        // "splitn(2, '#')" produces ["HANDLE", ""] – length 2 but empty key
        // The function does not reject empty keys, so this succeeds with empty key
        let result = parse_mega_url("https://mega.nz/file/HANDLE#");
        // Regardless of success/failure, document behaviour
        if let Ok((_handle, key)) = &result {
            assert!(key.is_empty());
        }
    }

    #[test]
    fn parse_url_with_query_params_new_format() {
        // Query params end up as part of the key (since we only split on #)
        let (handle, key) = parse_mega_url("https://mega.nz/file/HANDLE#KEY?foo=bar").unwrap();
        assert_eq!(handle, "HANDLE");
        assert_eq!(key, "KEY?foo=bar");
    }

    #[test]
    fn parse_url_with_extra_path_segments() {
        // "/file/" prefix is stripped, then everything up to # is handle
        let (handle, key) = parse_mega_url("https://mega.nz/file/HANDLE/extra#KEY").unwrap();
        assert_eq!(handle, "HANDLE/extra");
        assert_eq!(key, "KEY");
    }

    // ── decode_mega_key: valid 32-byte keys ──────────────────────────────

    #[test]
    fn decode_key_valid_32_bytes() {
        // 32 zero bytes -> base64url = AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=
        // URL_SAFE_NO_PAD expects no padding, so use the unpadded form
        let key_bytes = [0u8; 32];
        let key_b64 = URL_SAFE_NO_PAD.encode(key_bytes);
        let (aes_key, iv) = decode_mega_key(&key_b64).unwrap();
        // 0 XOR 0 = 0 for all bytes
        assert_eq!(aes_key, [0u8; 16]);
        // IV = bytes 16..24 of original (all zero), zero-padded
        assert_eq!(iv, [0u8; 16]);
    }

    #[test]
    fn decode_key_xor_logic() {
        // Construct a 32-byte key where first half = [1..=16], second half = [17..=32]
        let mut key_bytes = [0u8; 32];
        for i in 0..16 {
            key_bytes[i] = (i + 1) as u8;
            key_bytes[i + 16] = (i + 17) as u8;
        }
        let key_b64 = URL_SAFE_NO_PAD.encode(key_bytes);
        let (aes_key, iv) = decode_mega_key(&key_b64).unwrap();

        // Verify XOR: aes_key[i] = key_bytes[i] ^ key_bytes[i+16]
        for i in 0..16 {
            assert_eq!(
                aes_key[i],
                key_bytes[i] ^ key_bytes[i + 16],
                "XOR mismatch at index {i}"
            );
        }

        // Verify IV: bytes 16..24 of original, zero-padded to 16
        let mut expected_iv = [0u8; 16];
        expected_iv[..8].copy_from_slice(&key_bytes[16..24]);
        assert_eq!(iv, expected_iv);
    }

    #[test]
    fn decode_key_xor_inverse() {
        // If first half == second half, XOR yields all zeros
        let mut key_bytes = [0u8; 32];
        for byte in key_bytes.iter_mut().take(16) {
            *byte = 0xAB;
        }
        for byte in key_bytes.iter_mut().skip(16) {
            *byte = 0xAB;
        }
        let key_b64 = URL_SAFE_NO_PAD.encode(key_bytes);
        let (aes_key, _iv) = decode_mega_key(&key_b64).unwrap();
        assert_eq!(aes_key, [0u8; 16]);
    }

    #[test]
    fn decode_key_xor_all_ones() {
        // first half = 0xFF, second half = 0x00 -> XOR = 0xFF
        let mut key_bytes = [0u8; 32];
        for byte in key_bytes.iter_mut().take(16) {
            *byte = 0xFF;
        }
        let key_b64 = URL_SAFE_NO_PAD.encode(key_bytes);
        let (aes_key, _iv) = decode_mega_key(&key_b64).unwrap();
        assert_eq!(aes_key, [0xFF; 16]);
    }

    // ── decode_mega_key: IV extraction correctness ───────────────────────

    #[test]
    fn decode_key_iv_extraction() {
        let mut key_bytes = [0u8; 32];
        // Set bytes 16..24 to distinct values
        for i in 0..8 {
            key_bytes[16 + i] = (0x10 + i) as u8;
        }
        // Set bytes 24..32 to something else (should NOT appear in IV)
        for i in 0..8 {
            key_bytes[24 + i] = 0xFF;
        }
        let key_b64 = URL_SAFE_NO_PAD.encode(key_bytes);
        let (_aes_key, iv) = decode_mega_key(&key_b64).unwrap();

        // First 8 bytes of IV = bytes 16..24 of original
        for (i, byte) in iv.iter().enumerate().take(8) {
            assert_eq!(*byte, (0x10 + i) as u8, "IV byte {i} mismatch");
        }
        // Last 8 bytes of IV must be zero (counter)
        for (i, byte) in iv.iter().enumerate().skip(8) {
            assert_eq!(*byte, 0, "IV counter byte {i} should be zero");
        }
    }

    // ── decode_mega_key: invalid inputs ──────────────────────────────────

    #[test]
    fn decode_key_too_short() {
        let short = URL_SAFE_NO_PAD.encode([0u8; 16]);
        let err = decode_mega_key(&short).unwrap_err();
        assert!(
            err.to_string().contains("expected 32-byte"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn decode_key_too_long() {
        let long = URL_SAFE_NO_PAD.encode([0u8; 48]);
        let err = decode_mega_key(&long).unwrap_err();
        assert!(
            err.to_string().contains("expected 32-byte"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn decode_key_empty() {
        let err = decode_mega_key("").unwrap_err();
        assert!(
            err.to_string().contains("expected 32-byte"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn decode_key_invalid_base64() {
        let err = decode_mega_key("!!!not-valid-base64!!!").unwrap_err();
        assert!(
            err.to_string().contains("base64"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn decode_key_one_byte() {
        let one = URL_SAFE_NO_PAD.encode([0x42u8; 1]);
        let err = decode_mega_key(&one).unwrap_err();
        assert!(err.to_string().contains("expected 32-byte"));
    }

    #[test]
    fn decode_key_31_bytes() {
        let data = URL_SAFE_NO_PAD.encode([0u8; 31]);
        assert!(decode_mega_key(&data).is_err());
    }

    #[test]
    fn decode_key_33_bytes() {
        let data = URL_SAFE_NO_PAD.encode([0u8; 33]);
        assert!(decode_mega_key(&data).is_err());
    }

    // ── decode_mega_key: very long key string ────────────────────────────

    #[test]
    fn decode_key_very_long_base64() {
        // 256 bytes is way too long
        let long = URL_SAFE_NO_PAD.encode([0xABu8; 256]);
        assert!(decode_mega_key(&long).is_err());
    }

    // ── can_handle ───────────────────────────────────────────────────────

    #[test]
    fn can_handle_mega_directive() {
        let source = MegaSource::new(Client::new());
        let directive = DownloadDirective::Mega {
            url: "https://mega.nz/file/ABC#KEY".to_string(),
            hash: 0,
        };
        assert!(source.can_handle(&directive));
    }

    #[test]
    fn can_handle_rejects_nexus() {
        let source = MegaSource::new(Client::new());
        let directive = DownloadDirective::Nexus {
            game_id: GameId::from("skyrim"),
            mod_id: 1.into(),
            file_id: 1.into(),
            hash: 0,
        };
        assert!(!source.can_handle(&directive));
    }

    #[test]
    fn can_handle_rejects_google_drive() {
        let source = MegaSource::new(Client::new());
        let directive = DownloadDirective::GoogleDrive {
            id: "some-id".to_string(),
            hash: 0,
        };
        assert!(!source.can_handle(&directive));
    }

    #[test]
    fn can_handle_rejects_github() {
        let source = MegaSource::new(Client::new());
        let directive = DownloadDirective::GitHub {
            user: "user".to_string(),
            repo: "repo".to_string(),
            tag: "v1".to_string(),
            asset: "file.zip".to_string(),
            hash: 0,
        };
        assert!(!source.can_handle(&directive));
    }

    #[test]
    fn can_handle_rejects_direct_url() {
        let source = MegaSource::new(Client::new());
        let directive = DownloadDirective::DirectURL {
            url: "https://example.com/file.zip".to_string(),
            headers: HashMap::new(),
            mirror_resolver: None,
            hash: 0,
        };
        assert!(!source.can_handle(&directive));
    }
}
