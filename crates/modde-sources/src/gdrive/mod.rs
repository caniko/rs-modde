use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;

use modde_core::manifest::wabbajack::DownloadDirective;

use crate::traits::{DownloadHandle, DownloadSource, VerifiedFile};

/// Google Drive download source.
///
/// Uses OAuth2 device flow for headless compatibility.
/// Tokens are stored in the secret-service keyring.
#[allow(dead_code)]
pub struct GoogleDriveSource {
    client: Client,
}

impl GoogleDriveSource {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl DownloadSource for GoogleDriveSource {
    fn can_handle(&self, directive: &DownloadDirective) -> bool {
        matches!(directive, DownloadDirective::GoogleDrive { .. })
    }

    async fn resolve(&self, directive: &DownloadDirective) -> Result<DownloadHandle> {
        let DownloadDirective::GoogleDrive { id, hash } = directive else {
            anyhow::bail!("not a Google Drive directive");
        };

        // TODO: OAuth2 device flow + resolve actual download URL
        let url = format!("https://drive.google.com/uc?id={id}&export=download");

        Ok(DownloadHandle {
            url,
            headers: HashMap::new(),
            expected_hash: *hash,
            size_hint: None,
        })
    }

    async fn download(&self, handle: DownloadHandle, dest: &Path) -> Result<VerifiedFile> {
        let _ = (&handle, dest);
        todo!("Google Drive download not yet implemented")
    }
}
