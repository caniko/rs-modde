use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;

use modde_core::manifest::wabbajack::DownloadDirective;

use crate::traits::{DownloadHandle, DownloadSource, VerifiedFile};

/// Plain HTTPS download source with Range header support for resume.
#[allow(dead_code)]
pub struct DirectSource {
    client: Client,
}

impl DirectSource {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait]
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

    async fn download(&self, handle: DownloadHandle, dest: &Path) -> Result<VerifiedFile> {
        // TODO: download with Range header support, retry with exponential backoff
        let _ = (&handle, dest);
        todo!("direct download not yet implemented")
    }
}
