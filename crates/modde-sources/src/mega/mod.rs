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
mod tests;
