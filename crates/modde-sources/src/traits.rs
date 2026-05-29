use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use modde_core::manifest::wabbajack::DownloadDirective;

use crate::error::SourceResult;

/// Progress callback: (`bytes_downloaded`, `total_bytes`).
/// `total_bytes` may be 0 if unknown.
pub type ProgressCallback = Arc<dyn Fn(u64, u64) + Send + Sync>;

/// A resolved download ready for fetching.
#[derive(Debug, Clone)]
pub struct DownloadHandle {
    pub url: String,
    pub candidate_urls: Vec<String>,
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
///
/// Each source (Nexus, GitHub, Google Drive, Mega, Direct URL) implements this
/// trait. `AnySource` provides zero-cost enum dispatch, while this trait enables
/// `impl DownloadSource` generics for monomorphization in hot paths.
pub trait DownloadSource: Send + Sync {
    /// Check if this source can handle the given directive.
    fn can_handle(&self, directive: &DownloadDirective) -> bool;

    /// Resolve a directive into a download handle with a concrete URL.
    fn resolve(
        &self,
        directive: &DownloadDirective,
    ) -> impl std::future::Future<Output = SourceResult<DownloadHandle>> + Send;

    /// Download the file to `dest` with progress reporting and verify its hash.
    fn download_with_progress(
        &self,
        handle: DownloadHandle,
        dest: &Path,
        progress: ProgressCallback,
    ) -> impl std::future::Future<Output = SourceResult<VerifiedFile>> + Send;

    /// Download the file to `dest` and verify its hash (no-op progress).
    fn download(
        &self,
        handle: DownloadHandle,
        dest: &Path,
    ) -> impl std::future::Future<Output = SourceResult<VerifiedFile>> + Send {
        let noop: ProgressCallback = Arc::new(|_, _| {});
        self.download_with_progress(handle, dest, noop)
    }
}

/// Enum dispatch for all download source implementations.
///
/// Wraps concrete source types for heterogeneous collections (e.g. `Vec<AnySource>`).
/// Each variant implements `DownloadSource`; the enum delegates via match.
pub enum AnySource {
    Nexus(crate::nexus::NexusSource),
    GitHub(crate::github::GitHubSource),
    GoogleDrive(crate::gdrive::GoogleDriveSource),
    Mega(crate::mega::MegaSource),
    MediaFire(crate::mediafire::MediaFireSource),
    Manual(crate::manual::ManualSource),
    Direct(crate::direct::DirectSource),
    WabbajackCdn(crate::wabbajack::cdn::WabbajackCdnSource),
}

/// Dispatch a non-async method call to the inner source variant.
macro_rules! dispatch {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
            AnySource::Nexus(s) => s.$method($($arg),*),
            AnySource::GitHub(s) => s.$method($($arg),*),
            AnySource::GoogleDrive(s) => s.$method($($arg),*),
            AnySource::Mega(s) => s.$method($($arg),*),
            AnySource::MediaFire(s) => s.$method($($arg),*),
            AnySource::Manual(s) => s.$method($($arg),*),
            AnySource::Direct(s) => s.$method($($arg),*),
            AnySource::WabbajackCdn(s) => s.$method($($arg),*),
        }
    };
}

/// Dispatch an async method call to the inner source variant.
macro_rules! dispatch_async {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
            AnySource::Nexus(s) => s.$method($($arg),*).await,
            AnySource::GitHub(s) => s.$method($($arg),*).await,
            AnySource::GoogleDrive(s) => s.$method($($arg),*).await,
            AnySource::Mega(s) => s.$method($($arg),*).await,
            AnySource::MediaFire(s) => s.$method($($arg),*).await,
            AnySource::Manual(s) => s.$method($($arg),*).await,
            AnySource::Direct(s) => s.$method($($arg),*).await,
            AnySource::WabbajackCdn(s) => s.$method($($arg),*).await,
        }
    };
}

impl DownloadSource for AnySource {
    fn can_handle(&self, directive: &DownloadDirective) -> bool {
        dispatch!(self, can_handle(directive))
    }

    async fn resolve(&self, directive: &DownloadDirective) -> SourceResult<DownloadHandle> {
        dispatch_async!(self, resolve(directive))
    }

    async fn download_with_progress(
        &self,
        handle: DownloadHandle,
        dest: &Path,
        progress: ProgressCallback,
    ) -> SourceResult<VerifiedFile> {
        dispatch_async!(self, download_with_progress(handle, dest, progress))
    }
}
