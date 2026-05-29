//! Source for manual-download directives, which cannot be fetched
//! automatically and instead fail fast with a pointer to the upstream URL.

use std::path::Path;

use modde_core::manifest::wabbajack::DownloadDirective;

use crate::error::{SourceError, SourceResult};
use crate::traits::{DownloadHandle, DownloadSource, ProgressCallback, VerifiedFile};

/// Source for `ManualDownloader` archives. The Wabbajack tool prompts the user
/// to download these by hand and drop the file into the downloads directory;
/// modde mirrors that behaviour by failing fast at resolve time with a clear
/// pointer to the upstream URL.
pub struct ManualSource;

impl ManualSource {
    /// Create a new manual source.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for ManualSource {
    fn default() -> Self {
        Self::new()
    }
}

impl DownloadSource for ManualSource {
    fn can_handle(&self, directive: &DownloadDirective) -> bool {
        matches!(directive, DownloadDirective::Manual { .. })
    }

    async fn resolve(&self, directive: &DownloadDirective) -> SourceResult<DownloadHandle> {
        let DownloadDirective::Manual {
            url,
            prompt,
            expected_name,
            ..
        } = directive
        else {
            return Err(SourceError::other(anyhow::anyhow!(
                "not a Manual directive"
            )));
        };

        let prompt = if prompt.is_empty() { "(none)" } else { prompt };
        Err(SourceError::other(anyhow::anyhow!(
            "manual download required for {expected_name}: visit {url} and place the downloaded \
             file in the modde downloads directory. Prompt from list author: {prompt}"
        )))
    }

    async fn download_with_progress(
        &self,
        _handle: DownloadHandle,
        _dest: &Path,
        _progress: ProgressCallback,
    ) -> SourceResult<VerifiedFile> {
        Err(SourceError::other(anyhow::anyhow!(
            "manual downloads cannot be fetched automatically"
        )))
    }
}
