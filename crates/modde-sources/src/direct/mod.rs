use std::path::Path;

use anyhow::Result;
use futures::StreamExt;
use reqwest::Client;
use tokio::io::AsyncWriteExt;
use tracing::debug;

use modde_core::manifest::wabbajack::DownloadDirective;

use crate::common::{ensure_parent, verify_and_wrap, with_retry};
use crate::traits::{DownloadHandle, DownloadSource, ProgressCallback, VerifiedFile};

/// Plain HTTPS download source with Range header support for resume.
pub struct DirectSource {
    client: Client,
}

impl DirectSource {
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl DownloadSource for DirectSource {
    fn can_handle(&self, directive: &DownloadDirective) -> bool {
        matches!(directive, DownloadDirective::DirectURL { .. })
    }

    async fn resolve(&self, directive: &DownloadDirective) -> Result<DownloadHandle> {
        let DownloadDirective::DirectURL { url, headers, hash } = directive else {
            anyhow::bail!("not a DirectURL directive");
        };

        Ok(DownloadHandle {
            url: url.clone(),
            headers: headers.clone(),
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
        ensure_parent(dest).await?;

        let client = self.client.clone();
        let handle_ref = &handle;
        let dest_ref = dest;
        let progress_ref = &progress;

        with_retry("direct download", || async {
            download_with_resume(&client, handle_ref, dest_ref, progress_ref).await
        })
        .await?;

        verify_and_wrap(dest, handle.expected_hash).await
    }
}

async fn download_with_resume(
    client: &Client,
    handle: &DownloadHandle,
    dest: &Path,
    progress: &ProgressCallback,
) -> Result<()> {
    let existing_len = tokio::fs::metadata(dest).await.map_or(0, |m| m.len());

    let mut req = client.get(&handle.url);
    for (k, v) in &handle.headers {
        req = req.header(k.as_str(), v.as_str());
    }

    if existing_len > 0 {
        debug!(bytes = existing_len, "attempting range resume");
        req = req.header("Range", format!("bytes={existing_len}-"));
    }

    let resp = req.send().await?.error_for_status()?;
    let status = resp.status();
    let total = resp.content_length().or(handle.size_hint).unwrap_or(0);

    let (mut file, mut downloaded) = if status == reqwest::StatusCode::PARTIAL_CONTENT {
        debug!("server returned 206, resuming download");
        let file = tokio::fs::OpenOptions::new()
            .append(true)
            .open(dest)
            .await?;
        (file, existing_len)
    } else {
        if existing_len > 0 {
            debug!(
                "server returned {}, restarting download from scratch",
                status
            );
        }
        let file = tokio::fs::File::create(dest).await?;
        (file, 0u64)
    };

    let total_size = if status == reqwest::StatusCode::PARTIAL_CONTENT {
        total + existing_len
    } else {
        total
    };

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        progress(downloaded, total_size);
    }

    file.flush().await?;
    Ok(())
}
