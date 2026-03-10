use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;

use modde_core::manifest::wabbajack::DownloadDirective;

use crate::traits::{DownloadHandle, DownloadSource, VerifiedFile};

/// Mega.nz download source.
///
/// Handles Mega's client-side AES-CBC + MAC verification protocol.
#[allow(dead_code)]
pub struct MegaSource {
    client: Client,
}

impl MegaSource {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl DownloadSource for MegaSource {
    fn can_handle(&self, directive: &DownloadDirective) -> bool {
        matches!(directive, DownloadDirective::Mega { .. })
    }

    async fn resolve(&self, directive: &DownloadDirective) -> Result<DownloadHandle> {
        let DownloadDirective::Mega { url, hash } = directive else {
            anyhow::bail!("not a Mega directive");
        };

        // TODO: Mega API protocol to resolve actual download URL + key
        Ok(DownloadHandle {
            url: url.clone(),
            headers: HashMap::new(),
            expected_hash: *hash,
            size_hint: None,
        })
    }

    async fn download(&self, handle: DownloadHandle, dest: &Path) -> Result<VerifiedFile> {
        // TODO: implement Mega's crypto protocol (AES-CBC + MAC)
        let _ = (&handle, dest);
        todo!("Mega download not yet implemented")
    }
}
