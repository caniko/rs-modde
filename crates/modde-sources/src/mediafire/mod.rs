use std::path::Path;

use anyhow::{Context, Result};
use reqwest::Client;
use tracing::{debug, info};

use modde_core::manifest::wabbajack::DownloadDirective;

use crate::direct::DirectSource;
use crate::traits::{DownloadHandle, DownloadSource, ProgressCallback, VerifiedFile};

/// `MediaFire` download source. Resolves the file page to its underlying direct URL,
/// then delegates the actual transfer to [`DirectSource`].
pub struct MediaFireSource {
    client: Client,
    direct: DirectSource,
}

impl MediaFireSource {
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            direct: DirectSource::new(client.clone()),
            client,
        }
    }
}

impl DownloadSource for MediaFireSource {
    fn can_handle(&self, directive: &DownloadDirective) -> bool {
        matches!(directive, DownloadDirective::MediaFire { .. })
    }

    async fn resolve(&self, directive: &DownloadDirective) -> Result<DownloadHandle> {
        let DownloadDirective::MediaFire { url, hash } = directive else {
            anyhow::bail!("not a MediaFire directive");
        };

        let direct_url = scrape_mediafire_direct(&self.client, url).await?;
        info!(page = %url, direct = %direct_url, "resolved MediaFire direct URL");

        Ok(DownloadHandle {
            url: direct_url,
            candidate_urls: Vec::new(),
            headers: Default::default(),
            expected_hash: *hash,
            size_hint: None,
        })
    }

    async fn download_with_progress(
        &self,
        handle: DownloadHandle,
        dest: &Path,
        progress: ProgressCallback,
    ) -> Result<VerifiedFile> {
        self.direct
            .download_with_progress(handle, dest, progress)
            .await
    }
}

async fn scrape_mediafire_direct(client: &Client, page_url: &str) -> Result<String> {
    let html = client
        .get(page_url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (X11; Linux x86_64) modde/wabbajack",
        )
        .send()
        .await
        .with_context(|| format!("failed to fetch MediaFire page {page_url}"))?
        .error_for_status()
        .with_context(|| format!("MediaFire page returned an error for {page_url}"))?
        .text()
        .await
        .context("failed to read MediaFire page body")?;

    extract_mediafire_direct(&html).with_context(|| {
        format!("could not find MediaFire direct download link on page {page_url}")
    })
}

/// Extracts the actual download URL from a `MediaFire` file page.
///
/// The page contains an anchor of the form
/// `<a aria-label="Download file" class="input popsok …" href="https://download…mediafire.com/…">`.
fn extract_mediafire_direct(html: &str) -> Result<String> {
    let needle = "aria-label=\"Download file\"";
    let pos = html
        .find(needle)
        .ok_or_else(|| anyhow::anyhow!("MediaFire page is missing the 'Download file' anchor"))?;
    debug!("found mediafire download anchor at byte {pos}");

    let region_start = html[..pos].rfind("<a").unwrap_or(0);
    let region_end = pos
        + html[pos..]
            .find('>')
            .ok_or_else(|| anyhow::anyhow!("malformed anchor on MediaFire page"))?;
    let anchor = &html[region_start..=region_end];

    let href_marker = "href=\"";
    let href_pos = anchor
        .find(href_marker)
        .ok_or_else(|| anyhow::anyhow!("MediaFire anchor missing href"))?;
    let href_start = href_pos + href_marker.len();
    let href_end_rel = anchor[href_start..]
        .find('"')
        .ok_or_else(|| anyhow::anyhow!("MediaFire anchor href is unterminated"))?;
    Ok(anchor[href_start..href_start + href_end_rel].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_direct_link_from_popsok_button() {
        let html = r#"<html><body>
            <a aria-label="Download file" class="input popsok btn-prompt" href="https://download123.mediafire.com/abc/file.7z" id="downloadButton">
              <span class="dl-btn-label">Download (123MB)</span>
            </a>
        </body></html>"#;
        let url = extract_mediafire_direct(html).expect("should parse");
        assert_eq!(url, "https://download123.mediafire.com/abc/file.7z");
    }

    #[test]
    fn errors_when_no_download_button() {
        let html = "<html><body>nope</body></html>";
        assert!(extract_mediafire_direct(html).is_err());
    }
}
