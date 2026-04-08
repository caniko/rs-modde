use std::path::PathBuf;

use crate::meta::DownloadMeta;

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
/// responsible for taking tasks via [`take_next`], spawning async downloads,
/// and updating task state when downloads progress or complete.
pub struct DownloadQueue {
    tasks: Vec<DownloadTask>,
    max_concurrent: usize,
    next_id: usize,
}

impl DownloadQueue {
    /// Create a new queue with the given concurrency limit.
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

    /// Pause an active download. No-op if the task is not `Active`.
    pub fn pause(&mut self, id: usize) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            if let DownloadState::Active {
                bytes_downloaded,
                total_bytes,
            } = task.state
            {
                task.state = DownloadState::Paused {
                    bytes_downloaded,
                    total_bytes,
                };
            }
        }
    }

    /// Resume a paused download by moving it back to `Queued`.
    pub fn resume(&mut self, id: usize) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            if matches!(task.state, DownloadState::Paused { .. }) {
                task.state = DownloadState::Queued;
            }
        }
    }

    /// Remove a task from the queue entirely.
    pub fn cancel(&mut self, id: usize) {
        self.tasks.retain(|t| t.id != id);
    }

    /// Number of currently active downloads.
    pub fn active_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| matches!(t.state, DownloadState::Active { .. }))
            .count()
    }

    /// All tasks in the `Queued` state, ready to be started.
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
        Some(&mut self.tasks[idx])
    }

    /// Look up a task by ID.
    pub fn get(&self, id: usize) -> Option<&DownloadTask> {
        self.tasks.iter().find(|t| t.id == id)
    }

    /// Look up a task by ID (mutable).
    pub fn get_mut(&mut self, id: usize) -> Option<&mut DownloadTask> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    /// Total number of tasks in the queue (all states).
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
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
}
