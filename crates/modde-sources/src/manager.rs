use std::path::PathBuf;

/// Tracks an active or completed download.
#[derive(Debug, Clone)]
pub struct ManagedDownload {
    pub id: usize,
    pub url: String,
    pub dest: PathBuf,
    pub mod_name: Option<String>,
    pub state: DownloadState,
}

/// State of a managed download.
#[derive(Debug, Clone)]
pub enum DownloadState {
    Queued,
    Active { progress: f64 },
    Paused { bytes: u64 },
    Complete { path: PathBuf },
    Failed { error: String },
}

/// Manages a queue of downloads with progress tracking.
///
/// This is a synchronous data structure -- callers drive the actual async
/// downloads and update state through the provided methods.
pub struct DownloadManager {
    downloads: Vec<ManagedDownload>,
    max_concurrent: usize,
    next_id: usize,
}

impl DownloadManager {
    #[must_use]
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            downloads: Vec::new(),
            max_concurrent,
            next_id: 0,
        }
    }

    /// Add a download to the queue. Returns the assigned download ID.
    pub fn enqueue(&mut self, url: String, dest: PathBuf, mod_name: Option<String>) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.downloads.push(ManagedDownload {
            id,
            url,
            dest,
            mod_name,
            state: DownloadState::Queued,
        });
        id
    }

    /// Pause an active download, recording how many bytes were fetched so far.
    pub fn pause(&mut self, id: usize, bytes_so_far: u64) {
        if let Some(dl) = self.get_mut(id)
            && matches!(dl.state, DownloadState::Active { .. }) {
                dl.state = DownloadState::Paused {
                    bytes: bytes_so_far,
                };
            }
    }

    /// Move a paused download back to the queue.
    pub fn resume(&mut self, id: usize) {
        if let Some(dl) = self.get_mut(id)
            && matches!(dl.state, DownloadState::Paused { .. }) {
                dl.state = DownloadState::Queued;
            }
    }

    /// Remove a download from the manager entirely.
    pub fn cancel(&mut self, id: usize) {
        self.downloads.retain(|dl| dl.id != id);
    }

    /// Number of currently active downloads.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.downloads
            .iter()
            .filter(|dl| matches!(dl.state, DownloadState::Active { .. }))
            .count()
    }

    /// Maximum number of concurrent downloads allowed.
    #[must_use]
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    /// View all tracked downloads.
    #[must_use]
    pub fn all(&self) -> &[ManagedDownload] {
        &self.downloads
    }

    /// Get a mutable reference to a download by ID.
    pub fn get_mut(&mut self, id: usize) -> Option<&mut ManagedDownload> {
        self.downloads.iter_mut().find(|dl| dl.id == id)
    }

    /// Get an immutable reference to a download by ID.
    #[must_use]
    pub fn get(&self, id: usize) -> Option<&ManagedDownload> {
        self.downloads.iter().find(|dl| dl.id == id)
    }

    /// Returns true if there is room to start another download.
    #[must_use]
    pub fn can_start_more(&self) -> bool {
        self.active_count() < self.max_concurrent
    }

    /// Return IDs of queued downloads that could be activated, up to the
    /// remaining concurrency budget.
    #[must_use]
    pub fn next_queued(&self) -> Vec<usize> {
        let budget = self.max_concurrent.saturating_sub(self.active_count());
        self.downloads
            .iter()
            .filter(|dl| matches!(dl.state, DownloadState::Queued))
            .take(budget)
            .map(|dl| dl.id)
            .collect()
    }
}
