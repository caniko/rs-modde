//! Plain HTTPS download source with `Range`-based resume and HTML mirror
//! resolution.

use std::path::Path;

use futures::StreamExt;
use reqwest::Client;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt};
use tracing::{debug, info};
use xxhash_rust::xxh3::Xxh3;
use xxhash_rust::xxh64::Xxh64;

use modde_core::manifest::wabbajack::DownloadDirective;

use crate::common::{ensure_parent, with_retry};
use crate::error::{SourceError, SourceResult, status_error};
use crate::mirror::resolve_html_mirrors;
use crate::traits::{DownloadHandle, DownloadSource, ProgressCallback, VerifiedFile};

/// Plain HTTPS download source with Range header support for resume.
pub struct DirectSource {
    client: Client,
}

impl DirectSource {
    /// Create a source that downloads over the given HTTP `client`.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl DownloadSource for DirectSource {
    fn can_handle(&self, directive: &DownloadDirective) -> bool {
        matches!(directive, DownloadDirective::DirectURL { .. })
    }

    async fn resolve(&self, directive: &DownloadDirective) -> SourceResult<DownloadHandle> {
        let DownloadDirective::DirectURL {
            url,
            headers,
            mirror_resolver,
            hash,
        } = directive
        else {
            return Err(SourceError::other(anyhow::anyhow!(
                "not a DirectURL directive"
            )));
        };

        let candidate_urls = if let Some(resolver) = mirror_resolver {
            resolve_html_mirrors(&self.client, resolver)
                .await
                .map_err(SourceError::other)?
        } else {
            Vec::new()
        };
        let mut headers = headers.clone();
        if let Some(resolver) = mirror_resolver
            && let Some(user_agent) = &resolver.user_agent
        {
            headers
                .entry("User-Agent".to_string())
                .or_insert_with(|| user_agent.clone());
        }

        Ok(DownloadHandle {
            url: url.clone(),
            candidate_urls,
            headers,
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

        let client = self.client.clone();
        let dest_ref = dest;
        let progress_ref = &progress;
        let candidates = if handle.candidate_urls.is_empty() {
            vec![handle.url.clone()]
        } else {
            handle.candidate_urls.clone()
        };

        let mut errors = Vec::new();
        let mut last_error = None;
        for (idx, url) in candidates.iter().enumerate() {
            let mut candidate = handle.clone();
            candidate.url = url.clone();
            let result = with_retry("direct download", || async {
                download_with_resume(&client, &candidate, dest_ref, progress_ref).await
            })
            .await;
            match result {
                Ok(verified) => {
                    if idx > 0 {
                        info!(url = %candidate.url, "direct download mirror succeeded");
                    }
                    return Ok(verified);
                }
                Err(e) => {
                    errors.push(format!("{}: {e:#}", candidate.url));
                    last_error = Some(e);
                    let _ = tokio::fs::remove_file(dest).await;
                }
            }
        }

        if candidates.len() == 1
            && let Some(error) = last_error
        {
            return Err(error);
        }

        Err(SourceError::other(anyhow::anyhow!(
            "all {} direct download candidate(s) failed:\n{}",
            candidates.len(),
            errors
                .iter()
                .map(|error| format!("  - {error}"))
                .collect::<Vec<_>>()
                .join("\n")
        )))
    }
}

async fn download_with_resume(
    client: &Client,
    handle: &DownloadHandle,
    dest: &Path,
    progress: &ProgressCallback,
) -> SourceResult<VerifiedFile> {
    let existing_len = tokio::fs::metadata(dest).await.map_or(0, |m| m.len());

    let mut req = client.get(&handle.url);
    for (k, v) in &handle.headers {
        req = req.header(k.as_str(), v.as_str());
    }

    if existing_len > 0 {
        debug!(bytes = existing_len, "attempting range resume");
        req = req.header("Range", format!("bytes={existing_len}-"));
    }

    let resp = status_error(req.send().await?)?;
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

    let mut xxh64 = Xxh64::new(0);
    let mut xxh3 = Xxh3::new();
    if status == reqwest::StatusCode::PARTIAL_CONTENT && existing_len > 0 {
        let mut existing = tokio::fs::File::open(dest).await?;
        let mut buf = vec![0_u8; 1024 * 1024];
        loop {
            let read = existing.read(&mut buf).await?;
            if read == 0 {
                break;
            }
            xxh64.update(&buf[..read]);
            xxh3.update(&buf[..read]);
        }
    }

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        xxh64.update(&chunk);
        xxh3.update(&chunk);
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        progress(downloaded, total_size);
    }

    file.flush().await?;
    let h64 = xxh64.digest();
    if h64 == handle.expected_hash || xxh3.digest() == handle.expected_hash {
        return Ok(VerifiedFile {
            path: dest.to_path_buf(),
            hash: handle.expected_hash,
        });
    }

    let _ = tokio::fs::remove_file(dest).await;
    Err(SourceError::hash_mismatch(dest, handle.expected_hash, h64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::thread;
    use xxhash_rust::xxh64::xxh64;

    #[tokio::test]
    async fn direct_download_tries_candidate_urls_in_order() {
        let body = b"mirror payload";
        let expected_hash = xxh64(body, 0);
        let (base_url, requests) = start_fallback_server(body);
        let handle = DownloadHandle {
            url: format!("{base_url}/original"),
            candidate_urls: vec![format!("{base_url}/bad"), format!("{base_url}/good")],
            headers: HashMap::new(),
            expected_hash,
            size_hint: None,
        };
        let temp = tempfile::tempdir().unwrap();
        let dest = temp.path().join("download.archive");
        let source = DirectSource::new(Client::new());

        let verified = source.download(handle, &dest).await.unwrap();
        assert_eq!(verified.hash, expected_hash);
        assert_eq!(tokio::fs::read(&verified.path).await.unwrap(), body);
        assert!(requests.load(Ordering::SeqCst) >= 2);
    }

    fn start_fallback_server(body: &'static [u8]) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let request_count = Arc::clone(&requests);
        thread::spawn(move || {
            for stream in listener.incoming().take(8) {
                let mut stream = stream.unwrap();
                let mut buf = [0_u8; 1024];
                let n = stream.read(&mut buf).unwrap();
                let request = String::from_utf8_lossy(&buf[..n]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");
                request_count.fetch_add(1, Ordering::SeqCst);
                match path {
                    "/bad" => write_response(&mut stream, "500 Internal Server Error", b"bad"),
                    "/good" => write_response(&mut stream, "200 OK", body),
                    _ => write_response(&mut stream, "404 Not Found", b"missing"),
                }
            }
        });
        (format!("http://{addr}"), requests)
    }

    fn write_response(stream: &mut std::net::TcpStream, status: &str, body: &[u8]) {
        let headers = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(headers.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    }
}
