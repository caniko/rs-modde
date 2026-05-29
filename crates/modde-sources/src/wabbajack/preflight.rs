//! Pre-install check that verifies Wabbajack CDN authored files referenced by a
//! manifest are still downloadable, failing fast with actionable remediation via
//! [`MissingAuthoredArtifacts`] when any are unavailable. See
//! [`preflight_authored_files`].

use std::fmt::Write as _;

use anyhow::{Result, bail};

use modde_core::manifest::wabbajack::{ArchiveEntry, ArchiveState};

use super::catalog::{authored_file_target, check_authored_file_available};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingAuthoredArtifact {
    pub archive_name: String,
    pub expected_hash: u64,
    pub original_url: String,
    pub metadata_url: String,
    pub status_or_error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingAuthoredArtifacts {
    pub artifacts: Vec<MissingAuthoredArtifact>,
}

impl std::fmt::Display for MissingAuthoredArtifacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Wabbajack manifest references {} unavailable authored file(s).",
            self.artifacts.len()
        )?;
        writeln!(
            f,
            "The install cannot continue without restoring these exact files or using a newer .wabbajack manifest."
        )?;
        writeln!(
            f,
            "Upstream producer to fix: the LoTF/Wabbajack authored-files publishing workflow."
        )?;
        for artifact in &self.artifacts {
            writeln!(f)?;
            writeln!(f, "- {}", artifact.archive_name)?;
            writeln!(f, "  expected hash: {:016x}", artifact.expected_hash)?;
            writeln!(f, "  original URL: {}", artifact.original_url)?;
            writeln!(f, "  metadata URL: {}", artifact.metadata_url)?;
            writeln!(f, "  status/error: {}", artifact.status_or_error)?;
            writeln!(
                f,
                "  validate: curl -fI '{}'",
                artifact.metadata_url.replace('\'', "'\\''")
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for MissingAuthoredArtifacts {}

pub async fn preflight_authored_files(
    client: &reqwest::Client,
    archives: &[ArchiveEntry],
) -> Result<()> {
    let mut missing = Vec::new();

    for archive in archives {
        let Some(url) = archive_wabbajack_cdn_url(archive) else {
            continue;
        };

        let metadata_url = authored_file_target(url)
            .map(|target| target.metadata_url)
            .unwrap_or_else(|| "<unavailable: malformed authored-files URL>".to_string());

        if let Err(e) = check_authored_file_available(client, url).await {
            missing.push(MissingAuthoredArtifact {
                archive_name: archive.name.clone(),
                expected_hash: archive.hash,
                original_url: url.to_string(),
                metadata_url,
                status_or_error: status_or_error(&e),
            });
        }
    }

    if !missing.is_empty() {
        bail!(MissingAuthoredArtifacts { artifacts: missing });
    }

    Ok(())
}

fn archive_wabbajack_cdn_url(archive: &ArchiveEntry) -> Option<&str> {
    let Some(ArchiveState::WabbajackCDNDownloader { metadata }) = archive.state.as_ref() else {
        return None;
    };
    metadata
        .get("Url")
        .and_then(serde_json::Value::as_str)
        .filter(|url| !url.is_empty())
}

fn status_or_error(err: &anyhow::Error) -> String {
    let mut out = String::new();
    for (idx, cause) in err.chain().enumerate() {
        if idx > 0 {
            let _ = write!(out, ": ");
        }
        let _ = write!(out, "{cause}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::thread;

    fn cdn_archive(name: &str, hash: u64, url: &str) -> ArchiveEntry {
        ArchiveEntry {
            hash,
            name: name.into(),
            size: 1,
            state: Some(ArchiveState::WabbajackCDNDownloader {
                metadata: [("Url".to_string(), serde_json::Value::String(url.into()))].into(),
            }),
        }
    }

    fn nexus_archive() -> ArchiveEntry {
        ArchiveEntry {
            hash: 2,
            name: "nexus".into(),
            size: 1,
            state: Some(ArchiveState::NexusDownloader {
                game_name: "skyrimspecialedition".into(),
                mod_id: 1.into(),
                file_id: 2.into(),
            }),
        }
    }

    #[tokio::test]
    async fn wabbajack_authored_preflight_ignores_non_cdn_archives() {
        preflight_authored_files(&reqwest::Client::new(), &[nexus_archive()])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn wabbajack_authored_preflight_reports_one_missing_file() {
        let archive = cdn_archive(
            "missing.7z",
            0x1234,
            "https://authored-files.wabbajack.org/missing.7z_abc",
        );
        let err = preflight_authored_files(&reqwest::Client::new(), &[archive])
            .await
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("unavailable authored file"));
        assert!(msg.contains("missing.7z"));
        assert!(msg.contains("curl -fI"));
    }

    #[tokio::test]
    async fn wabbajack_authored_preflight_reports_multiple_missing_files() {
        let archives = [
            cdn_archive(
                "missing-a.7z",
                1,
                "https://authored-files.wabbajack.org/missing-a.7z_abc",
            ),
            cdn_archive(
                "missing-b.7z",
                2,
                "https://authored-files.wabbajack.org/missing-b.7z_def",
            ),
        ];
        let err = preflight_authored_files(&reqwest::Client::new(), &archives)
            .await
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("2 unavailable"));
        assert!(msg.contains("missing-a.7z"));
        assert!(msg.contains("missing-b.7z"));
    }

    #[tokio::test]
    async fn wabbajack_authored_preflight_passes_available_metadata() {
        let (base_url, handle) = start_authored_metadata_server();
        let archive = cdn_archive(
            "available.7z",
            1,
            &format!("{base_url}/authored_files/download/available.7z_abc"),
        );
        preflight_authored_files(&reqwest::Client::new(), &[archive])
            .await
            .unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn missing_authored_file_error_formats_remediation() {
        let error = MissingAuthoredArtifacts {
            artifacts: vec![MissingAuthoredArtifact {
                archive_name: "missing.7z".into(),
                expected_hash: 0x1234,
                original_url: "https://authored-files.wabbajack.org/missing.7z_abc".into(),
                metadata_url: "https://build.wabbajack.org/authored_files/download/missing.7z_abc"
                    .into(),
                status_or_error: "404 Not Found".into(),
            }],
        };
        let msg = error.to_string();
        assert!(msg.contains("LoTF/Wabbajack authored-files publishing workflow"));
        assert!(msg.contains("curl -fI"));
        assert!(msg.contains("0000000000001234"));
    }

    fn start_authored_metadata_server() -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            if let Some(stream) = listener.incoming().next() {
                let mut stream = stream.unwrap();
                let mut buf = [0_u8; 1024];
                let _ = stream.read(&mut buf).unwrap();
                let body = br#"
                  <script>
                    const MUNGED_NAME = "available.7z_abc";
                    const FILE_NAME = "available.7z";
                    const FILE_SIZE_BYTES = 0;
                    const PARTS = [];
                  </script>
                "#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(body).unwrap();
            }
        });
        (format!("http://{addr}"), handle)
    }
}
