//! Per-download state machine backed by `.meta` sidecars, used to resume,
//! pause, and report progress for individual download tasks.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::meta::{DownloadMeta, meta_path};

/// State machine for a single download task.
#[derive(Debug, Clone, PartialEq)]
pub enum DownloadState {
    Queued,
    Active {
        bytes_downloaded: u64,
        total_bytes: Option<u64>,
    },
    Paused {
        bytes_downloaded: u64,
        total_bytes: Option<u64>,
    },
    Complete {
        path: PathBuf,
        hash: u64,
    },
    Failed {
        error: String,
    },
}

/// A single download in the queue.
pub struct DownloadTask {
    pub id: usize,
    pub url: String,
    pub dest: PathBuf,
    pub expected_hash: Option<u64>,
    pub state: DownloadState,
    pub meta: DownloadMeta,
}

/// Synchronous download queue that tracks tasks and enforces concurrency limits.
///
/// This is a pure data structure — it does not perform I/O. The caller is
/// responsible for taking tasks via [`take_next`](DownloadQueue::take_next), spawning async downloads,
/// and updating task state when downloads progress or complete.
pub struct DownloadQueue {
    tasks: Vec<DownloadTask>,
    max_concurrent: usize,
    next_id: usize,
}

impl DownloadQueue {
    /// Create a new queue with the given concurrency limit.
    #[must_use]
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            tasks: Vec::new(),
            max_concurrent,
            next_id: 0,
        }
    }

    /// Add a task to the queue. Returns the assigned task ID.
    pub fn enqueue(
        &mut self,
        url: String,
        dest: PathBuf,
        expected_hash: Option<u64>,
        meta: DownloadMeta,
    ) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        let mut meta = meta;
        meta.expected_hash = expected_hash.or(meta.expected_hash);
        self.tasks.push(DownloadTask {
            id,
            url,
            dest,
            expected_hash,
            state: DownloadState::Queued,
            meta,
        });
        id
    }

    /// Rebuild a queue from persisted `.meta` sidecars in a download directory.
    ///
    /// Active downloads are restored as paused because no transport is running
    /// after process startup. Complete entries are restored only when the
    /// downloaded file still exists.
    pub fn load_from_sidecars(download_dir: &Path, max_concurrent: usize) -> Result<Self> {
        let mut queue = Self::new(max_concurrent);
        if !download_dir.exists() {
            return Ok(queue);
        }

        let mut sidecars = std::fs::read_dir(download_dir)
            .with_context(|| format!("reading {}", download_dir.display()))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        sidecars.sort_by_key(std::fs::DirEntry::path);

        for entry in sidecars {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("meta") {
                continue;
            }
            let meta = DownloadMeta::load(&path)?;
            let Some(dest) = download_path_from_meta_path(&path) else {
                continue;
            };
            let state = state_from_meta(&meta, &dest);
            let id = queue.next_id;
            queue.next_id += 1;
            queue.tasks.push(DownloadTask {
                id,
                url: meta.url.clone(),
                dest,
                expected_hash: meta.expected_hash,
                state,
                meta,
            });
        }

        Ok(queue)
    }

    /// Persist every task's `.meta` sidecar beside its destination file.
    pub fn save_sidecars(&mut self) -> Result<()> {
        for task in &mut self.tasks {
            sync_meta_from_state(task);
            task.meta.save(&meta_path(&task.dest))?;
        }
        Ok(())
    }

    /// Pause an active download. No-op if the task is not `Active`.
    pub fn pause(&mut self, id: usize) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id)
            && let DownloadState::Active {
                bytes_downloaded,
                total_bytes,
            } = task.state
        {
            task.state = DownloadState::Paused {
                bytes_downloaded,
                total_bytes,
            };
            sync_meta_from_state(task);
        }
    }

    /// Resume a paused download by moving it back to `Queued`.
    pub fn resume(&mut self, id: usize) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id)
            && matches!(task.state, DownloadState::Paused { .. })
        {
            task.state = DownloadState::Queued;
            sync_meta_from_state(task);
        }
    }

    /// Remove a task from the queue entirely.
    pub fn cancel(&mut self, id: usize) {
        self.tasks.retain(|t| t.id != id);
    }

    /// Number of currently active downloads.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| matches!(t.state, DownloadState::Active { .. }))
            .count()
    }

    /// All tasks in the `Queued` state, ready to be started.
    #[must_use]
    pub fn pending(&self) -> Vec<&DownloadTask> {
        self.tasks
            .iter()
            .filter(|t| matches!(t.state, DownloadState::Queued))
            .collect()
    }

    /// Get the next `Queued` task if the concurrency limit has not been reached.
    ///
    /// Transitions the task to `Active` and returns a mutable reference so the
    /// caller can spawn the download.
    pub fn take_next(&mut self) -> Option<&mut DownloadTask> {
        if self.active_count() >= self.max_concurrent {
            return None;
        }

        // Find the index of the first Queued task.
        let idx = self
            .tasks
            .iter()
            .position(|t| matches!(t.state, DownloadState::Queued))?;

        self.tasks[idx].state = DownloadState::Active {
            bytes_downloaded: 0,
            total_bytes: None,
        };
        sync_meta_from_state(&mut self.tasks[idx]);
        Some(&mut self.tasks[idx])
    }

    /// Look up a task by ID.
    #[must_use]
    pub fn get(&self, id: usize) -> Option<&DownloadTask> {
        self.tasks.iter().find(|t| t.id == id)
    }

    /// Look up a task by ID (mutable).
    pub fn get_mut(&mut self, id: usize) -> Option<&mut DownloadTask> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    /// View all tracked tasks in insertion order.
    #[must_use]
    pub fn all(&self) -> &[DownloadTask] {
        &self.tasks
    }

    /// Total number of tasks in the queue (all states).
    #[must_use]
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Whether the queue is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}

fn sync_meta_from_state(task: &mut DownloadTask) {
    task.meta.expected_hash = task.expected_hash.or(task.meta.expected_hash);
    match &task.state {
        DownloadState::Queued => {
            task.meta.status = "queued".to_string();
        }
        DownloadState::Active {
            bytes_downloaded,
            total_bytes,
        } => {
            task.meta.status = "downloading".to_string();
            task.meta.bytes_downloaded = *bytes_downloaded;
            task.meta.total_bytes = *total_bytes;
        }
        DownloadState::Paused {
            bytes_downloaded,
            total_bytes,
        } => {
            task.meta.status = "paused".to_string();
            task.meta.bytes_downloaded = *bytes_downloaded;
            task.meta.total_bytes = *total_bytes;
        }
        DownloadState::Complete { hash, .. } => {
            task.meta.status = "complete".to_string();
            task.meta.expected_hash = Some(*hash);
            if let Ok(metadata) = std::fs::metadata(&task.dest) {
                task.meta.bytes_downloaded = metadata.len();
                task.meta.total_bytes = Some(metadata.len());
            }
        }
        DownloadState::Failed { error } => {
            task.meta.status = format!("failed: {error}");
        }
    }
}

fn state_from_meta(meta: &DownloadMeta, dest: &Path) -> DownloadState {
    if meta.status == "complete" && dest.exists() {
        return DownloadState::Complete {
            path: dest.to_path_buf(),
            hash: meta.expected_hash.unwrap_or_default(),
        };
    }
    if meta.status == "paused" || meta.status == "downloading" {
        return DownloadState::Paused {
            bytes_downloaded: meta.bytes_downloaded,
            total_bytes: meta.total_bytes,
        };
    }
    if let Some(error) = meta.status.strip_prefix("failed: ") {
        return DownloadState::Failed {
            error: error.to_string(),
        };
    }
    DownloadState::Queued
}

fn download_path_from_meta_path(path: &Path) -> Option<PathBuf> {
    let file_name = path.file_name()?.to_str()?;
    let download_name = file_name.strip_suffix(".meta")?;
    Some(path.with_file_name(download_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_meta() -> DownloadMeta {
        DownloadMeta {
            url: "https://example.com/mod.zip".into(),
            expected_hash: None,
            bytes_downloaded: 0,
            total_bytes: None,
            nexus_mod_id: None,
            nexus_file_id: None,
            game_domain: None,
            mod_name: None,
            version: None,
            status: "queued".into(),
        }
    }

    #[test]
    fn test_enqueue_and_take_next() {
        let mut q = DownloadQueue::new(2);
        let id = q.enqueue(
            "https://example.com/a.zip".into(),
            PathBuf::from("/tmp/a.zip"),
            Some(123),
            test_meta(),
        );
        assert_eq!(id, 0);
        assert_eq!(q.len(), 1);
        assert_eq!(q.pending().len(), 1);

        let task = q.take_next().unwrap();
        assert_eq!(task.id, 0);
        assert!(matches!(task.state, DownloadState::Active { .. }));

        // No more queued tasks.
        assert!(q.pending().is_empty());
        assert!(q.take_next().is_none());
    }

    #[test]
    fn test_concurrency_limit() {
        let mut q = DownloadQueue::new(2);

        q.enqueue("https://a".into(), PathBuf::from("/a"), None, test_meta());
        q.enqueue("https://b".into(), PathBuf::from("/b"), None, test_meta());
        q.enqueue("https://c".into(), PathBuf::from("/c"), None, test_meta());

        // Take two — should succeed.
        assert!(q.take_next().is_some());
        assert!(q.take_next().is_some());

        // Third should be blocked by concurrency limit.
        assert_eq!(q.active_count(), 2);
        assert!(q.take_next().is_none());

        // Still one pending.
        assert_eq!(q.pending().len(), 1);
    }

    #[test]
    fn test_pause_resume() {
        let mut q = DownloadQueue::new(2);
        let id = q.enqueue("https://a".into(), PathBuf::from("/a"), None, test_meta());

        // Take to make Active.
        q.take_next();
        assert_eq!(q.active_count(), 1);

        // Pause.
        q.pause(id);
        assert_eq!(q.active_count(), 0);
        assert!(matches!(
            q.get(id).unwrap().state,
            DownloadState::Paused { .. }
        ));

        // Resume — goes back to Queued.
        q.resume(id);
        assert!(matches!(q.get(id).unwrap().state, DownloadState::Queued));
        assert_eq!(q.pending().len(), 1);

        // Can take again.
        let task = q.take_next().unwrap();
        assert!(matches!(task.state, DownloadState::Active { .. }));
    }

    #[test]
    fn test_cancel() {
        let mut q = DownloadQueue::new(2);
        let id0 = q.enqueue("https://a".into(), PathBuf::from("/a"), None, test_meta());
        let id1 = q.enqueue("https://b".into(), PathBuf::from("/b"), None, test_meta());

        assert_eq!(q.len(), 2);

        q.cancel(id0);
        assert_eq!(q.len(), 1);
        assert!(q.get(id0).is_none());
        assert!(q.get(id1).is_some());
    }

    #[test]
    fn test_queue_sidecar_roundtrip_restores_active_as_paused() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("mod.zip");
        std::fs::write(&dest, b"partial").unwrap();
        let mut queue = DownloadQueue::new(2);
        let id = queue.enqueue(
            "https://example.com/mod.zip".into(),
            dest.clone(),
            Some(123),
            test_meta(),
        );
        queue.get_mut(id).unwrap().state = DownloadState::Active {
            bytes_downloaded: 7,
            total_bytes: Some(100),
        };

        queue.save_sidecars().unwrap();
        let restored = DownloadQueue::load_from_sidecars(dir.path(), 2).unwrap();
        let task = restored.all().first().unwrap();

        assert_eq!(task.url, "https://example.com/mod.zip");
        assert_eq!(task.dest, dest);
        assert_eq!(task.expected_hash, Some(123));
        assert!(matches!(
            task.state,
            DownloadState::Paused {
                bytes_downloaded: 7,
                total_bytes: Some(100)
            }
        ));
    }
}
