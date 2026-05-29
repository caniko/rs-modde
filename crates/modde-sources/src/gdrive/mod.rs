//! Google Drive download source, including handling of the large-file virus
//! scan confirmation page.

use std::collections::HashMap;
use std::path::Path;

use reqwest::Client;
use tracing::debug;

use modde_core::manifest::wabbajack::DownloadDirective;

use crate::common::{ensure_parent, stream_to_file_verified};
use crate::error::{SourceError, SourceResult, status_error};
use crate::traits::{DownloadHandle, DownloadSource, ProgressCallback, VerifiedFile};

/// Google Drive download source.
///
/// Handles the virus scan warning page for large files.
pub struct GoogleDriveSource {
    client: Client,
}

impl GoogleDriveSource {
    /// Create a source that downloads over the given HTTP `client`.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl DownloadSource for GoogleDriveSource {
    fn can_handle(&self, directive: &DownloadDirective) -> bool {
        matches!(directive, DownloadDirective::GoogleDrive { .. })
    }

    async fn resolve(&self, directive: &DownloadDirective) -> SourceResult<DownloadHandle> {
        let DownloadDirective::GoogleDrive { id, hash } = directive else {
            return Err(SourceError::other(anyhow::anyhow!(
                "not a Google Drive directive"
            )));
        };

        // Modern Google Drive flow: hit the usercontent host directly with confirm=t,
        // which skips the interstitial virus-scan warning entirely. Falling back to
        // the legacy `drive.google.com/uc?...` URL is left to the HTML extraction
        // path inside `do_download` for old links that still serve the warning page.
        let url = format!(
            "https://drive.usercontent.google.com/download?id={id}&export=download&authuser=0&confirm=t"
        );

        Ok(DownloadHandle {
            url,
            candidate_urls: Vec::new(),
            headers: HashMap::new(),
            expected_hash: *hash,
            size_hint: None,
        })
    }

    async fn download_with_progress(
        &self,
        handle: DownloadHandle,
        dest: &Path,
        progress: ProgressCallback,
    ) -> SourceResult<VerifiedFile> {
        ensure_parent(dest).await?;

        do_download(&self.client, &handle, dest, &progress).await
    }
}

async fn do_download(
    client: &Client,
    handle: &DownloadHandle,
    dest: &Path,
    progress: &ProgressCallback,
) -> SourceResult<VerifiedFile> {
    let resp = status_error(client.get(&handle.url).send().await?)?;
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // If Google returns HTML, it's the virus scan warning page
    if content_type.contains("text/html") {
        debug!("got virus scan warning page, extracting confirm token");
        let body = resp.text().await?;
        let confirm_token = extract_confirm_token(&body).ok_or_else(|| {
            SourceError::other(anyhow::anyhow!(
                "failed to extract confirm token from virus scan page"
            ))
        })?;

        let confirmed_url = format!("{}&confirm={confirm_token}", handle.url);
        let resp = status_error(client.get(&confirmed_url).send().await?)?;
        return stream_to_file_verified(
            resp,
            dest,
            handle.expected_hash,
            handle.size_hint.unwrap_or(0),
            progress,
        )
        .await;
    }
    stream_to_file_verified(
        resp,
        dest,
        handle.expected_hash,
        handle.size_hint.unwrap_or(0),
        progress,
    )
    .await
}

/// Extract the confirm token from Google Drive's virus scan warning HTML.
fn extract_confirm_token(html: &str) -> Option<String> {
    // Google uses a form with action containing confirm=TOKEN or an input named confirm
    // Pattern 1: &confirm=TOKEN&
    if let Some(pos) = html.find("confirm=") {
        let rest = &html[pos + 8..];
        let end = rest.find(|c: char| c == '&' || c == '"' || c == '\'' || c.is_whitespace())?;
        let token = &rest[..end];
        if !token.is_empty() {
            return Some(token.to_string());
        }
    }
    // Pattern 2: name="confirm" value="TOKEN"
    if let Some(pos) = html.find("name=\"confirm\"") {
        let rest = &html[pos..];
        if let Some(val_pos) = rest.find("value=\"") {
            let val_rest = &rest[val_pos + 7..];
            let end = val_rest.find('"')?;
            let token = &val_rest[..end];
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    // Pattern 3: id="uc-download-link" with href containing confirm=
    if let Some(pos) = html.find("id=\"uc-download-link\"") {
        let rest = &html[pos..];
        if let Some(href_pos) = rest.find("confirm=") {
            let val_rest = &rest[href_pos + 8..];
            let end = val_rest.find(['&', '"', '\''])?;
            let token = &val_rest[..end];
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use modde_core::GameId;

    // ── extract_confirm_token: Pattern 1 – &confirm=TOKEN& ───────────────

    #[test]
    fn confirm_token_pattern1_ampersand_delimited() {
        let html =
            r#"<a href="https://drive.google.com/uc?id=ID&confirm=t&export=download">Download</a>"#;
        assert_eq!(extract_confirm_token(html), Some("t".to_string()));
    }

    #[test]
    fn confirm_token_pattern1_long_token() {
        let html = r"something confirm=AbCdEfGh1234&rest";
        assert_eq!(
            extract_confirm_token(html),
            Some("AbCdEfGh1234".to_string())
        );
    }

    #[test]
    fn confirm_token_pattern1_quote_delimited() {
        let html = r#"href="https://example.com?confirm=mytoken""#;
        assert_eq!(extract_confirm_token(html), Some("mytoken".to_string()));
    }

    #[test]
    fn confirm_token_pattern1_single_quote_delimited() {
        let html = r"href='https://example.com?confirm=tok123'";
        assert_eq!(extract_confirm_token(html), Some("tok123".to_string()));
    }

    #[test]
    fn confirm_token_pattern1_whitespace_delimited() {
        let html = "url?confirm=TOKEN rest of text";
        assert_eq!(extract_confirm_token(html), Some("TOKEN".to_string()));
    }

    // ── extract_confirm_token: Pattern 2 – name="confirm" value="TOKEN" ──

    #[test]
    fn confirm_token_pattern2_input_field() {
        let html = r#"<input type="hidden" name="confirm" value="SecretVal"><input type="submit">"#;
        assert_eq!(extract_confirm_token(html), Some("SecretVal".to_string()));
    }

    #[test]
    fn confirm_token_pattern2_with_extra_attrs() {
        let html = r#"<input class="foo" name="confirm" id="bar" value="TOKEN42">"#;
        assert_eq!(extract_confirm_token(html), Some("TOKEN42".to_string()));
    }

    // ── extract_confirm_token: Pattern 3 – id="uc-download-link" ─────────

    #[test]
    fn confirm_token_pattern3_uc_download_link() {
        let html = r#"<a id="uc-download-link" href="/uc?export=download&confirm=XyZ123&id=abc">Download anyway</a>"#;
        assert_eq!(extract_confirm_token(html), Some("XyZ123".to_string()));
    }

    #[test]
    fn confirm_token_pattern3_uc_download_link_quote_end() {
        let html = r#"<a id="uc-download-link" href="/uc?export=download&confirm=TOK">"#;
        assert_eq!(extract_confirm_token(html), Some("TOK".to_string()));
    }

    // ── extract_confirm_token: non-matching / edge cases ─────────────────

    #[test]
    fn confirm_token_no_match_random_html() {
        let html = "<html><body><p>Hello world</p></body></html>";
        assert_eq!(extract_confirm_token(html), None);
    }

    #[test]
    fn confirm_token_no_match_empty_string() {
        assert_eq!(extract_confirm_token(""), None);
    }

    #[test]
    fn confirm_token_no_match_similar_but_not_confirm() {
        let html = r#"<input name="confirmed" value="nope">"#;
        // "confirmed" does not match "confirm=" pattern for pattern 1
        // but it does contain "confirm=" when the 'e' follows... let's check:
        // "confirmed" contains substring "confirm" so pattern 1 ("confirm=") won't match
        // because after "confirm" comes "ed" not "=".
        // But wait, the html also doesn't have "confirm=" anywhere.
        assert_eq!(extract_confirm_token(html), None);
    }

    #[test]
    fn confirm_token_empty_token_returns_none() {
        // Pattern 1: confirm= followed immediately by &
        let html = "confirm=&rest";
        // Token would be "", which is checked: `if !token.is_empty()`
        assert_eq!(extract_confirm_token(html), None);
    }

    // ── can_handle for GoogleDriveSource ─────────────────────────────────

    #[test]
    fn can_handle_google_drive_directive() {
        let source = GoogleDriveSource::new(Client::new());
        let directive = DownloadDirective::GoogleDrive {
            id: "1AbCdEfGh".to_string(),
            hash: 42,
        };
        assert!(source.can_handle(&directive));
    }

    #[test]
    fn can_handle_rejects_mega() {
        let source = GoogleDriveSource::new(Client::new());
        let directive = DownloadDirective::Mega {
            url: "https://mega.nz/file/X#Y".to_string(),
            hash: 0,
        };
        assert!(!source.can_handle(&directive));
    }

    #[test]
    fn can_handle_rejects_nexus() {
        let source = GoogleDriveSource::new(Client::new());
        let directive = DownloadDirective::Nexus {
            game_id: GameId::from("skyrim"),
            mod_id: 1.into(),
            file_id: 1.into(),
            hash: 0,
        };
        assert!(!source.can_handle(&directive));
    }

    #[test]
    fn can_handle_rejects_github() {
        let source = GoogleDriveSource::new(Client::new());
        let directive = DownloadDirective::GitHub {
            user: "u".to_string(),
            repo: "r".to_string(),
            tag: "t".to_string(),
            asset: "a".to_string(),
            hash: 0,
        };
        assert!(!source.can_handle(&directive));
    }

    #[test]
    fn can_handle_rejects_direct_url() {
        let source = GoogleDriveSource::new(Client::new());
        let directive = DownloadDirective::DirectURL {
            url: "https://example.com/file".to_string(),
            headers: std::collections::HashMap::new(),
            mirror_resolver: None,
            hash: 0,
        };
        assert!(!source.can_handle(&directive));
    }
}
