use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;

use modde_core::manifest::wabbajack::DownloadDirective;

/// A resolved download ready for fetching.
#[derive(Debug, Clone)]
pub struct DownloadHandle {
    pub url: String,
    pub headers: HashMap<String, String>,
    pub expected_hash: u64,
    pub size_hint: Option<u64>,
}

/// A downloaded and hash-verified file.
#[derive(Debug, Clone)]
pub struct VerifiedFile {
    pub path: PathBuf,
    pub hash: u64,
}

/// Trait for download source implementations.
#[async_trait]
pub trait DownloadSource: Send + Sync {
    /// Check if this source can handle the given directive.
    fn can_handle(&self, directive: &DownloadDirective) -> bool;

    /// Resolve a directive into a download handle with a concrete URL.
    async fn resolve(&self, directive: &DownloadDirective) -> anyhow::Result<DownloadHandle>;

    /// Download the file to `dest` and verify its hash.
    async fn download(&self, handle: DownloadHandle, dest: &Path) -> anyhow::Result<VerifiedFile>;
}
