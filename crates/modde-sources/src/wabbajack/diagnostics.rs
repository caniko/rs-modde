//! Runtime diagnostics for Wabbajack installs: tracks install phase, active
//! archive batches and patches, and process/cgroup memory, emitting periodic
//! JSONL heartbeats and aborting on a memory-saturated stall. See
//! [`WabbajackDiagnostics`].

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct WabbajackDiagnosticsOptions {
    pub dir: PathBuf,
    pub interval: Duration,
    pub stall_warn: Duration,
    pub stall_abort: Duration,
}

#[derive(Debug, Clone)]
pub struct WabbajackDiagnostics {
    inner: Arc<DiagnosticsInner>,
}

#[derive(Debug)]
struct DiagnosticsInner {
    options: WabbajackDiagnosticsOptions,
    start: Instant,
    abort: AtomicBool,
    state: Mutex<DiagnosticsState>,
}

#[derive(Debug)]
struct DiagnosticsState {
    phase: String,
    last_progress: Instant,
    progress_events: u64,
    completed_archive_batches: u64,
    completed_create_bsa: u64,
    active_archive_batches: Vec<ActiveArchiveBatchState>,
    active_patches: Vec<ActivePatchState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveArchiveBatch {
    archive_hash: String,
    directive_count: usize,
    patch_count: usize,
    archive_size_bytes: u64,
    started_millis_ago: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivePatch {
    to: String,
    patch_id: String,
    source_bytes: u64,
    expected_output_bytes: u64,
    started_millis_ago: u128,
}

#[derive(Debug, Clone)]
struct ActiveArchiveBatchState {
    archive_hash: String,
    directive_count: usize,
    patch_count: usize,
    archive_size_bytes: u64,
    started: Instant,
}

#[derive(Debug, Clone)]
struct ActivePatchState {
    to: String,
    patch_id: String,
    source_bytes: u64,
    expected_output_bytes: u64,
    started: Instant,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HeartbeatRecord {
    pub kind: String,
    pub unix_ms: u128,
    pub uptime_ms: u128,
    pub phase: String,
    pub progress_events: u64,
    pub completed_archive_batches: u64,
    pub completed_create_bsa: u64,
    pub idle_ms: u128,
    pub active_archive_batches: Vec<ActiveArchiveBatch>,
    #[serde(default)]
    pub active_patches: Vec<ActivePatch>,
    pub process: ProcessSnapshot,
    pub cgroup: Option<CgroupSnapshot>,
    pub byte_cache_used: u64,
    pub abort_requested: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ProcessSnapshot {
    pub vm_rss_kib: Option<u64>,
    pub vm_swap_kib: Option<u64>,
    pub threads: Option<u64>,
    pub rchar: Option<u64>,
    pub wchar: Option<u64>,
    pub read_bytes: Option<u64>,
    pub write_bytes: Option<u64>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CgroupSnapshot {
    pub path: String,
    pub memory_current: Option<u64>,
    pub memory_high: Option<u64>,
    pub memory_max: Option<u64>,
    pub memory_swap_current: Option<u64>,
    pub memory_swap_max: Option<u64>,
    pub memory_events_high: Option<u64>,
    pub memory_events_oom: Option<u64>,
    pub memory_events_oom_kill: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveBatchRecord {
    pub kind: String,
    pub unix_ms: u128,
    pub archive_hash: String,
    pub archive_size_bytes: u64,
    pub directive_count: usize,
    pub patch_count: usize,
    pub elapsed_ms: u128,
    #[serde(default)]
    pub trust_check_ms: u128,
    pub extraction_ms: u128,
    pub patch_ms: u128,
    #[serde(default)]
    pub prune_ms: u128,
    pub extracted_patch_source_bytes: u64,
    #[serde(default)]
    pub sidecar_hit: bool,
    #[serde(default)]
    pub streamed_hash_bytes: u64,
    #[serde(default)]
    pub memory_archive_hit: bool,
    #[serde(default)]
    pub disk_archive_fallback: bool,
    #[serde(default)]
    pub pruned_bytes: u64,
    pub byte_cache_used_before: u64,
    pub byte_cache_used_after: u64,
    pub success_count: usize,
    pub error_count: usize,
    #[serde(default)]
    pub first_error: Option<String>,
    pub rss_before_kib: Option<u64>,
    pub rss_after_kib: Option<u64>,
    pub swap_before_kib: Option<u64>,
    pub swap_after_kib: Option<u64>,
}

impl WabbajackDiagnostics {
    pub async fn new(options: WabbajackDiagnosticsOptions) -> Result<Self> {
        tokio::fs::create_dir_all(&options.dir)
            .await
            .with_context(|| {
                format!(
                    "failed to create diagnostics directory: {}",
                    options.dir.display()
                )
            })?;
        Ok(Self {
            inner: Arc::new(DiagnosticsInner {
                options,
                start: Instant::now(),
                abort: AtomicBool::new(false),
                state: Mutex::new(DiagnosticsState {
                    phase: "starting".to_string(),
                    last_progress: Instant::now(),
                    progress_events: 0,
                    completed_archive_batches: 0,
                    completed_create_bsa: 0,
                    active_archive_batches: Vec::new(),
                    active_patches: Vec::new(),
                }),
            }),
        })
    }

    pub fn spawn_heartbeat(
        &self,
        byte_cache_used: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> tokio::task::JoinHandle<()> {
        let diagnostics = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(diagnostics.inner.options.interval);
            loop {
                interval.tick().await;
                if let Err(e) = diagnostics.write_heartbeat(byte_cache_used()).await {
                    warn!("failed to write Wabbajack diagnostics heartbeat: {e:#}");
                }
            }
        })
    }

    pub fn set_phase(&self, phase: impl Into<String>) {
        self.inner.state.lock().phase = phase.into();
    }

    pub fn record_progress(&self, event: ProgressEvent) {
        let mut state = self.inner.state.lock();
        state.last_progress = Instant::now();
        state.progress_events += 1;
        match event {
            ProgressEvent::ArchiveBatchComplete => state.completed_archive_batches += 1,
            ProgressEvent::CreateBsaComplete => state.completed_create_bsa += 1,
            ProgressEvent::Other => {}
        }
    }

    pub fn start_archive_batch(
        &self,
        archive_hash: u64,
        directive_count: usize,
        patch_count: usize,
        archive_size_bytes: u64,
    ) -> ArchiveBatchGuard {
        let started = Instant::now();
        let active = ActiveArchiveBatchState {
            archive_hash: format!("{archive_hash:016x}"),
            directive_count,
            patch_count,
            archive_size_bytes,
            started,
        };
        self.inner.state.lock().active_archive_batches.push(active);
        ArchiveBatchGuard {
            diagnostics: self.clone(),
            archive_hash,
            started,
        }
    }

    pub fn start_patch(
        &self,
        to: impl Into<String>,
        patch_id: impl Into<String>,
        source_bytes: u64,
        expected_output_bytes: u64,
    ) -> PatchGuard {
        let started = Instant::now();
        let active = ActivePatchState {
            to: to.into(),
            patch_id: patch_id.into(),
            source_bytes,
            expected_output_bytes,
            started,
        };
        let patch_id = active.patch_id.clone();
        self.inner.state.lock().active_patches.push(active);
        PatchGuard {
            diagnostics: self.clone(),
            patch_id,
            started,
        }
    }

    pub fn check_abort(&self) -> Result<()> {
        if self.inner.abort.load(Ordering::Relaxed) {
            bail!("Wabbajack install aborted by diagnostics stall detector");
        }
        Ok(())
    }

    pub async fn record_archive_batch(&self, record: &ArchiveBatchRecord) -> Result<()> {
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.inner.options.dir.join("archive-batches.jsonl"))
            .await?;
        file.write_all(&serde_json::to_vec(record)?).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }

    async fn write_heartbeat(&self, byte_cache_used: u64) -> Result<()> {
        let snapshot = self.snapshot(byte_cache_used);
        let idle = Duration::from_millis(snapshot.idle_ms as u64);
        if idle >= self.inner.options.stall_warn {
            warn!(
                phase = %snapshot.phase,
                idle_seconds = idle.as_secs(),
                active_batches = snapshot.active_archive_batches.len(),
                "Wabbajack install has not completed a batch or sentinel recently"
            );
        }
        if idle >= self.inner.options.stall_abort && memory_is_saturated(snapshot.cgroup.as_ref()) {
            self.inner.abort.store(true, Ordering::Relaxed);
            warn!(
                phase = %snapshot.phase,
                idle_seconds = idle.as_secs(),
                "Wabbajack diagnostics requested abort: cgroup memory is saturated and apply progress stopped"
            );
        }

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.inner.options.dir.join("heartbeat.jsonl"))
            .await?;
        file.write_all(&serde_json::to_vec(&snapshot)?).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }

    fn snapshot(&self, byte_cache_used: u64) -> HeartbeatRecord {
        let now = Instant::now();
        let state = self.inner.state.lock();
        let active_archive_batches = state
            .active_archive_batches
            .iter()
            .map(|active| ActiveArchiveBatch {
                archive_hash: active.archive_hash.clone(),
                directive_count: active.directive_count,
                patch_count: active.patch_count,
                archive_size_bytes: active.archive_size_bytes,
                started_millis_ago: now.duration_since(active.started).as_millis(),
            })
            .collect();
        let active_patches = state
            .active_patches
            .iter()
            .map(|active| ActivePatch {
                to: active.to.clone(),
                patch_id: active.patch_id.clone(),
                source_bytes: active.source_bytes,
                expected_output_bytes: active.expected_output_bytes,
                started_millis_ago: now.duration_since(active.started).as_millis(),
            })
            .collect();
        HeartbeatRecord {
            kind: "heartbeat".to_string(),
            unix_ms: unix_ms(),
            uptime_ms: now.duration_since(self.inner.start).as_millis(),
            phase: state.phase.clone(),
            progress_events: state.progress_events,
            completed_archive_batches: state.completed_archive_batches,
            completed_create_bsa: state.completed_create_bsa,
            idle_ms: now.duration_since(state.last_progress).as_millis(),
            active_archive_batches,
            active_patches,
            process: process_snapshot(),
            cgroup: cgroup_snapshot(),
            byte_cache_used,
            abort_requested: self.inner.abort.load(Ordering::Relaxed),
        }
    }
}

pub enum ProgressEvent {
    ArchiveBatchComplete,
    CreateBsaComplete,
    Other,
}

pub struct ArchiveBatchGuard {
    diagnostics: WabbajackDiagnostics,
    archive_hash: u64,
    started: Instant,
}

pub struct PatchGuard {
    diagnostics: WabbajackDiagnostics,
    patch_id: String,
    started: Instant,
}

impl Drop for ArchiveBatchGuard {
    fn drop(&mut self) {
        let mut state = self.diagnostics.inner.state.lock();
        state
            .active_archive_batches
            .retain(|batch| batch.archive_hash != format!("{:016x}", self.archive_hash));
        info!(
            archive_hash = %format!("{:016x}", self.archive_hash),
            elapsed_ms = self.started.elapsed().as_millis(),
            "archive batch finished"
        );
    }
}

impl Drop for PatchGuard {
    fn drop(&mut self) {
        let mut state = self.diagnostics.inner.state.lock();
        state
            .active_patches
            .retain(|patch| patch.patch_id != self.patch_id);
        info!(
            patch_id = %self.patch_id,
            elapsed_ms = self.started.elapsed().as_millis(),
            "patch finished"
        );
    }
}

mod snapshot;
pub use snapshot::{cgroup_memory_pressure_high, current_process_snapshot, memory_is_saturated};
use snapshot::{cgroup_snapshot, process_snapshot, unix_ms};

#[cfg(test)]
mod tests;
