use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinHandle;
use tracing::{info, warn};

use modde_core::manifest::wabbajack::{
    ArchiveInstallBatch, ArchiveState, DownloadDirective, InstallDirective, WabbajackManifest,
};

use crate::cache::{ByteCacheKey, ByteLruCache};
use crate::decompress::{ArchiveBatchExtractor, ArchiveInput, ArchiveRequest, ArchiveRequestKind};
use crate::traits::{AnySource, DownloadSource};

use super::bsa_repack;
use super::diagnostics::{
    ArchiveBatchRecord, ProcessSnapshot, ProgressEvent, WabbajackDiagnostics,
    cgroup_memory_pressure_high, current_process_snapshot,
};
use super::impact::{MissingArchiveImpact, MissingArchivePolicy};
use super::inline::InlineSource;
use super::patcher;
use super::staging::StagingStore;

/// Default maximum number of concurrent downloads.
const DEFAULT_CONCURRENCY: usize = 4;
const APPLY_STATE_VERSION: u32 = 1;
const DEFAULT_ARCHIVE_MEMORY_MAX_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArchiveRetentionPolicy {
    #[default]
    Keep,
    PruneApplied,
    Auto,
}

impl ArchiveRetentionPolicy {
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var("MODDE_ARCHIVE_RETENTION")
            .ok()
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("prune-applied" | "prune" | "delete") => Self::PruneApplied,
            Some("auto") => Self::Auto,
            _ => Self::Keep,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ArchiveBatchSentinel {
    pipeline_version: u32,
    archive_hash: u64,
    archive_size_bytes: u64,
    directive_indices: Vec<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateBsaSentinel {
    pipeline_version: u32,
    directive_index: usize,
    temp_id: String,
    to: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct VerifiedArchiveSidecar {
    pipeline_version: u32,
    archive_hash: u64,
    size_bytes: u64,
    modified_unix_ms: u128,
    verified_unix_ms: u128,
}

#[derive(Debug, Clone)]
enum TrustedArchive {
    Path(PathBuf),
    Bytes {
        label: String,
        bytes: Bytes,
        fallback_path: PathBuf,
    },
}

#[derive(Debug, Default, Clone, Copy)]
struct ArchiveTrustStats {
    sidecar_hit: bool,
    streamed_hash_bytes: u64,
    memory_archive_hit: bool,
    disk_fallback: bool,
}

#[derive(Debug, Default, Clone, Copy)]
struct StagingAdoptionSummary {
    archive_batches: usize,
    create_bsa: usize,
}

struct DiagnosticsHeartbeatGuard(Option<JoinHandle<()>>);

impl Drop for DiagnosticsHeartbeatGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

/// Progress update sent during installation.
#[derive(Debug, Clone)]
pub enum InstallProgress {
    Starting {
        total_downloads: usize,
    },
    Downloading {
        name: String,
        bytes: u64,
        total: u64,
    },
    DownloadComplete {
        name: String,
    },
    Verifying {
        name: String,
    },
    Applying {
        directive_index: usize,
        total: usize,
    },
    Patching {
        name: String,
    },
    CreatingBSA {
        name: String,
    },
    InlineFile {
        name: String,
    },
    StagingAdopted {
        archive_batches: usize,
        create_bsa: usize,
    },
    Complete,
    Failed {
        error: String,
    },
}

/// Orchestrate a full Wabbajack install pipeline.
pub struct WabbajackInstaller {
    manifest: WabbajackManifest,
    /// Path to the `.wabbajack` zip file (needed for `InlineFile` and `PatchedFromArchive` data).
    wabbajack_path: PathBuf,
    store_dir: PathBuf,
    staging_dir: PathBuf,
    game_dir: Option<PathBuf>,
    sources: Arc<Vec<AnySource>>,
    concurrency: usize,
    /// When true, per-archive download and per-directive apply failures are logged
    /// rather than fatal. Used for very large modlists where a handful of archives
    /// require manual intervention (deleted Nexus mods, rate-limited cloud hosts)
    /// but the rest of the modlist should still install.
    continue_on_error: bool,
    byte_cache: Arc<ByteLruCache>,
    diagnostics: Option<WabbajackDiagnostics>,
    archive_retention: ArchiveRetentionPolicy,
    missing_archive_policy: MissingArchivePolicy,
    archive_memory_max_bytes: u64,
}

impl WabbajackInstaller {
    #[must_use]
    pub fn new(
        manifest: WabbajackManifest,
        wabbajack_path: PathBuf,
        store_dir: PathBuf,
        staging_dir: PathBuf,
    ) -> Self {
        Self {
            manifest,
            wabbajack_path,
            store_dir,
            staging_dir,
            game_dir: None,
            sources: Arc::new(Vec::new()),
            concurrency: DEFAULT_CONCURRENCY,
            continue_on_error: false,
            byte_cache: Arc::new(ByteLruCache::from_env()),
            diagnostics: None,
            archive_retention: ArchiveRetentionPolicy::from_env(),
            missing_archive_policy: MissingArchivePolicy::Fail,
            archive_memory_max_bytes: std::env::var("MODDE_ARCHIVE_MEMORY_MAX_BYTES")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(DEFAULT_ARCHIVE_MEMORY_MAX_BYTES),
        }
    }

    /// When set, log per-archive failures rather than aborting the install.
    pub fn set_continue_on_error(&mut self, value: bool) {
        self.continue_on_error = value;
    }

    pub fn set_diagnostics(&mut self, diagnostics: WabbajackDiagnostics) {
        self.diagnostics = Some(diagnostics);
    }

    pub fn set_archive_retention(&mut self, policy: ArchiveRetentionPolicy) {
        self.archive_retention = policy;
    }

    pub fn set_missing_archive_policy(&mut self, policy: MissingArchivePolicy) {
        self.missing_archive_policy = policy;
    }

    /// Set the game install directory used by Wabbajack game-file sources.
    pub fn set_game_dir(&mut self, game_dir: PathBuf) {
        self.game_dir = Some(game_dir);
    }

    /// Register a download source implementation.
    pub fn add_source(&mut self, source: AnySource) {
        Arc::get_mut(&mut self.sources)
            .expect("add_source must be called before install")
            .push(source);
    }

    /// Set the maximum number of concurrent downloads.
    pub fn set_concurrency(&mut self, concurrency: usize) {
        self.concurrency = concurrency.max(1);
    }

    /// Run the full install pipeline, sending progress updates via channel.
    pub async fn install(&self, progress_tx: mpsc::UnboundedSender<InstallProgress>) -> Result<()> {
        let staging_store = StagingStore::new(&self.staging_dir);
        staging_store.prepare_resumable().await?;
        let _diagnostics_heartbeat = self.diagnostics.as_ref().map(|diagnostics| {
            let cache = Arc::clone(&self.byte_cache);
            DiagnosticsHeartbeatGuard(Some(
                diagnostics.spawn_heartbeat(Arc::new(move || cache.bytes_used())),
            ))
        });
        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.set_phase("preflight");
            diagnostics.record_progress(ProgressEvent::Other);
        }

        let downloads = self.manifest.download_directives();
        let installs = self.manifest.install_directives();

        self.validate_game_file_sources()?;
        self.verify_game_file_sources().await?;
        self.validate_download_sources(&downloads)?;
        self.preflight_authored_files().await?;
        self.check_diagnostics_abort()?;

        progress_tx
            .send(InstallProgress::Starting {
                total_downloads: downloads.len(),
            })
            .ok();

        // Apply install directives.
        //
        // FromArchive / InlineFile / PatchedFromArchive directives each touch a
        // unique destination path inside the staging tree, so they can run in
        // parallel. CreateBSA is held back to a second pass because it depends
        // on the staging directory being fully populated for its `temp_id`
        // bucket.
        let total = installs.len();
        let progress_for_parallel = progress_tx.clone();
        // Apply concurrency.
        //
        // Bsdiff patches and large archive extractions can each peak at
        // hundreds of MB of resident memory; multiplied by a fixed worker
        // count this can swamp swap on machines that are otherwise healthy.
        //
        // We use a host-RAM-aware admission gate (`memory-admission` crate) so
        // workers self-throttle when the system runs hot, and a generous
        // structural cap on `buffer_unordered` so the gate has waiters to
        // release once memory frees up. `MODDE_APPLY_MAX_IN_FLIGHT` overrides
        // the cap for benchmarking; `MODDE_APPLY_RAM_FRACTION` tunes the
        // throttle threshold (default 0.80).
        let max_in_flight = std::env::var("MODDE_APPLY_MAX_IN_FLIGHT")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|&n| n > 0)
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(std::num::NonZeroUsize::get)
                    .unwrap_or(8)
                    .clamp(1, 4)
            });
        // RAM fraction defaults are aggressive: the recommended deployment is
        // inside a cgroup (`systemd-run --user --scope -p MemoryHigh=…`) so
        // the kernel is the hard ceiling. The application-level gate just
        // prevents pathological in-process spikes.
        let max_ram_fraction = std::env::var("MODDE_APPLY_RAM_FRACTION")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|&v| v > 0.0 && v <= 1.0)
            .unwrap_or(0.85);
        let safety_reserve_bytes = std::env::var("MODDE_APPLY_SAFETY_RESERVE_GIB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map_or(2 * (1_u64 << 30), |g| g * (1_u64 << 30));
        // Page-cache throttle is an extra defence against the I/O-induced
        // thrash failure mode (huge readahead folios + slow reclaim). Default
        // is permissive since fadvise(DONTNEED) on archives already prevents
        // most readahead build-up; tighten it only if the safety reserve
        // alone is not enough.
        // Default page-cache check is OFF (1.0) — when modde runs inside a
        // cgroup MemoryHigh, the kernel handles page-cache pressure without
        // application-level help, and the in-application throttle just
        // misfires when the cache is hot from prior activity. Operators
        // running outside a cgroup can opt back in by setting
        // `MODDE_APPLY_PAGE_CACHE_FRACTION=0.6` or similar.
        let page_cache_fraction = std::env::var("MODDE_APPLY_PAGE_CACHE_FRACTION")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|&v: &f64| v > 0.0 && v <= 1.0)
            .unwrap_or(1.0);
        let weighted_config = memory_admission::weighted::WeightedConfig {
            base: memory_admission::Config {
                max_ram_fraction,
                ..memory_admission::Config::default()
            }
            .validate()
            .expect("hard-coded default memory-admission config is valid"),
            safety_reserve_bytes,
            // Anything beyond this size runs alone — typical Wabbajack lists
            // top out around 8-12 GiB single archives.
            max_single_weight_bytes: 4 * (1_u64 << 30),
            max_page_cache_fraction: page_cache_fraction,
            ..memory_admission::weighted::WeightedConfig::default()
        };
        // Prefer the cgroup-v2 provider when modde runs inside a memory-bounded
        // scope (e.g. `systemd-run --user --scope -p MemoryHigh=…`). The host
        // `MemAvailable` view is the wrong signal in that case — the cgroup
        // gets throttled long before host memory runs out, and a host-level
        // gate happily admits work that the kernel then forces into swap.
        let provider: memory_admission::provider::SharedMemoryProvider = {
            #[cfg(target_os = "linux")]
            {
                memory_admission::providers::CgroupV2Provider::shared()
                    .unwrap_or_else(|| memory_admission::providers::ProcMeminfoProvider::shared())
            }
            #[cfg(not(target_os = "linux"))]
            {
                memory_admission::providers::ProcMeminfoProvider::shared()
            }
        };
        let gate =
            memory_admission::weighted::AsyncWeightedAdmissionGate::new(weighted_config, provider);

        // Build a hash → size table from the manifest so per-directive weight
        // estimation is a constant-time lookup.
        let archive_size_by_hash: HashMap<u64, u64> = self
            .manifest
            .archives
            .iter()
            .map(|a| (a.hash, a.size))
            .collect();
        let archive_size_by_hash = Arc::new(archive_size_by_hash);

        info!(
            max_in_flight,
            max_ram_fraction,
            safety_reserve_gib = safety_reserve_bytes / (1 << 30),
            page_cache_fraction,
            byte_cache_used = self.byte_cache.bytes_used(),
            "starting weighted parallel apply pass"
        );
        let inline_source = installs
            .iter()
            .any(|directive| {
                matches!(
                    directive,
                    InstallDirective::InlineFile { .. }
                        | InstallDirective::PatchedFromArchive { .. }
                )
            })
            .then(|| InlineSource::open(&self.wabbajack_path).map(Arc::new))
            .transpose()?;

        // Phase 1 (per-archive batching): group FromArchive and
        // PatchedFromArchive directives by archive_hash so each archive is
        // touched at most once concurrently. Phase 2's native decoder can
        // then turn each batch into a single archive reader session. InlineFile
        // directives are independent of archives so they fan out separately.
        let impact = MissingArchiveImpact::analyze(&self.manifest, &self.store_dir);
        let skip_plan = impact.skip_plan(&self.manifest, self.missing_archive_policy);
        if !skip_plan.is_empty() {
            warn!(
                policy = ?self.missing_archive_policy,
                missing_archives = impact.missing_archives.len(),
                skipped_directives = skip_plan.skipped_directives.len(),
                skipped_mod_roots = skip_plan.skipped_mod_roots.len(),
                "omitting Wabbajack outputs for missing optional archives"
            );
        }

        let mut inline_directives: Vec<(usize, &InstallDirective)> = Vec::new();
        for (i, directive) in installs.iter().enumerate() {
            if matches!(directive, InstallDirective::InlineFile { .. })
                && !skip_plan.should_skip_directive(i, directive)
            {
                inline_directives.push((i, directive));
            }
        }
        let mut archive_batches = self.manifest.install_directives_grouped_by_archive();
        if !skip_plan.is_empty() {
            for batch in &mut archive_batches {
                batch.directives.retain(|indexed| {
                    !skip_plan.should_skip_directive(indexed.directive_index, &indexed.directive)
                });
            }
            archive_batches.retain(|batch| !batch.directives.is_empty());
        }
        let archive_batch_count = archive_batches.len();
        let patch_max_in_flight = std::env::var("MODDE_PATCH_MAX_IN_FLIGHT")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|&n| n > 0)
            .unwrap_or(1);
        let patch_batch_gate = Arc::new(Semaphore::new(patch_max_in_flight));
        info!(
            archive_batch_count,
            inline_directive_count = inline_directives.len(),
            patch_max_in_flight,
            "grouped apply directives by archive"
        );
        let adoption = self
            .adopt_existing_staging(&archive_batches, &installs)
            .await?;
        self.check_diagnostics_abort()?;
        if adoption.archive_batches > 0 || adoption.create_bsa > 0 {
            info!(
                archive_batches = adoption.archive_batches,
                create_bsa = adoption.create_bsa,
                "adopted existing Wabbajack staging outputs"
            );
            progress_tx
                .send(InstallProgress::StagingAdopted {
                    archive_batches: adoption.archive_batches,
                    create_bsa: adoption.create_bsa,
                })
                .ok();
        }

        // Inline-file pass — these have no archive contention so we can
        // saturate the gate's RAM budget without backpressuring on archives.
        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.set_phase("apply-inline");
        }
        let inline_results: Vec<(usize, Result<()>)> = stream::iter(inline_directives)
            .map(|(i, directive)| {
                let progress_tx = progress_for_parallel.clone();
                let gate = gate.clone();
                let sizes = Arc::clone(&archive_size_by_hash);
                let inline_source = inline_source.clone();
                async move {
                    let weight = estimate_directive_weight(directive, &sizes);
                    let _permit = gate.acquire(weight).await;
                    if let Err(e) = self.check_diagnostics_abort() {
                        return (i, Err(e));
                    }
                    progress_tx
                        .send(InstallProgress::Applying {
                            directive_index: i,
                            total,
                        })
                        .ok();
                    let InstallDirective::InlineFile { source_data_id, to } = directive else {
                        unreachable!("inline_directives only contains InlineFile");
                    };
                    progress_tx
                        .send(InstallProgress::InlineFile { name: to.clone() })
                        .ok();
                    let result = match inline_source.as_deref() {
                        Some(inline_source) => {
                            self.apply_inline_file(inline_source, source_data_id, to)
                                .await
                        }
                        None => Err(anyhow::anyhow!("inline source was not initialized")),
                    };
                    (i, result)
                }
            })
            .buffer_unordered(max_in_flight)
            .collect()
            .await;

        // Archive-batch pass — one task per archive_hash, each task processes
        // its own directives sequentially. Concurrency is over batches, so
        // the same archive is never decompressed twice in parallel.
        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.set_phase("apply-archive-batches");
        }
        let archive_results: Vec<Vec<(usize, Result<()>)>> = stream::iter(archive_batches)
            .map(|batch| {
                let progress_tx = progress_for_parallel.clone();
                let gate = gate.clone();
                let inline_source = inline_source.clone();
                let patch_batch_gate = Arc::clone(&patch_batch_gate);
                async move {
                    let _patch_permit = if archive_batch_has_patch(&batch) {
                        Some(
                            patch_batch_gate
                                .acquire_owned()
                                .await
                                .expect("semaphore open"),
                        )
                    } else {
                        None
                    };
                    // The batch's whole-archive memory cost is the archive
                    // size itself (decompression working set, conservatively
                    // half), regardless of how many directives it serves.
                    let archive_weight = estimate_archive_batch_weight(&batch);
                    let _permit = gate.acquire(archive_weight).await;
                    if let Err(e) = self.check_diagnostics_abort() {
                        return batch
                            .directives
                            .into_iter()
                            .map(|directive| {
                                (directive.directive_index, Err(anyhow::anyhow!("{e:#}")))
                            })
                            .collect();
                    }

                    for indexed in &batch.directives {
                        let i = indexed.directive_index;
                        progress_tx
                            .send(InstallProgress::Applying {
                                directive_index: i,
                                total,
                            })
                            .ok();
                    }
                    self.apply_archive_batch(batch, inline_source.as_deref(), &progress_tx)
                        .await
                }
            })
            .buffer_unordered(max_in_flight)
            .collect()
            .await;

        for (i, result) in inline_results
            .into_iter()
            .chain(archive_results.into_iter().flatten())
        {
            if let Err(e) = result {
                if self.continue_on_error {
                    warn!(directive_index = i, "skipping directive after error: {e:#}");
                } else {
                    return Err(e);
                }
            }
        }

        // Second pass: CreateBSA (depends on staged temp directories).
        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.set_phase("create-bsa");
        }
        for (i, directive) in installs.iter().enumerate() {
            self.check_diagnostics_abort()?;
            let InstallDirective::CreateBSA {
                temp_id,
                to,
                file_states,
            } = directive
            else {
                continue;
            };
            if skip_plan.should_skip_create_bsa(i, temp_id, to) {
                warn!(
                    directive_index = i,
                    temp_id,
                    to,
                    "skipping CreateBSA because an optional upstream archive is missing"
                );
                continue;
            }
            if self.create_bsa_sentinel_valid(i, temp_id, to).await {
                continue;
            }
            progress_tx
                .send(InstallProgress::Applying {
                    directive_index: i,
                    total,
                })
                .ok();
            progress_tx
                .send(InstallProgress::CreatingBSA { name: to.clone() })
                .ok();
            if let Err(e) = self.apply_create_bsa(temp_id, to, file_states).await {
                if self.continue_on_error {
                    warn!(directive_index = i, "skipping CreateBSA after error: {e:#}");
                } else {
                    return Err(e);
                }
            } else {
                self.write_create_bsa_sentinel(i, temp_id, to).await?;
                if let Some(diagnostics) = &self.diagnostics {
                    diagnostics.record_progress(ProgressEvent::CreateBsaComplete);
                }
            }
        }

        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.set_phase("compress-staging");
        }
        self.check_diagnostics_abort()?;
        let summary = staging_store
            .compress_eligible_files(self.concurrency)
            .await?;
        info!(
            compressed_files = summary.compressed_files,
            skipped_files = summary.skipped_files,
            original_bytes = summary.original_bytes,
            compressed_bytes = summary.compressed_bytes,
            "compressed Wabbajack staging files"
        );

        progress_tx.send(InstallProgress::Complete).ok();
        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.set_phase("complete");
            diagnostics.record_progress(ProgressEvent::Other);
        }
        info!("wabbajack installation complete");
        Ok(())
    }

    async fn ensure_archive_trusted(
        &self,
        archive_hash: u64,
        progress_tx: &mpsc::UnboundedSender<InstallProgress>,
    ) -> Result<(TrustedArchive, ArchiveTrustStats)> {
        let path = archive_path(&self.store_dir, &archive_hash);
        let mut stats = ArchiveTrustStats::default();

        if path.exists() {
            if self.verified_sidecar_valid(&path, archive_hash).await {
                stats.sidecar_hit = true;
            } else {
                let metadata = tokio::fs::metadata(&path).await?;
                progress_tx
                    .send(InstallProgress::Verifying {
                        name: format!("{archive_hash:016x}"),
                    })
                    .ok();
                modde_core::hash::verify_xxh64(&path, archive_hash)
                    .await
                    .with_context(|| {
                        format!("hash verification failed for archive {archive_hash:016x}")
                    })?;
                stats.streamed_hash_bytes = metadata.len();
                self.write_verified_sidecar(&path, archive_hash).await?;
            }
            return self
                .trusted_archive_for_path(path, archive_hash, stats)
                .await;
        }

        let Some(directive) = self
            .manifest
            .download_directives()
            .into_iter()
            .find(|directive| directive.hash() == archive_hash)
        else {
            bail!(
                "archive {archive_hash:016x} is not present in store and has no download directive"
            );
        };

        let name = directive.display_name().into_owned();
        if matches!(directive, DownloadDirective::Manual { .. }) {
            bail!("manual archive {name} ({archive_hash:016x}) is missing from the store");
        }

        let Some(source) = self
            .sources
            .iter()
            .find(|source| source.can_handle(&directive))
        else {
            bail!("no download source registered for archive {name} ({archive_hash:016x})");
        };

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let handle = source
            .resolve(&directive)
            .await
            .with_context(|| format!("failed to resolve download: {name}"))?;
        progress_tx
            .send(InstallProgress::Downloading {
                name: name.clone(),
                bytes: 0,
                total: handle.size_hint.unwrap_or(0),
            })
            .ok();
        source
            .download(handle, &path)
            .await
            .with_context(|| format!("failed to download: {name}"))?;
        self.write_verified_sidecar(&path, archive_hash).await?;
        progress_tx
            .send(InstallProgress::DownloadComplete { name })
            .ok();
        self.trusted_archive_for_path(path, archive_hash, stats)
            .await
    }

    async fn trusted_archive_for_path(
        &self,
        path: PathBuf,
        archive_hash: u64,
        mut stats: ArchiveTrustStats,
    ) -> Result<(TrustedArchive, ArchiveTrustStats)> {
        let metadata = tokio::fs::metadata(&path).await?;
        if metadata.len() <= self.archive_memory_max_bytes
            && archive_path_is_memory_supported(&path).await
        {
            let bytes = tokio::fs::read(&path).await?;
            stats.memory_archive_hit = true;
            return Ok((
                TrustedArchive::Bytes {
                    label: format!("{archive_hash:016x}.archive"),
                    bytes: Bytes::from(bytes),
                    fallback_path: path,
                },
                stats,
            ));
        }
        stats.disk_fallback = true;
        Ok((TrustedArchive::Path(path), stats))
    }

    async fn verified_sidecar_valid(&self, path: &Path, archive_hash: u64) -> bool {
        let Ok(metadata) = tokio::fs::metadata(path).await else {
            return false;
        };
        let Ok(data) = tokio::fs::read_to_string(verified_sidecar_path(path)).await else {
            return false;
        };
        let Ok(sidecar) = serde_json::from_str::<VerifiedArchiveSidecar>(&data) else {
            return false;
        };
        sidecar.pipeline_version == APPLY_STATE_VERSION
            && sidecar.archive_hash == archive_hash
            && sidecar.size_bytes == metadata.len()
            && sidecar.modified_unix_ms == metadata_modified_unix_ms(&metadata)
    }

    async fn write_verified_sidecar(&self, path: &Path, archive_hash: u64) -> Result<()> {
        let metadata = tokio::fs::metadata(path).await?;
        let sidecar = VerifiedArchiveSidecar {
            pipeline_version: APPLY_STATE_VERSION,
            archive_hash,
            size_bytes: metadata.len(),
            modified_unix_ms: metadata_modified_unix_ms(&metadata),
            verified_unix_ms: unix_ms(),
        };
        tokio::fs::write(
            verified_sidecar_path(path),
            serde_json::to_vec_pretty(&sidecar)?,
        )
        .await?;
        Ok(())
    }

    async fn maybe_prune_archive(&self, batch: &ArchiveInstallBatch) -> Result<u64> {
        if self.archive_retention == ArchiveRetentionPolicy::Keep {
            return Ok(0);
        }
        if self.game_file_source_path(batch.archive_hash)?.is_some() {
            return Ok(0);
        }
        let path = archive_path(&self.store_dir, &batch.archive_hash);
        if !path.exists() {
            return Ok(0);
        }
        if self.archive_retention == ArchiveRetentionPolicy::Auto
            && (batch.archive_size_bytes > self.archive_memory_max_bytes
                || batch.directives.len() > 1)
        {
            return Ok(0);
        }
        let size = tokio::fs::metadata(&path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        tokio::fs::remove_file(&path).await?;
        let _ = tokio::fs::remove_file(verified_sidecar_path(&path)).await;
        Ok(size)
    }

    /// Extract a file from a downloaded archive and place it in the staging directory.
    async fn apply_from_archive(
        &self,
        archive_hash: u64,
        from: &str,
        to: &str,
        expected_size: u64,
    ) -> Result<()> {
        // Validate paths against traversal attacks
        validate_archive_entry(from)?;
        validate_archive_entry(to)?;

        let output_path = self.staging_dir.join(normalize_path(to));

        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        if let Some(source) = self.game_file_source_path(archive_hash)? {
            if game_file_source_is_whole_file(&source.path, &source.rel_path, from) {
                modde_core::link::link_or_copy(&source.path, &output_path).await?;
            } else {
                let archive_path = source.path;
                let from = from.to_string();
                tokio::task::spawn_blocking(move || {
                    ArchiveBatchExtractor::extract_selected(
                        &archive_path,
                        &[ArchiveRequest {
                            directive_index: 0,
                            from,
                            inner_path: None,
                            kind: ArchiveRequestKind::WriteFile {
                                to: output_path,
                                expected_size: (expected_size > 0).then_some(expected_size),
                            },
                        }],
                    )
                })
                .await??;
            }
            info!(from = %from, to = %to, "extracted game-file source");
            return Ok(());
        }

        // Extract the specific file from a downloaded archive or resolve a
        // Wabbajack game-file source from the local game install.
        let data = self
            .read_archive_source(archive_hash, from)
            .await
            .with_context(|| {
                format!("failed to extract '{from}' from archive {archive_hash:016x}")
            })?;

        tokio::fs::write(&output_path, &data).await?;

        info!(from = %from, to = %to, "extracted file from archive");
        Ok(())
    }

    async fn apply_archive_batch(
        &self,
        batch: ArchiveInstallBatch,
        inline_source: Option<&InlineSource>,
        progress_tx: &mpsc::UnboundedSender<InstallProgress>,
    ) -> Vec<(usize, Result<()>)> {
        if let Err(e) = self.check_diagnostics_abort() {
            return batch
                .directives
                .into_iter()
                .map(|directive| (directive.directive_index, Err(anyhow::anyhow!("{e:#}"))))
                .collect();
        }
        if self.archive_batch_sentinel_valid(&batch).await {
            return batch
                .directives
                .into_iter()
                .map(|directive| (directive.directive_index, Ok(())))
                .collect();
        }

        let mut native_requests = Vec::new();
        let mut patch_directives = HashMap::new();
        let mut results = Vec::with_capacity(batch.directives.len());
        let batch_started = Instant::now();
        let mut extraction_ms = 0;
        let mut patch_ms = 0;
        let mut trust_check_ms = 0;
        let mut prune_ms = 0;
        let mut pruned_bytes = 0;
        let mut trust_stats = ArchiveTrustStats::default();
        let mut extracted_patch_source_bytes = 0;
        let byte_cache_used_before = self.byte_cache.bytes_used();
        let process_before = current_process_snapshot();
        let patch_count = batch
            .directives
            .iter()
            .filter(|indexed| {
                matches!(
                    indexed.directive,
                    InstallDirective::PatchedFromArchive { .. }
                )
            })
            .count();
        let _batch_guard = self.diagnostics.as_ref().map(|diagnostics| {
            diagnostics.start_archive_batch(
                batch.archive_hash,
                batch.directives.len(),
                patch_count,
                batch.archive_size_bytes,
            )
        });
        info!(
            archive_hash = %format!("{:016x}", batch.archive_hash),
            archive_size_bytes = batch.archive_size_bytes,
            directives = batch.directives.len(),
            patch_directives = patch_count,
            byte_cache_used = self.byte_cache.bytes_used(),
            "starting archive apply batch"
        );

        let trusted_archive = if self
            .game_file_source_path(batch.archive_hash)
            .ok()
            .flatten()
            .is_some()
        {
            None
        } else {
            let trust_started = Instant::now();
            let trusted = self
                .ensure_archive_trusted(batch.archive_hash, progress_tx)
                .await;
            trust_check_ms = trust_started.elapsed().as_millis();
            match trusted {
                Ok((archive, stats)) => {
                    trust_stats = stats;
                    Some(archive)
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    let results = batch
                        .directives
                        .iter()
                        .map(|indexed| {
                            (
                                indexed.directive_index,
                                Err(anyhow::anyhow!("archive trust check failed: {msg}")),
                            )
                        })
                        .collect::<Vec<_>>();
                    self.write_archive_batch_metrics(
                        &batch,
                        patch_count,
                        batch_started,
                        trust_check_ms,
                        extraction_ms,
                        patch_ms,
                        prune_ms,
                        extracted_patch_source_bytes,
                        trust_stats,
                        pruned_bytes,
                        byte_cache_used_before,
                        process_before,
                        &results,
                    )
                    .await;
                    return results;
                }
            }
        };

        for indexed in &batch.directives {
            if let Err(e) = self.check_diagnostics_abort() {
                results.push((indexed.directive_index, Err(e)));
                continue;
            }
            match &indexed.directive {
                InstallDirective::FromArchive {
                    archive_hash,
                    from,
                    inner_path,
                    to,
                    size,
                } => match self.game_file_source_path(*archive_hash) {
                    Ok(Some(_)) => {
                        results.push((
                            indexed.directive_index,
                            self.apply_from_archive(*archive_hash, from, to, *size)
                                .await,
                        ));
                    }
                    Ok(None) => {
                        // Validate against path traversal / zip-slip before joining
                        // onto the staging dir (mirrors apply_from_archive). The batch
                        // extractor validates `from`/`inner_path` but never the `to`
                        // destination, so a malicious manifest `to` must be rejected here.
                        if let Err(e) =
                            validate_archive_entry(from).and_then(|()| validate_archive_entry(to))
                        {
                            results.push((indexed.directive_index, Err(e)));
                            continue;
                        }
                        let output_path = self.staging_dir.join(normalize_path(to));
                        native_requests.push(ArchiveRequest {
                            directive_index: indexed.directive_index,
                            from: from.clone(),
                            inner_path: inner_path.clone(),
                            kind: ArchiveRequestKind::WriteFile {
                                to: output_path,
                                expected_size: (*size > 0).then_some(*size),
                            },
                        });
                    }
                    Err(e) => results.push((indexed.directive_index, Err(e))),
                },
                InstallDirective::PatchedFromArchive {
                    archive_hash,
                    from,
                    inner_path,
                    to,
                    patch_id,
                    size,
                } => {
                    progress_tx
                        .send(InstallProgress::Patching { name: to.clone() })
                        .ok();
                    match self.game_file_source_path(*archive_hash) {
                        Ok(Some(_)) => {
                            let Some(inline_source) = inline_source else {
                                results.push((
                                    indexed.directive_index,
                                    Err(anyhow::anyhow!("inline source was not initialized")),
                                ));
                                continue;
                            };
                            results.push((
                                indexed.directive_index,
                                self.apply_patched_from_archive(
                                    inline_source,
                                    *archive_hash,
                                    from,
                                    to,
                                    patch_id,
                                    *size,
                                )
                                .await,
                            ));
                        }
                        Ok(None) => {
                            if let Some(bytes) = self.patch_source_from_cache(*archive_hash, from) {
                                let Some(inline_source) = inline_source else {
                                    results.push((
                                        indexed.directive_index,
                                        Err(anyhow::anyhow!("inline source was not initialized")),
                                    ));
                                    continue;
                                };
                                results.push((
                                    indexed.directive_index,
                                    self.write_patched_output(
                                        inline_source,
                                        bytes,
                                        to,
                                        patch_id,
                                        *size,
                                    )
                                    .await,
                                ));
                            } else {
                                patch_directives.insert(
                                    indexed.directive_index,
                                    (
                                        *archive_hash,
                                        from.clone(),
                                        inner_path.clone(),
                                        to.clone(),
                                        patch_id.clone(),
                                        *size,
                                    ),
                                );
                                native_requests.push(ArchiveRequest {
                                    directive_index: indexed.directive_index,
                                    from: from.clone(),
                                    inner_path: inner_path.clone(),
                                    kind: ArchiveRequestKind::Bytes,
                                });
                            }
                        }
                        Err(e) => results.push((indexed.directive_index, Err(e))),
                    }
                }
                _ => unreachable!("archive batch contains only archive-backed directives"),
            }
        }

        if native_requests.is_empty() {
            if results.iter().all(|(_, result)| result.is_ok())
                && let Err(e) = self.write_archive_batch_sentinel(&batch).await
            {
                let msg = format!("{e:#}");
                return results
                    .into_iter()
                    .map(|(idx, result)| {
                        if result.is_ok() {
                            (
                                idx,
                                Err(anyhow::anyhow!(
                                    "failed to write archive batch sentinel: {msg}"
                                )),
                            )
                        } else {
                            (idx, result)
                        }
                    })
                    .collect();
            }
            if results.iter().all(|(_, result)| result.is_ok())
                && let Some(diagnostics) = &self.diagnostics
            {
                diagnostics.record_progress(ProgressEvent::ArchiveBatchComplete);
            }
            self.write_archive_batch_metrics(
                &batch,
                patch_count,
                batch_started,
                trust_check_ms,
                extraction_ms,
                patch_ms,
                prune_ms,
                extracted_patch_source_bytes,
                trust_stats,
                pruned_bytes,
                byte_cache_used_before,
                process_before,
                &results,
            )
            .await;
            return results;
        }

        let mut write_requests = Vec::new();
        let mut patch_requests = Vec::new();
        for request in native_requests {
            if patch_directives.contains_key(&request.directive_index) {
                patch_requests.push(request);
            } else {
                write_requests.push(request);
            }
        }

        if !write_requests.is_empty() {
            let extraction_started = Instant::now();
            let native_result =
                extract_trusted_archive_requests(trusted_archive.clone(), write_requests.clone())
                    .await;
            extraction_ms += extraction_started.elapsed().as_millis();
            match native_result {
                Ok(_) => {
                    results.extend(
                        write_requests
                            .iter()
                            .map(|request| (request.directive_index, Ok(()))),
                    );
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    results.extend(write_requests.iter().map(|request| {
                        (
                            request.directive_index,
                            Err(anyhow::anyhow!("archive batch extraction failed: {msg}")),
                        )
                    }));
                }
            }
        }

        let mut nested_patch_groups: HashMap<String, Vec<ArchiveRequest>> = HashMap::new();
        let mut plain_patch_requests = Vec::new();
        for request in patch_requests {
            if request.inner_path.is_some() {
                nested_patch_groups
                    .entry(normalize_path(&request.from).to_lowercase())
                    .or_default()
                    .push(request);
            } else {
                plain_patch_requests.push(request);
            }
        }

        let patch_chunk_size = archive_patch_chunk_size();
        for chunk in plain_patch_requests.chunks(patch_chunk_size) {
            let chunk_requests = chunk.to_vec();
            let extraction_started = Instant::now();
            let native_result =
                extract_trusted_archive_requests(trusted_archive.clone(), chunk_requests.clone())
                    .await;
            extraction_ms += extraction_started.elapsed().as_millis();
            let mut output = match native_result {
                Ok(output) => output,
                Err(e) => {
                    let msg = format!("{e:#}");
                    results.extend(chunk_requests.iter().map(|request| {
                        (
                            request.directive_index,
                            Err(anyhow::anyhow!("archive batch extraction failed: {msg}")),
                        )
                    }));
                    continue;
                }
            };

            for request in chunk_requests {
                if let Err(e) = self.check_diagnostics_abort() {
                    results.push((request.directive_index, Err(e)));
                    continue;
                }
                if let Some((archive_hash, from, inner_path, to, patch_id, size)) =
                    patch_directives.remove(&request.directive_index)
                {
                    let Some(inline_source) = inline_source else {
                        results.push((
                            request.directive_index,
                            Err(anyhow::anyhow!("inline source was not initialized")),
                        ));
                        continue;
                    };
                    let Some(bytes) = output.bytes.remove(&request.directive_index) else {
                        results.push((
                            request.directive_index,
                            Err(anyhow::anyhow!(
                                "patch source '{}' missing from extractor output",
                                inner_path.as_deref().unwrap_or(&from)
                            )),
                        ));
                        continue;
                    };
                    extracted_patch_source_bytes += bytes.len() as u64;
                    let bytes = self.cache_patch_source(archive_hash, &from, Bytes::from(bytes));
                    let patch_started = Instant::now();
                    results.push((
                        request.directive_index,
                        self.write_patched_output(inline_source, bytes, &to, &patch_id, size)
                            .await,
                    ));
                    patch_ms += patch_started.elapsed().as_millis();
                } else {
                    results.push((request.directive_index, Ok(())));
                }
            }
        }

        for group_requests in nested_patch_groups.into_values() {
            let Some(first_request) = group_requests.first() else {
                continue;
            };
            let outer_directive_index = first_request.directive_index;
            let outer_from = first_request.from.clone();
            let outer_request = ArchiveRequest {
                directive_index: outer_directive_index,
                from: outer_from.clone(),
                inner_path: None,
                kind: ArchiveRequestKind::Bytes,
            };
            let extraction_started = Instant::now();
            let outer_result =
                extract_trusted_archive_requests(trusted_archive.clone(), vec![outer_request])
                    .await;
            extraction_ms += extraction_started.elapsed().as_millis();
            let outer_bytes = match outer_result {
                Ok(mut output) => {
                    if let Some(bytes) = output.bytes.remove(&outer_directive_index) {
                        Bytes::from(bytes)
                    } else {
                        let msg = format!("nested archive '{outer_from}' missing from output");
                        results.extend(group_requests.iter().map(|request| {
                            (request.directive_index, Err(anyhow::anyhow!(msg.clone())))
                        }));
                        continue;
                    }
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    results.extend(group_requests.iter().map(|request| {
                        (
                            request.directive_index,
                            Err(anyhow::anyhow!(
                                "nested archive extraction failed for '{outer_from}': {msg}"
                            )),
                        )
                    }));
                    continue;
                }
            };

            let inner_requests = group_requests
                .iter()
                .filter_map(|request| {
                    Some(ArchiveRequest {
                        directive_index: request.directive_index,
                        from: request.inner_path.clone()?,
                        inner_path: None,
                        kind: ArchiveRequestKind::Bytes,
                    })
                })
                .collect::<Vec<_>>();

            let nested_temp =
                if outer_bytes.starts_with(b"BSA\0") || outer_bytes.starts_with(b"BTDX") {
                    let temp_result = (|| -> Result<tempfile::NamedTempFile> {
                        let mut temp = tempfile::NamedTempFile::new().with_context(|| {
                            format!("failed to create temp nested archive for {outer_from}")
                        })?;
                        std::io::Write::write_all(&mut temp, &outer_bytes).with_context(|| {
                            format!("failed to write temp nested archive for {outer_from}")
                        })?;
                        std::io::Write::flush(&mut temp).with_context(|| {
                            format!("failed to flush temp nested archive for {outer_from}")
                        })?;
                        Ok(temp)
                    })();
                    let Ok(temp) = temp_result else {
                        let msg = format!("{:#}", temp_result.unwrap_err());
                        results.extend(group_requests.iter().map(|request| {
                            (
                                request.directive_index,
                                Err(anyhow::anyhow!(
                                    "failed to stage nested archive '{outer_from}': {msg}"
                                )),
                            )
                        }));
                        continue;
                    };
                    Some(temp)
                } else {
                    None
                };

            for chunk in inner_requests.chunks(patch_chunk_size) {
                let chunk_requests = chunk.to_vec();
                let extraction_started = Instant::now();
                let native_result = if let Some(temp) = &nested_temp {
                    extract_archive_path_requests(temp.path().to_path_buf(), chunk_requests.clone())
                        .await
                } else {
                    extract_nested_archive_requests(
                        outer_from.clone(),
                        outer_bytes.clone(),
                        chunk_requests.clone(),
                    )
                    .await
                };
                extraction_ms += extraction_started.elapsed().as_millis();
                let mut output = match native_result {
                    Ok(output) => output,
                    Err(e) => {
                        let msg = format!("{e:#}");
                        results.extend(chunk_requests.iter().map(|request| {
                            (
                                request.directive_index,
                                Err(anyhow::anyhow!(
                                    "nested archive batch extraction failed: {msg}"
                                )),
                            )
                        }));
                        continue;
                    }
                };

                for request in chunk_requests {
                    if let Err(e) = self.check_diagnostics_abort() {
                        results.push((request.directive_index, Err(e)));
                        continue;
                    }
                    if let Some((archive_hash, from, inner_path, to, patch_id, size)) =
                        patch_directives.remove(&request.directive_index)
                    {
                        let Some(inline_source) = inline_source else {
                            results.push((
                                request.directive_index,
                                Err(anyhow::anyhow!("inline source was not initialized")),
                            ));
                            continue;
                        };
                        let Some(bytes) = output.bytes.remove(&request.directive_index) else {
                            results.push((
                                request.directive_index,
                                Err(anyhow::anyhow!(
                                    "patch source '{}' missing from nested extractor output",
                                    inner_path.as_deref().unwrap_or(&from)
                                )),
                            ));
                            continue;
                        };
                        extracted_patch_source_bytes += bytes.len() as u64;
                        let cache_key = inner_path.as_deref().unwrap_or(&from);
                        let bytes =
                            self.cache_patch_source(archive_hash, cache_key, Bytes::from(bytes));
                        let patch_started = Instant::now();
                        results.push((
                            request.directive_index,
                            self.write_patched_output(inline_source, bytes, &to, &patch_id, size)
                                .await,
                        ));
                        patch_ms += patch_started.elapsed().as_millis();
                    }
                }
            }
        }

        if results.iter().all(|(_, result)| result.is_ok())
            && let Err(e) = self.write_archive_batch_sentinel(&batch).await
        {
            let msg = format!("{e:#}");
            return results
                .into_iter()
                .map(|(idx, result)| {
                    if result.is_ok() {
                        (
                            idx,
                            Err(anyhow::anyhow!(
                                "failed to write archive batch sentinel: {msg}"
                            )),
                        )
                    } else {
                        (idx, result)
                    }
                })
                .collect();
        }
        if results.iter().all(|(_, result)| result.is_ok())
            && let Some(diagnostics) = &self.diagnostics
        {
            diagnostics.record_progress(ProgressEvent::ArchiveBatchComplete);
        }
        if results.iter().all(|(_, result)| result.is_ok()) {
            let prune_started = Instant::now();
            match self.maybe_prune_archive(&batch).await {
                Ok(bytes) => pruned_bytes = bytes,
                Err(e) => warn!(
                    archive_hash = %format!("{:016x}", batch.archive_hash),
                    "failed to prune applied archive: {e:#}"
                ),
            }
            prune_ms = prune_started.elapsed().as_millis();
        }
        self.write_archive_batch_metrics(
            &batch,
            patch_count,
            batch_started,
            trust_check_ms,
            extraction_ms,
            patch_ms,
            prune_ms,
            extracted_patch_source_bytes,
            trust_stats,
            pruned_bytes,
            byte_cache_used_before,
            process_before,
            &results,
        )
        .await;

        results
    }

    async fn write_archive_batch_metrics(
        &self,
        batch: &ArchiveInstallBatch,
        patch_count: usize,
        batch_started: Instant,
        trust_check_ms: u128,
        extraction_ms: u128,
        patch_ms: u128,
        prune_ms: u128,
        extracted_patch_source_bytes: u64,
        trust_stats: ArchiveTrustStats,
        pruned_bytes: u64,
        byte_cache_used_before: u64,
        process_before: ProcessSnapshot,
        results: &[(usize, Result<()>)],
    ) {
        let Some(diagnostics) = &self.diagnostics else {
            return;
        };
        let process_after = current_process_snapshot();
        let record = ArchiveBatchRecord {
            kind: "archive_batch".to_string(),
            unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            archive_hash: format!("{:016x}", batch.archive_hash),
            archive_size_bytes: batch.archive_size_bytes,
            directive_count: batch.directives.len(),
            patch_count,
            elapsed_ms: batch_started.elapsed().as_millis(),
            trust_check_ms,
            extraction_ms,
            patch_ms,
            prune_ms,
            extracted_patch_source_bytes,
            sidecar_hit: trust_stats.sidecar_hit,
            streamed_hash_bytes: trust_stats.streamed_hash_bytes,
            memory_archive_hit: trust_stats.memory_archive_hit,
            disk_archive_fallback: trust_stats.disk_fallback,
            pruned_bytes,
            byte_cache_used_before,
            byte_cache_used_after: self.byte_cache.bytes_used(),
            success_count: results.iter().filter(|(_, result)| result.is_ok()).count(),
            error_count: results.iter().filter(|(_, result)| result.is_err()).count(),
            first_error: results
                .iter()
                .find_map(|(_, result)| result.as_ref().err().map(|error| format!("{error:#}"))),
            rss_before_kib: process_before.vm_rss_kib,
            rss_after_kib: process_after.vm_rss_kib,
            swap_before_kib: process_before.vm_swap_kib,
            swap_after_kib: process_after.vm_swap_kib,
        };
        if let Err(e) = diagnostics.record_archive_batch(&record).await {
            warn!("failed to write Wabbajack archive batch diagnostics: {e:#}");
        }
    }

    async fn adopt_existing_staging(
        &self,
        archive_batches: &[ArchiveInstallBatch],
        installs: &[InstallDirective],
    ) -> Result<StagingAdoptionSummary> {
        let mut summary = StagingAdoptionSummary::default();

        for batch in archive_batches {
            if self.archive_batch_sentinel_valid(batch).await {
                continue;
            }
            if self.archive_batch_outputs_exist(batch).await {
                self.write_archive_batch_sentinel(batch).await?;
                summary.archive_batches += 1;
            }
        }

        for (directive_index, directive) in installs.iter().enumerate() {
            let InstallDirective::CreateBSA { temp_id, to, .. } = directive else {
                continue;
            };
            if self
                .create_bsa_sentinel_valid(directive_index, temp_id, to)
                .await
            {
                continue;
            }
            if StagingStore::new(&self.staging_dir)
                .logical_exists(to)
                .await
            {
                self.write_create_bsa_sentinel(directive_index, temp_id, to)
                    .await?;
                summary.create_bsa += 1;
            }
        }

        Ok(summary)
    }

    async fn archive_batch_sentinel_valid(&self, batch: &ArchiveInstallBatch) -> bool {
        let path = self.archive_batch_sentinel_path(batch.archive_hash);
        let Ok(data) = tokio::fs::read_to_string(&path).await else {
            return false;
        };
        let Ok(sentinel) = serde_json::from_str::<ArchiveBatchSentinel>(&data) else {
            return false;
        };
        let expected_indices = batch
            .directives
            .iter()
            .map(|d| d.directive_index)
            .collect::<Vec<_>>();
        sentinel.pipeline_version == APPLY_STATE_VERSION
            && sentinel.archive_hash == batch.archive_hash
            && sentinel.archive_size_bytes == batch.archive_size_bytes
            && sentinel.directive_indices == expected_indices
            && self.archive_batch_outputs_exist(batch).await
    }

    async fn archive_batch_outputs_exist(&self, batch: &ArchiveInstallBatch) -> bool {
        let staging_store = StagingStore::new(&self.staging_dir);
        for indexed in &batch.directives {
            let to = match &indexed.directive {
                InstallDirective::FromArchive { to, .. }
                | InstallDirective::PatchedFromArchive { to, .. } => to,
                _ => continue,
            };
            if !staging_store.logical_exists(to).await {
                return false;
            }
        }
        true
    }

    async fn write_archive_batch_sentinel(&self, batch: &ArchiveInstallBatch) -> Result<()> {
        let path = self.archive_batch_sentinel_path(batch.archive_hash);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let sentinel = ArchiveBatchSentinel {
            pipeline_version: APPLY_STATE_VERSION,
            archive_hash: batch.archive_hash,
            archive_size_bytes: batch.archive_size_bytes,
            directive_indices: batch.directives.iter().map(|d| d.directive_index).collect(),
        };
        tokio::fs::write(&path, serde_json::to_vec_pretty(&sentinel)?).await?;
        Ok(())
    }

    fn archive_batch_sentinel_path(&self, archive_hash: u64) -> PathBuf {
        self.staging_dir
            .join("_state/archive-batches")
            .join(format!("{archive_hash:016x}.json"))
    }

    async fn create_bsa_sentinel_valid(
        &self,
        directive_index: usize,
        temp_id: &str,
        to: &str,
    ) -> bool {
        let path = self.create_bsa_sentinel_path(directive_index);
        let Ok(data) = tokio::fs::read_to_string(&path).await else {
            return false;
        };
        let Ok(sentinel) = serde_json::from_str::<CreateBsaSentinel>(&data) else {
            return false;
        };
        sentinel.pipeline_version == APPLY_STATE_VERSION
            && sentinel.directive_index == directive_index
            && sentinel.temp_id == temp_id
            && sentinel.to == to
            && StagingStore::new(&self.staging_dir)
                .logical_exists(to)
                .await
    }

    async fn write_create_bsa_sentinel(
        &self,
        directive_index: usize,
        temp_id: &str,
        to: &str,
    ) -> Result<()> {
        let path = self.create_bsa_sentinel_path(directive_index);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let sentinel = CreateBsaSentinel {
            pipeline_version: APPLY_STATE_VERSION,
            directive_index,
            temp_id: temp_id.to_string(),
            to: to.to_string(),
        };
        tokio::fs::write(&path, serde_json::to_vec_pretty(&sentinel)?).await?;
        Ok(())
    }

    fn create_bsa_sentinel_path(&self, directive_index: usize) -> PathBuf {
        self.staging_dir
            .join("_state/create-bsa")
            .join(format!("{directive_index}.json"))
    }

    /// Extract inline file data from the `.wabbajack` zip and write to staging.
    async fn apply_inline_file(
        &self,
        inline_source: &InlineSource,
        source_data_id: &str,
        to: &str,
    ) -> Result<()> {
        let staging_store = StagingStore::new(&self.staging_dir);

        // Validate the output path against traversal attacks
        validate_archive_entry(to)?;

        let data = inline_source.read(source_data_id)?;
        let output_path = staging_store.write_path_for_logical(to, Some(data.len() as u64));

        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        write_bytes_maybe_zstd(&output_path, &data).await?;

        info!(source_data_id = %source_data_id, to = %to, "wrote inline file");
        Ok(())
    }

    /// Extract a file from archive, apply a binary delta patch, and write to staging.
    async fn apply_patched_from_archive(
        &self,
        inline_source: &InlineSource,
        archive_hash: u64,
        from: &str,
        to: &str,
        patch_id: &str,
        expected_size: u64,
    ) -> Result<()> {
        // Extract source file from a downloaded archive or local game-file source.
        let source_data = self
            .read_archive_source_cached(archive_hash, from)
            .await
            .with_context(|| {
                format!("failed to extract '{from}' from archive {archive_hash:016x} for patching")
            })?;

        self.write_patched_output(inline_source, source_data, to, patch_id, expected_size)
            .await
    }

    async fn write_patched_output(
        &self,
        inline_source: &InlineSource,
        source_data: Bytes,
        to: &str,
        patch_id: &str,
        expected_size: u64,
    ) -> Result<()> {
        // Validate the output path against traversal attacks
        validate_archive_entry(to)?;

        // Patched outputs can expand far beyond the source size. Keep the hot
        // patch path plain and let the compatibility compression sweep handle
        // eligible files later with bounded worker controls.
        let output_path = self.staging_dir.join(normalize_path(to));
        let part_path = output_path.with_extension(format!(
            "{}modde-part",
            output_path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| format!("{extension}."))
                .unwrap_or_default()
        ));
        let stale_compressed = super::staging::compressed_path(&output_path);

        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if tokio::fs::metadata(&stale_compressed).await.is_ok() {
            tokio::fs::remove_file(&stale_compressed).await?;
        }
        if tokio::fs::metadata(&part_path).await.is_ok() {
            tokio::fs::remove_file(&part_path).await?;
        }

        let patch_data = inline_source.read(patch_id)?;
        let _patch_guard = self.diagnostics.as_ref().map(|diagnostics| {
            diagnostics.start_patch(
                to.to_string(),
                patch_id.to_string(),
                source_data.len() as u64,
                expected_size,
            )
        });

        let to_owned = to.to_string();
        let output_path_for_task = output_path.clone();
        let part_path_for_task = part_path.clone();
        let patch_result = tokio::task::spawn_blocking(move || {
            let mut output = std::fs::File::create(&part_path_for_task).with_context(|| {
                format!(
                    "failed to create patched output: {}",
                    part_path_for_task.display()
                )
            })?;
            let expected = (expected_size > 0).then_some(expected_size);
            let output_bytes = patcher::apply_patch_to_writer_limited(
                &source_data,
                &patch_data,
                &mut output,
                expected,
            )
            .with_context(|| format!("failed to apply patch for: {to_owned}"))?;
            output
                .sync_all()
                .with_context(|| format!("failed to sync patched output: {to_owned}"))?;
            drop(output);
            std::fs::rename(&part_path_for_task, &output_path_for_task).with_context(|| {
                format!(
                    "failed to move patched output into place: {}",
                    output_path_for_task.display()
                )
            })?;
            Ok::<u64, anyhow::Error>(output_bytes)
        })
        .await?;
        let output_bytes = match patch_result {
            Ok(output_bytes) => output_bytes,
            Err(e) => {
                let _ = tokio::fs::remove_file(&part_path).await;
                return Err(e);
            }
        };
        trim_process_allocator();

        info!(
            to = %to,
            patch_id = %patch_id,
            expected_size,
            output_bytes,
            "applied binary patch"
        );
        Ok(())
    }

    /// Create a BSA/BA2 archive from file states.
    async fn apply_create_bsa(
        &self,
        temp_id: &str,
        to: &str,
        file_states: &[modde_core::manifest::wabbajack::BSAFileState],
    ) -> Result<()> {
        // BSA source files are expected to be in a temp subdirectory of staging
        let bsa_staging = self.staging_dir.join(format!("bsa_temp_{temp_id}"));
        let output_path = self.staging_dir.join(normalize_path(to));

        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        bsa_repack::create_bsa(file_states, &bsa_staging, &output_path)
            .await
            .with_context(|| format!("failed to create BSA: {to}"))?;

        info!(to = %to, files = file_states.len(), "created BSA archive");
        Ok(())
    }

    fn validate_game_file_sources(&self) -> Result<()> {
        if !self.manifest.archives.iter().any(is_game_file_archive) {
            return Ok(());
        }

        let Some(game_dir) = &self.game_dir else {
            bail!(
                "wabbajack modlist references local game files; pass --game-dir so modde can read the installed game files"
            );
        };

        if !game_dir.is_dir() {
            bail!(
                "game directory for Wabbajack game-file sources does not exist: {}",
                game_dir.display()
            );
        }

        Ok(())
    }

    fn validate_download_sources(&self, downloads: &[DownloadDirective]) -> Result<()> {
        let missing: Vec<String> = downloads
            .iter()
            .filter(|directive| {
                !self
                    .sources
                    .iter()
                    .any(|source| source.can_handle(directive))
            })
            .map(|directive| directive.display_name().into_owned())
            .collect();

        if !missing.is_empty() {
            let shown = missing
                .iter()
                .take(20)
                .map(|name| format!("  - {name}"))
                .collect::<Vec<_>>()
                .join("\n");
            let omitted = missing.len().saturating_sub(20);
            let suffix = if omitted == 0 {
                String::new()
            } else {
                format!("\n  ... and {omitted} more")
            };
            bail!(
                "Wabbajack manifest requires {} download(s) with no registered source:\n{}{}\nConfigure the missing source before running the install.",
                missing.len(),
                shown,
                suffix
            );
        }

        Ok(())
    }

    async fn preflight_authored_files(&self) -> Result<()> {
        for source in self.sources.iter() {
            if let AnySource::WabbajackCdn(source) = source {
                return source.preflight_archives(&self.manifest.archives).await;
            }
        }
        Ok(())
    }

    async fn verify_game_file_sources(&self) -> Result<()> {
        let mut failures = Vec::new();

        for archive in self
            .manifest
            .archives
            .iter()
            .filter(|a| is_game_file_archive(a))
        {
            let Some(source) = self.game_file_source_path(archive.hash)? else {
                continue;
            };

            if !source.path.exists() {
                failures.push(format!(
                    "game-file source '{}' is missing at {}",
                    source.rel_path,
                    source.path.display()
                ));
                continue;
            }

            let actual = modde_core::hash::hash_file_xxh64(&source.path)
                .await
                .with_context(|| {
                    format!("failed to hash game-file source '{}'", source.rel_path)
                })?;
            if actual != archive.hash {
                failures.push(format!(
                    "game-file source '{}' failed hash verification (expected xxh64 {:016x}, got {:016x})",
                    source.rel_path, archive.hash, actual
                ));
            }
        }

        if !failures.is_empty() {
            bail!(
                "Wabbajack game-file source validation failed for {} file(s):\n{}",
                failures.len(),
                failures
                    .iter()
                    .map(|failure| format!("  - {failure}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }

        Ok(())
    }

    async fn read_archive_source(&self, archive_hash: u64, from: &str) -> Result<Vec<u8>> {
        if let Some(source) = self.game_file_source_path(archive_hash)? {
            return read_game_file_source(&source.path, from).await;
        }

        let archive_path = archive_path(&self.store_dir, &archive_hash);
        let from = from.to_string();
        tokio::task::spawn_blocking(move || {
            let output = ArchiveBatchExtractor::extract_selected(
                &archive_path,
                &[ArchiveRequest {
                    directive_index: 0,
                    from,
                    inner_path: None,
                    kind: ArchiveRequestKind::Bytes,
                }],
            )?;
            output
                .bytes
                .get(&0)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("archive extractor returned no bytes"))
        })
        .await?
    }

    async fn read_archive_source_cached(&self, archive_hash: u64, from: &str) -> Result<Bytes> {
        if let Some(bytes) = self.patch_source_from_cache(archive_hash, from) {
            return Ok(bytes);
        }
        let data = self.read_archive_source(archive_hash, from).await?;
        Ok(self.cache_patch_source(archive_hash, from, Bytes::from(data)))
    }

    fn patch_source_from_cache(&self, archive_hash: u64, from: &str) -> Option<Bytes> {
        self.byte_cache.get(&ByteCacheKey {
            archive_hash,
            inner_path: normalize_path(from),
        })
    }

    fn cache_patch_source(&self, archive_hash: u64, from: &str, bytes: Bytes) -> Bytes {
        let disable_under_pressure = std::env::var("MODDE_BYTE_CACHE_DISABLE_UNDER_PRESSURE")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(true);
        let pressure_threshold = std::env::var("MODDE_BYTE_CACHE_PRESSURE_FRACTION")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|&v| v > 0.0 && v <= 1.0)
            .unwrap_or(0.80);
        if disable_under_pressure && cgroup_memory_pressure_high(pressure_threshold) {
            warn!(
                archive_hash = %format!("{archive_hash:016x}"),
                from = %from,
                bytes = bytes.len(),
                pressure_threshold,
                "skipping patch source byte cache under cgroup memory pressure"
            );
            return bytes;
        }
        self.byte_cache.insert(
            ByteCacheKey {
                archive_hash,
                inner_path: normalize_path(from),
            },
            bytes,
        )
    }

    fn check_diagnostics_abort(&self) -> Result<()> {
        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.check_abort()?;
        }
        Ok(())
    }

    fn game_file_source_path(&self, archive_hash: u64) -> Result<Option<GameFileSourcePath>> {
        let Some(archive) = self
            .manifest
            .archives
            .iter()
            .find(|a| a.hash == archive_hash)
        else {
            return Ok(None);
        };

        let Some(state) = archive.state.as_ref() else {
            return Ok(None);
        };

        let rel_path = match state {
            ArchiveState::GameFileSourceDownloader { metadata } => state.game_file_path().ok_or_else(|| {
                anyhow::anyhow!(
                    "game-file source archive '{}' does not contain a recognized file path field; metadata keys: {}",
                    archive.name,
                    metadata.keys().cloned().collect::<Vec<_>>().join(", ")
                )
            })?,
            _ => return Ok(None),
        };

        validate_archive_entry(rel_path)?;

        let Some(game_dir) = &self.game_dir else {
            bail!(
                "archive {} is a game-file source, but no game directory was provided",
                archive.name
            );
        };

        let path = game_dir.join(normalize_path(rel_path));
        if path.exists() {
            return Ok(Some(GameFileSourcePath {
                rel_path: rel_path.to_string(),
                path,
            }));
        }

        Ok(Some(GameFileSourcePath {
            rel_path: rel_path.to_string(),
            path: find_path_case_insensitive(game_dir, &normalize_path(rel_path)).unwrap_or(path),
        }))
    }
}

#[derive(Debug)]
struct GameFileSourcePath {
    rel_path: String,
    path: PathBuf,
}

fn is_game_file_archive(archive: &modde_core::manifest::wabbajack::ArchiveEntry) -> bool {
    matches!(
        archive.state.as_ref(),
        Some(ArchiveState::GameFileSourceDownloader { .. })
    )
}

/// Validate that an archive entry name does not contain path traversal components
/// or represent a symlink entry, preventing zip-slip and symlink attacks.
fn validate_archive_entry(name: &str) -> Result<()> {
    // Normalize separators for consistent checking
    let normalized = name.replace('\\', "/");

    // Reject entries with ".." path components (path traversal / zip-slip)
    for component in normalized.split('/') {
        if component == ".." {
            bail!("archive entry contains path traversal: {name}");
        }
    }

    // Reject absolute paths
    if normalized.starts_with('/') {
        bail!("archive entry contains absolute path: {name}");
    }

    Ok(())
}

/// Validate a zip entry, checking for path traversal and symlinks.
#[cfg(test)]
fn validate_zip_entry<R: std::io::Read + ?Sized>(entry: &zip::read::ZipFile<'_, R>) -> Result<()> {
    let name = entry.name();
    validate_archive_entry(name)?;

    // Reject symlink entries from zip archives
    if entry.is_symlink() {
        bail!("archive entry is a symlink (rejected for security): {name}");
    }

    Ok(())
}

/// Normalize Windows-style backslash paths to forward slashes for Linux.
fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn game_file_source_is_whole_file(path: &Path, rel_path: &str, from: &str) -> bool {
    if from.trim().is_empty() || from == "." {
        return true;
    }

    let normalized_from = normalize_path(from);
    let normalized_rel_path = normalize_path(rel_path);
    let normalized_path = normalize_path(&path.to_string_lossy());
    let path_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(normalize_path);

    normalized_rel_path.eq_ignore_ascii_case(&normalized_from)
        || normalized_path.ends_with(&normalized_from)
        || path_name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(&normalized_from))
}

async fn read_game_file_source(path: &Path, from: &str) -> Result<Vec<u8>> {
    if !from.trim().is_empty() {
        validate_archive_entry(from)?;
    }

    if path.symlink_metadata()?.file_type().is_symlink() {
        bail!(
            "game-file source is a symlink (rejected for security): {}",
            path.display()
        );
    }

    if from.trim().is_empty() || from == "." {
        return tokio::fs::read(path)
            .await
            .with_context(|| format!("failed to read game-file source: {}", path.display()));
    }

    if game_file_source_is_whole_file(path, &path.to_string_lossy(), from) {
        return tokio::fs::read(path)
            .await
            .with_context(|| format!("failed to read game-file source: {}", path.display()));
    }

    let archive_path = path.to_path_buf();
    let from = from.to_string();
    tokio::task::spawn_blocking(move || {
        let output = ArchiveBatchExtractor::extract_selected(
            &archive_path,
            &[ArchiveRequest {
                directive_index: 0,
                from,
                inner_path: None,
                kind: ArchiveRequestKind::Bytes,
            }],
        )?;
        output
            .bytes
            .get(&0)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("archive extractor returned no bytes"))
    })
    .await?
}

/// Compute the storage path for an archive by its hash.
pub(crate) fn archive_path(store_dir: &Path, hash: &u64) -> PathBuf {
    store_dir.join(format!("{hash:016x}.archive"))
}

fn verified_sidecar_path(path: &Path) -> PathBuf {
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(".verified.json");
    PathBuf::from(sidecar)
}

fn metadata_modified_unix_ms(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

async fn archive_path_is_memory_supported(path: &Path) -> bool {
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return false;
    };
    let mut magic = [0_u8; 8];
    let Ok(read) = tokio::io::AsyncReadExt::read(&mut file, &mut magic).await else {
        return false;
    };
    let prefix = &magic[..read];
    prefix.starts_with(b"PK") || prefix.starts_with(&[0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c])
}

async fn extract_trusted_archive_requests(
    trusted_archive: Option<TrustedArchive>,
    requests: Vec<ArchiveRequest>,
) -> Result<crate::decompress::ArchiveBatchOutput> {
    let native_result = tokio::task::spawn_blocking(move || match trusted_archive {
        Some(TrustedArchive::Path(path)) => ArchiveBatchExtractor::extract_selected(&path, &requests),
        Some(TrustedArchive::Bytes {
            label,
            bytes,
            fallback_path,
        }) => {
            let memory_result = ArchiveBatchExtractor::extract_selected_from(
                ArchiveInput::Bytes {
                    name: &label,
                    bytes: &bytes,
                },
                &requests,
            );
            match memory_result {
                Ok(output) => Ok(output),
                Err(memory_error) if is_unsupported_in_memory_archive(&memory_error) => {
                    warn!(
                        archive = %label,
                        "in-memory archive extraction failed, falling back to disk: {memory_error:#}"
                    );
                    ArchiveBatchExtractor::extract_selected(&fallback_path, &requests)
                }
                Err(memory_error) => Err(memory_error),
            }
        }
        None => unreachable!("native requests require a trusted archive"),
    })
    .await;
    trim_process_allocator();

    match native_result {
        Ok(result) => result,
        Err(e) => Err(anyhow::anyhow!("archive extraction task failed: {e:#}")),
    }
}

async fn extract_archive_path_requests(
    path: PathBuf,
    requests: Vec<ArchiveRequest>,
) -> Result<crate::decompress::ArchiveBatchOutput> {
    let native_result = tokio::task::spawn_blocking(move || {
        ArchiveBatchExtractor::extract_selected(&path, &requests)
    })
    .await;
    trim_process_allocator();

    match native_result {
        Ok(result) => result,
        Err(e) => Err(anyhow::anyhow!("archive extraction task failed: {e:#}")),
    }
}

async fn extract_nested_archive_requests(
    label: String,
    bytes: Bytes,
    requests: Vec<ArchiveRequest>,
) -> Result<crate::decompress::ArchiveBatchOutput> {
    let native_result = tokio::task::spawn_blocking(move || {
        if bytes.starts_with(b"BSA\0") || bytes.starts_with(b"BTDX") {
            let mut temp = tempfile::NamedTempFile::new()?;
            std::io::Write::write_all(&mut temp, &bytes)?;
            std::io::Write::flush(&mut temp)?;
            ArchiveBatchExtractor::extract_selected(temp.path(), &requests)
        } else {
            ArchiveBatchExtractor::extract_selected_from(
                ArchiveInput::Bytes {
                    name: &label,
                    bytes: &bytes,
                },
                &requests,
            )
        }
    })
    .await;
    trim_process_allocator();

    match native_result {
        Ok(result) => result,
        Err(e) => Err(anyhow::anyhow!(
            "nested archive extraction task failed: {e:#}"
        )),
    }
}

fn is_unsupported_in_memory_archive(error: &anyhow::Error) -> bool {
    format!("{error:#}").contains("unsupported in-memory archive format")
}

fn archive_patch_chunk_size() -> usize {
    std::env::var("MODDE_ARCHIVE_PATCH_CHUNK_SIZE")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|&value| value > 0)
        .unwrap_or(8)
}

async fn write_bytes_maybe_zstd(path: &Path, data: &[u8]) -> Result<()> {
    if super::staging::is_compressed_path(path) {
        let path = path.to_path_buf();
        let data = Bytes::copy_from_slice(data);
        tokio::task::spawn_blocking(move || -> Result<()> {
            let file = std::fs::File::create(&path)
                .with_context(|| format!("failed to create {}", path.display()))?;
            let mut encoder = zstd::stream::write::Encoder::new(file, 9)?;
            std::io::Write::write_all(&mut encoder, &data)?;
            encoder.finish()?;
            Ok(())
        })
        .await??;
        return Ok(());
    }
    tokio::fs::write(path, data).await?;
    Ok(())
}

/// Best-effort estimate of the peak resident bytes a directive will need
/// during its apply step.
///
/// Most apply work is dominated by archive operations — either streaming a
/// single file out of a native decoder, or feeding a source file plus its
/// patch into bsdiff. We intentionally over-count rather than under-count:
/// being throttled too aggressively just slows the install, while admitting
/// a worker that the host can't satisfy crashes it.
fn estimate_directive_weight(
    directive: &InstallDirective,
    archive_size_by_hash: &HashMap<u64, u64>,
) -> u64 {
    // Tunable through env vars so site operators can dial without rebuilding.
    let from_archive_factor: f64 = std::env::var("MODDE_APPLY_WEIGHT_FROM_ARCHIVE_FACTOR")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&v: &f64| v > 0.0)
        .unwrap_or(0.5);
    let patched_factor: f64 = std::env::var("MODDE_APPLY_WEIGHT_PATCHED_FACTOR")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&v: &f64| v > 0.0)
        .unwrap_or(3.0);
    let create_bsa_floor_mb: u64 = std::env::var("MODDE_APPLY_WEIGHT_BSA_FLOOR_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64);

    const FLOOR_BYTES: u64 = 8 * 1024 * 1024; // 8 MiB

    let raw = match directive {
        InstallDirective::InlineFile { .. } => FLOOR_BYTES,
        InstallDirective::FromArchive { archive_hash, .. } => {
            let size = archive_size_by_hash.get(archive_hash).copied().unwrap_or(0);
            ((size as f64) * from_archive_factor) as u64
        }
        InstallDirective::PatchedFromArchive { archive_hash, .. } => {
            let size = archive_size_by_hash.get(archive_hash).copied().unwrap_or(0);
            ((size as f64) * patched_factor) as u64
        }
        InstallDirective::CreateBSA { file_states, .. } => file_states
            .iter()
            .map(|fs| fs.size)
            .sum::<u64>()
            .max(create_bsa_floor_mb * 1024 * 1024),
    };

    raw.max(FLOOR_BYTES)
}

fn estimate_archive_batch_weight(batch: &ArchiveInstallBatch) -> u64 {
    const FLOOR_BYTES: u64 = 8 * 1024 * 1024;
    let has_patch = archive_batch_has_patch(batch);
    let factor = if has_patch {
        std::env::var("MODDE_APPLY_WEIGHT_PATCHED_BATCH_FACTOR")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|&v| v > 0.0)
            .unwrap_or(1.0)
    } else {
        std::env::var("MODDE_APPLY_WEIGHT_ARCHIVE_BATCH_FACTOR")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|&v| v > 0.0)
            .unwrap_or(0.5)
    };
    (((batch.archive_size_bytes as f64) * factor) as u64).max(FLOOR_BYTES)
}

fn archive_batch_has_patch(batch: &ArchiveInstallBatch) -> bool {
    batch.directives.iter().any(|directive| {
        matches!(
            &directive.directive,
            InstallDirective::PatchedFromArchive { .. }
        )
    })
}

#[cfg(all(unix, target_env = "gnu"))]
fn trim_process_allocator() {
    // Native decoders can transiently allocate very large buffers for solid
    // archives. glibc often keeps those arenas mapped, which makes the next
    // batch look resident even after Rust values were dropped.
    unsafe {
        libc::malloc_trim(0);
    }
}

#[cfg(not(all(unix, target_env = "gnu")))]
fn trim_process_allocator() {}

/// Find a path by case-insensitive path matching in a directory tree.
fn find_path_case_insensitive(base: &Path, relative_path: &str) -> Result<PathBuf> {
    validate_archive_entry(relative_path)?;

    let parts: Vec<&str> = relative_path.split('/').collect();
    let mut current = base.to_path_buf();

    for part in &parts {
        let target_lower = part.to_lowercase();
        let mut found = false;

        for entry in std::fs::read_dir(&current)
            .with_context(|| format!("failed to read dir: {}", current.display()))?
        {
            let entry = entry?;
            if entry.file_name().to_string_lossy().to_lowercase() == target_lower {
                current = entry.path();
                // Reject symlinks in intermediate path components
                if current.symlink_metadata()?.file_type().is_symlink() {
                    anyhow::bail!("path component is a symlink (rejected for security): {part}");
                }
                found = true;
                break;
            }
        }

        if !found {
            anyhow::bail!(
                "path component '{}' not found in {}",
                part,
                current.display()
            );
        }
    }

    Ok(current)
}

/// Extract a file from a zip archive.
#[cfg(test)]
fn extract_from_zip(archive_path: &Path, inner_path: &str) -> Result<Vec<u8>> {
    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("failed to open archive: {}", archive_path.display()))?;

    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read zip archive: {}", archive_path.display()))?;

    let entry_name = find_entry_in_archive(&archive, inner_path).with_context(|| {
        format!(
            "file '{}' not found in archive {}",
            inner_path,
            archive_path.display()
        )
    })?;

    let mut entry = archive.by_name(&entry_name)?;
    validate_zip_entry(&entry)?;
    let mut data = Vec::with_capacity(entry.size() as usize);
    std::io::Read::read_to_end(&mut entry, &mut data)?;

    Ok(data)
}

/// Find a file entry in a zip archive, trying multiple path formats.
#[cfg(test)]
fn find_entry_in_archive(archive: &zip::ZipArchive<std::fs::File>, path: &str) -> Result<String> {
    // Normalize separators for comparison
    let normalized = path.replace('\\', "/");
    let backslash = path.replace('/', "\\");

    for i in 0..archive.len() {
        let name = archive.name_for_index(i).unwrap_or_default().to_string();
        if name == *path || name == normalized || name == backslash {
            return Ok(name);
        }
        // Case-insensitive fallback
        let name_lower = name.to_lowercase();
        if name_lower == path.to_lowercase()
            || name_lower == normalized.to_lowercase()
            || name_lower == backslash.to_lowercase()
        {
            return Ok(name);
        }
    }

    anyhow::bail!("entry not found: {path}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wabbajack::staging::{StagingPrepareStatus, StagingStore, compressed_path};

    #[test]
    fn validate_archive_entry_rejects_traversal_and_absolute() {
        // The batch FromArchive path (install_directives) relies on this to block
        // zip-slip via a malicious manifest `to` field.
        assert!(validate_archive_entry("mods/Foo/textures/x.dds").is_ok());
        assert!(validate_archive_entry("../escape.txt").is_err());
        assert!(validate_archive_entry("a/b/../../../escape").is_err());
        assert!(validate_archive_entry("..\\escape.txt").is_err());
        assert!(validate_archive_entry("/etc/passwd").is_err());
    }
    use std::collections::HashMap;
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use xxhash_rust::xxh64::xxh64;

    /// Helper: create a zip file on disk with the given entries.
    fn create_zip_file(path: &std::path::Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
    }

    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, data) in entries {
                writer.start_file(*name, options).unwrap();
                writer.write_all(data).unwrap();
            }
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    fn data_patch(data: &[u8]) -> Vec<u8> {
        let mut patch = Vec::new();
        patch.extend_from_slice(b"OCTODELTA");
        patch.push(1);
        patch.push(4);
        patch.extend_from_slice(b"SHA1");
        patch.extend_from_slice(&20_u32.to_le_bytes());
        patch.extend_from_slice(&[0_u8; 20]);
        patch.extend_from_slice(b">>>");
        patch.push(0x80);
        patch.extend_from_slice(&(data.len() as u64).to_le_bytes());
        patch.extend_from_slice(data);
        patch
    }

    /// Helper: open a zip for `find_entry_in_archive` tests.
    fn open_zip(path: &std::path::Path) -> zip::ZipArchive<std::fs::File> {
        let file = std::fs::File::open(path).unwrap();
        zip::ZipArchive::new(file).unwrap()
    }

    fn minimal_manifest() -> WabbajackManifest {
        WabbajackManifest {
            name: "test".into(),
            author: "a".into(),
            description: "d".into(),
            game: "SkyrimSE".into(),
            version: "1.0".into(),
            archives: vec![],
            directives: vec![],
        }
    }

    fn game_file_archive(
        hash: u64,
        rel_path: &str,
    ) -> modde_core::manifest::wabbajack::ArchiveEntry {
        modde_core::manifest::wabbajack::ArchiveEntry {
            hash,
            name: rel_path.replace(['\\', '/'], "_"),
            size: 0,
            state: Some(ArchiveState::GameFileSourceDownloader {
                metadata: HashMap::from([(
                    "File".to_string(),
                    serde_json::Value::String(rel_path.to_string()),
                )]),
            }),
        }
    }

    fn manifest_with_game_file(hash: u64, rel_path: &str, to: &str) -> WabbajackManifest {
        WabbajackManifest {
            archives: vec![game_file_archive(hash, rel_path)],
            directives: vec![modde_core::manifest::wabbajack::RawDirective::FromArchive {
                archive_hash_path: vec![serde_json::Value::Number(hash.into())],
                to: to.into(),
                size: 0,
            }],
            ..minimal_manifest()
        }
    }

    fn manifest_with_nexus_download(hash: u64) -> WabbajackManifest {
        WabbajackManifest {
            archives: vec![modde_core::manifest::wabbajack::ArchiveEntry {
                hash,
                name: "nexus archive".into(),
                size: 1,
                state: Some(ArchiveState::NexusDownloader {
                    game_name: "skyrimspecialedition".into(),
                    mod_id: 123,
                    file_id: 456,
                }),
            }],
            ..minimal_manifest()
        }
    }

    #[tokio::test]
    async fn archive_trust_sidecar_skips_second_hash_pass() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        let staging = dir.path().join("staging");
        tokio::fs::create_dir_all(&store).await.unwrap();
        let bytes = b"trusted archive bytes";
        let hash = xxh64(bytes, 0);
        let path = archive_path(&store, &hash);
        tokio::fs::write(&path, bytes).await.unwrap();

        let inst = WabbajackInstaller::new(
            minimal_manifest(),
            dir.path().join("list.wabbajack"),
            store,
            staging,
        );
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        let (_archive, first) = inst.ensure_archive_trusted(hash, &tx).await.unwrap();
        assert_eq!(first.streamed_hash_bytes, bytes.len() as u64);
        assert!(!first.sidecar_hit);
        assert!(verified_sidecar_path(&path).exists());

        let (_archive, second) = inst.ensure_archive_trusted(hash, &tx).await.unwrap();
        assert!(second.sidecar_hit);
        assert_eq!(second.streamed_hash_bytes, 0);
    }

    #[tokio::test]
    async fn stale_archive_trust_sidecar_forces_rehash() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        let staging = dir.path().join("staging");
        tokio::fs::create_dir_all(&store).await.unwrap();
        let bytes = b"trusted archive bytes";
        let hash = xxh64(bytes, 0);
        let path = archive_path(&store, &hash);
        tokio::fs::write(&path, bytes).await.unwrap();

        let inst = WabbajackInstaller::new(
            minimal_manifest(),
            dir.path().join("list.wabbajack"),
            store,
            staging,
        );
        inst.write_verified_sidecar(&path, hash).await.unwrap();
        let mut sidecar: serde_json::Value =
            serde_json::from_slice(&tokio::fs::read(verified_sidecar_path(&path)).await.unwrap())
                .unwrap();
        sidecar["size_bytes"] = serde_json::json!(1);
        tokio::fs::write(
            verified_sidecar_path(&path),
            serde_json::to_vec_pretty(&sidecar).unwrap(),
        )
        .await
        .unwrap();

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let (_archive, stats) = inst.ensure_archive_trusted(hash, &tx).await.unwrap();
        assert!(!stats.sidecar_hit);
        assert_eq!(stats.streamed_hash_bytes, bytes.len() as u64);
    }

    fn synthetic_server(
        cdn_archive: Vec<u8>,
        direct_archive: Vec<u8>,
    ) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let thread_count = Arc::clone(&count);
        thread::spawn(move || {
            for stream in listener.incoming().take(8) {
                let mut stream = stream.unwrap();
                let mut buf = [0_u8; 2048];
                let n = stream.read(&mut buf).unwrap();
                let request = String::from_utf8_lossy(&buf[..n]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");
                thread_count.fetch_add(1, Ordering::SeqCst);
                match path {
                    "/authored_files/download/cdn.zip_abc" => {
                        let body = format!(
                            r#"<script>
                              const MUNGED_NAME = "cdn.zip_abc";
                              const FILE_NAME = "cdn.zip";
                              const FILE_SIZE_BYTES = {};
                              const PARTS = [{{"Size":{},"Offset":0,"Index":0}}];
                            </script>"#,
                            cdn_archive.len(),
                            cdn_archive.len()
                        );
                        write_response(&mut stream, "200 OK", body.as_bytes());
                    }
                    "/authored_files/cdn.zip_abc/parts/0" => {
                        write_response(&mut stream, "200 OK", &cdn_archive);
                    }
                    "/direct.zip" => {
                        write_response(&mut stream, "200 OK", &direct_archive);
                    }
                    _ => write_response(&mut stream, "404 Not Found", b"not found"),
                }
            }
        });
        (format!("http://{addr}"), count)
    }

    fn write_response(stream: &mut std::net::TcpStream, status: &str, body: &[u8]) {
        let headers = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(headers.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    }

    // -----------------------------------------------------------------------
    // 1. WabbajackInstaller::new() creates correct directory structure
    // -----------------------------------------------------------------------
    #[test]
    fn new_stores_fields() {
        let store = PathBuf::from("/tmp/store");
        let staging = PathBuf::from("/tmp/staging");
        let inst = WabbajackInstaller::new(
            minimal_manifest(),
            PathBuf::from("/tmp/test.wabbajack"),
            store,
            staging,
        );
        assert_eq!(inst.concurrency, DEFAULT_CONCURRENCY);
        assert!(inst.sources.is_empty());
        assert_eq!(inst.store_dir, PathBuf::from("/tmp/store"));
        assert_eq!(inst.staging_dir, PathBuf::from("/tmp/staging"));
        assert_eq!(inst.manifest.name, "test");
    }

    #[test]
    fn validate_download_sources_rejects_required_source_before_network() {
        let dir = tempfile::tempdir().unwrap();
        let inst = WabbajackInstaller::new(
            manifest_with_nexus_download(1),
            dir.path().join("test.wabbajack"),
            dir.path().join("store"),
            dir.path().join("staging"),
        );

        let downloads = inst.manifest.download_directives();
        let err = inst.validate_download_sources(&downloads).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("no registered source"),
            "unexpected error: {msg}"
        );
        assert!(msg.contains("nexus:123"), "unexpected error: {msg}");
    }

    #[tokio::test]
    async fn adoption_preserves_existing_outputs_and_writes_sentinels() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        let output = staging.join("mods/adopted/file.txt");
        tokio::fs::create_dir_all(output.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&output, b"already applied").await.unwrap();
        tokio::fs::write(staging.join("mods/adopted/output.bsa"), b"bsa")
            .await
            .unwrap();

        let hash = 42_u64;
        let manifest = WabbajackManifest {
            archives: vec![modde_core::manifest::wabbajack::ArchiveEntry {
                hash,
                name: "source.zip".into(),
                size: 123,
                state: None,
            }],
            directives: vec![
                modde_core::manifest::wabbajack::RawDirective::FromArchive {
                    archive_hash_path: vec![
                        serde_json::Value::Number(hash.into()),
                        serde_json::Value::String("file.txt".into()),
                    ],
                    to: "mods/adopted/file.txt".into(),
                    size: 0,
                },
                modde_core::manifest::wabbajack::RawDirective::CreateBSA {
                    temp_id: "bsa-temp".into(),
                    to: "mods/adopted/output.bsa".into(),
                    file_states: vec![],
                },
            ],
            ..minimal_manifest()
        };
        let inst = WabbajackInstaller::new(
            manifest,
            dir.path().join("test.wabbajack"),
            dir.path().join("store"),
            staging.clone(),
        );

        let prepare_status = StagingStore::new(&staging)
            .prepare_resumable()
            .await
            .unwrap();
        assert_eq!(prepare_status, StagingPrepareStatus::Adopted);

        let batches = inst.manifest.install_directives_grouped_by_archive();
        let installs = inst.manifest.install_directives();
        let adoption = inst
            .adopt_existing_staging(&batches, &installs)
            .await
            .unwrap();

        assert_eq!(adoption.archive_batches, 1);
        assert_eq!(adoption.create_bsa, 1);
        assert!(output.exists());
        assert!(inst.archive_batch_sentinel_path(hash).exists());
        assert!(inst.create_bsa_sentinel_path(1).exists());
        assert!(inst.archive_batch_sentinel_valid(&batches[0]).await);
        assert!(
            inst.create_bsa_sentinel_valid(1, "bsa-temp", "mods/adopted/output.bsa")
                .await
        );
    }

    #[tokio::test]
    async fn synthetic_wabbajack_pipeline_reaches_late_directives() {
        let dir = tempfile::tempdir().unwrap();
        let cdn_archive = zip_bytes(&[("source.txt", b"basis")]);
        let large_direct = vec![7_u8; 1024 * 1024 + 17];
        let direct_archive = zip_bytes(&[
            ("direct.txt", b"direct"),
            ("large.dds", large_direct.as_slice()),
        ]);
        let cdn_hash = xxh64(&cdn_archive, 0);
        let direct_hash = xxh64(&direct_archive, 0);
        let (base_url, _requests) = synthetic_server(cdn_archive, direct_archive);
        let wabbajack_path = dir.path().join("synthetic.wabbajack");
        create_zip_file(
            &wabbajack_path,
            &[
                ("inline-data", b"inline"),
                ("patch-data", &data_patch(b"patched")),
            ],
        );

        let manifest = WabbajackManifest {
            archives: vec![
                modde_core::manifest::wabbajack::ArchiveEntry {
                    hash: cdn_hash,
                    name: "cdn.zip".into(),
                    size: 0,
                    state: Some(ArchiveState::WabbajackCDNDownloader {
                        metadata: HashMap::from([(
                            "Url".into(),
                            serde_json::Value::String(format!(
                                "{base_url}/authored_files/download/cdn.zip_abc"
                            )),
                        )]),
                    }),
                },
                modde_core::manifest::wabbajack::ArchiveEntry {
                    hash: direct_hash,
                    name: "direct.zip".into(),
                    size: 0,
                    state: Some(ArchiveState::HttpDownloader {
                        url: format!("{base_url}/direct.zip"),
                        headers: HashMap::new(),
                    }),
                },
            ],
            directives: vec![
                modde_core::manifest::wabbajack::RawDirective::FromArchive {
                    archive_hash_path: vec![
                        serde_json::Value::Number(cdn_hash.into()),
                        serde_json::Value::String("source.txt".into()),
                    ],
                    to: "mods/cdn/source.txt".into(),
                    size: 0,
                },
                modde_core::manifest::wabbajack::RawDirective::FromArchive {
                    archive_hash_path: vec![
                        serde_json::Value::Number(direct_hash.into()),
                        serde_json::Value::String("direct.txt".into()),
                    ],
                    to: "mods/direct/direct.txt".into(),
                    size: 0,
                },
                modde_core::manifest::wabbajack::RawDirective::FromArchive {
                    archive_hash_path: vec![
                        serde_json::Value::Number(direct_hash.into()),
                        serde_json::Value::String("large.dds".into()),
                    ],
                    to: "mods/direct/large.dds".into(),
                    size: 0,
                },
                modde_core::manifest::wabbajack::RawDirective::InlineFile {
                    hash: 0,
                    size: 6,
                    source_data_id: "inline-data".into(),
                    to: "mods/inline/inline.txt".into(),
                },
                modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                    archive_hash_path: vec![
                        serde_json::Value::Number(cdn_hash.into()),
                        serde_json::Value::String("source.txt".into()),
                    ],
                    to: "mods/patched/patched.txt".into(),
                    hash: 0,
                    patch_id: "patch-data".into(),
                    size: 0,
                },
            ],
            ..minimal_manifest()
        };
        let mut inst = WabbajackInstaller::new(
            manifest,
            wabbajack_path,
            dir.path().join("store"),
            dir.path().join("staging"),
        );
        let client = reqwest::Client::new();
        inst.add_source(crate::AnySource::WabbajackCdn(
            crate::wabbajack::cdn::WabbajackCdnSource::new(client.clone()),
        ));
        inst.add_source(crate::AnySource::Direct(crate::direct::DirectSource::new(
            client,
        )));

        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        inst.install(progress_tx).await.unwrap();

        assert_eq!(
            tokio::fs::read(dir.path().join("staging/mods/cdn/source.txt"))
                .await
                .unwrap(),
            b"basis"
        );
        assert_eq!(
            tokio::fs::read(dir.path().join("staging/mods/direct/direct.txt"))
                .await
                .unwrap(),
            b"direct"
        );
        assert_eq!(
            tokio::fs::read(dir.path().join("staging/mods/inline/inline.txt"))
                .await
                .unwrap(),
            b"inline"
        );
        assert_eq!(
            tokio::fs::read(dir.path().join("staging/mods/patched/patched.txt"))
                .await
                .unwrap(),
            b"patched"
        );

        let large_path = dir.path().join("staging/mods/direct/large.dds");
        assert!(compressed_path(&large_path).exists());
        assert!(!large_path.exists());
        let staging_store = StagingStore::new(dir.path().join("staging"));
        let mut reader = staging_store
            .open_logical_reader("mods/direct/large.dds")
            .unwrap();
        let mut decoded = Vec::new();
        reader.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, large_direct);

        assert!(
            dir.path()
                .join(format!(
                    "staging/_state/archive-batches/{direct_hash:016x}.json"
                ))
                .exists()
        );
        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        inst.install(progress_tx).await.unwrap();
        assert!(compressed_path(&large_path).exists());
    }

    // -----------------------------------------------------------------------
    // 2. set_concurrency changes and clamps the value
    // -----------------------------------------------------------------------
    #[test]
    fn set_concurrency_changes_value() {
        let mut inst = WabbajackInstaller::new(
            minimal_manifest(),
            PathBuf::new(),
            PathBuf::new(),
            PathBuf::new(),
        );
        inst.set_concurrency(16);
        assert_eq!(inst.concurrency, 16);
    }

    #[test]
    fn set_concurrency_clamps_zero_to_one() {
        let mut inst = WabbajackInstaller::new(
            minimal_manifest(),
            PathBuf::new(),
            PathBuf::new(),
            PathBuf::new(),
        );
        inst.set_concurrency(0);
        assert_eq!(inst.concurrency, 1);
    }

    #[tokio::test]
    async fn install_reads_game_file_source_from_game_dir() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("game");
        let store_dir = dir.path().join("store");
        let staging_dir = dir.path().join("staging");
        std::fs::create_dir_all(game_dir.join("Data")).unwrap();
        let source_bytes = b"game file bytes";
        std::fs::write(game_dir.join("Data/Update.esm"), source_bytes).unwrap();

        let manifest = manifest_with_game_file(
            xxh64(source_bytes, 0),
            "Data\\Update.esm",
            "mods/Skyrim Base/Update.esm",
        );

        let mut inst = WabbajackInstaller::new(
            manifest,
            dir.path().join("test.wabbajack"),
            store_dir,
            staging_dir.clone(),
        );
        inst.set_game_dir(game_dir);

        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        inst.install(progress_tx).await.unwrap();

        assert_eq!(
            std::fs::read(staging_dir.join("mods/Skyrim Base/Update.esm")).unwrap(),
            b"game file bytes"
        );
    }

    #[tokio::test]
    async fn install_requires_game_dir_for_game_file_sources() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = WabbajackManifest {
            archives: vec![game_file_archive(0x1234, "Data\\Update.esm")],
            directives: vec![],
            ..minimal_manifest()
        };

        let inst = WabbajackInstaller::new(
            manifest,
            dir.path().join("test.wabbajack"),
            dir.path().join("store"),
            dir.path().join("staging"),
        );

        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        let err = inst.install(progress_tx).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("pass --game-dir"),
            "unexpected error message: {msg}"
        );
    }

    #[tokio::test]
    async fn install_rejects_mismatched_game_file_hash() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("game");
        std::fs::create_dir_all(game_dir.join("Data")).unwrap();
        std::fs::write(game_dir.join("Data/Update.esm"), b"actual bytes").unwrap();

        let manifest = manifest_with_game_file(
            xxh64(b"expected bytes", 0),
            "Data\\Update.esm",
            "mods/Update.esm",
        );
        let mut inst = WabbajackInstaller::new(
            manifest,
            dir.path().join("test.wabbajack"),
            dir.path().join("store"),
            dir.path().join("staging"),
        );
        inst.set_game_dir(game_dir);

        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        let err = inst.install(progress_tx).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("Data\\Update.esm"), "unexpected error: {msg}");
        assert!(
            msg.contains("expected xxh64"),
            "unexpected error message: {msg}"
        );
    }

    #[tokio::test]
    async fn install_rejects_missing_game_file() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("game");
        std::fs::create_dir_all(&game_dir).unwrap();

        let manifest = manifest_with_game_file(
            xxh64(b"missing bytes", 0),
            "Data\\Update.esm",
            "mods/Update.esm",
        );
        let mut inst = WabbajackInstaller::new(
            manifest,
            dir.path().join("test.wabbajack"),
            dir.path().join("store"),
            dir.path().join("staging"),
        );
        inst.set_game_dir(game_dir);

        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        let err = inst.install(progress_tx).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("Data\\Update.esm"), "unexpected error: {msg}");
        assert!(msg.contains("missing"), "unexpected error: {msg}");
    }

    #[tokio::test]
    async fn install_reports_all_invalid_game_files() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("game");
        std::fs::create_dir_all(game_dir.join("Data")).unwrap();
        std::fs::write(game_dir.join("Data/Update.esm"), b"actual bytes").unwrap();

        let mismatch_hash = xxh64(b"expected bytes", 0);
        let missing_hash = xxh64(b"missing bytes", 0);
        let manifest = WabbajackManifest {
            archives: vec![
                game_file_archive(mismatch_hash, "Data\\Update.esm"),
                game_file_archive(missing_hash, "SkyrimSE.exe"),
            ],
            directives: vec![
                modde_core::manifest::wabbajack::RawDirective::FromArchive {
                    archive_hash_path: vec![serde_json::Value::Number(mismatch_hash.into())],
                    to: "mods/Update.esm".into(),
                    size: 0,
                },
                modde_core::manifest::wabbajack::RawDirective::FromArchive {
                    archive_hash_path: vec![serde_json::Value::Number(missing_hash.into())],
                    to: "mods/SkyrimSE.exe".into(),
                    size: 0,
                },
            ],
            ..minimal_manifest()
        };
        let mut inst = WabbajackInstaller::new(
            manifest,
            dir.path().join("test.wabbajack"),
            dir.path().join("store"),
            dir.path().join("staging"),
        );
        inst.set_game_dir(game_dir);

        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        let err = inst.install(progress_tx).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("validation failed for 2 file(s)"),
            "unexpected error: {msg}"
        );
        assert!(msg.contains("Data\\Update.esm"), "unexpected error: {msg}");
        assert!(msg.contains("got"), "unexpected error: {msg}");
        assert!(msg.contains("SkyrimSE.exe"), "unexpected error: {msg}");
        assert!(msg.contains("missing"), "unexpected error: {msg}");
    }

    #[tokio::test]
    async fn install_resolves_game_file_path_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("game");
        let store_dir = dir.path().join("store");
        let staging_dir = dir.path().join("staging");
        std::fs::create_dir_all(game_dir.join("DATA")).unwrap();
        let source_bytes = b"case insensitive game file";
        std::fs::write(game_dir.join("DATA/update.ESM"), source_bytes).unwrap();

        let manifest = manifest_with_game_file(
            xxh64(source_bytes, 0),
            "Data\\Update.esm",
            "mods/Update.esm",
        );
        let mut inst = WabbajackInstaller::new(
            manifest,
            dir.path().join("test.wabbajack"),
            store_dir,
            staging_dir.clone(),
        );
        inst.set_game_dir(game_dir);

        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        inst.install(progress_tx).await.unwrap();

        assert_eq!(
            std::fs::read(staging_dir.join("mods/Update.esm")).unwrap(),
            source_bytes
        );
    }

    // -----------------------------------------------------------------------
    // 3. find_entry_in_archive with exact match
    // -----------------------------------------------------------------------
    #[test]
    fn find_entry_exact_match() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("test.zip");
        create_zip_file(&zip_path, &[("data/meshes/test.nif", b"mesh")]);
        let archive = open_zip(&zip_path);

        let result = find_entry_in_archive(&archive, "data/meshes/test.nif").unwrap();
        assert_eq!(result, "data/meshes/test.nif");
    }

    // -----------------------------------------------------------------------
    // 4. find_entry_in_archive with normalized paths (/ vs \)
    // -----------------------------------------------------------------------
    #[test]
    fn find_entry_backslash_to_forward_slash() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("test.zip");
        create_zip_file(&zip_path, &[("data/meshes/test.nif", b"mesh")]);
        let archive = open_zip(&zip_path);

        let result = find_entry_in_archive(&archive, "data\\meshes\\test.nif").unwrap();
        assert_eq!(result, "data/meshes/test.nif");
    }

    #[test]
    fn find_entry_forward_slash_to_backslash() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("test.zip");
        create_zip_file(&zip_path, &[("data\\meshes\\test.nif", b"mesh")]);
        let archive = open_zip(&zip_path);

        let result = find_entry_in_archive(&archive, "data/meshes/test.nif").unwrap();
        assert_eq!(result, "data\\meshes\\test.nif");
    }

    // -----------------------------------------------------------------------
    // 5. find_entry_in_archive with case-insensitive fallback
    // -----------------------------------------------------------------------
    #[test]
    fn find_entry_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("test.zip");
        create_zip_file(&zip_path, &[("Data/Meshes/Test.NIF", b"mesh")]);
        let archive = open_zip(&zip_path);

        let result = find_entry_in_archive(&archive, "data/meshes/test.nif").unwrap();
        assert_eq!(result, "Data/Meshes/Test.NIF");
    }

    #[test]
    fn find_entry_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("test.zip");
        create_zip_file(&zip_path, &[("other.txt", b"data")]);
        let archive = open_zip(&zip_path);

        let result = find_entry_in_archive(&archive, "nonexistent.txt");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // 6. archive_path formatting (hash as 016x)
    // -----------------------------------------------------------------------
    #[test]
    fn archive_path_zero_padded_hex() {
        let store = PathBuf::from("/store");
        let hash: u64 = 0xDEADBEEF;
        let path = archive_path(&store, &hash);
        assert_eq!(path, PathBuf::from("/store/00000000deadbeef.archive"));
    }

    #[test]
    fn archive_path_full_width_hash() {
        let store = PathBuf::from("/store");
        let hash: u64 = 0xFFFFFFFFFFFFFFFF;
        let path = archive_path(&store, &hash);
        assert_eq!(path, PathBuf::from("/store/ffffffffffffffff.archive"));
    }

    #[test]
    fn archive_path_zero_hash() {
        let store = PathBuf::from("/store");
        let hash: u64 = 0;
        let path = archive_path(&store, &hash);
        assert_eq!(path, PathBuf::from("/store/0000000000000000.archive"));
    }

    // -----------------------------------------------------------------------
    // 7. directive_name for each directive type
    // -----------------------------------------------------------------------
    #[test]
    fn directive_name_nexus() {
        let d = DownloadDirective::Nexus {
            game_id: "skyrimse".into(),
            mod_id: 12345,
            file_id: 1,
            hash: 0,
        };
        assert_eq!(d.display_name(), "nexus:12345");
    }

    #[test]
    fn directive_name_github() {
        let d = DownloadDirective::GitHub {
            user: "user".into(),
            repo: "myrepo".into(),
            tag: "v1".into(),
            asset: "a.zip".into(),
            hash: 0,
        };
        assert_eq!(d.display_name(), "github:myrepo");
    }

    #[test]
    fn directive_name_gdrive() {
        let d = DownloadDirective::GoogleDrive {
            id: "abc123".into(),
            hash: 0,
        };
        assert_eq!(d.display_name(), "gdrive:abc123");
    }

    #[test]
    fn directive_name_mega() {
        let d = DownloadDirective::Mega {
            url: "https://mega.nz/file/ABCDEF#key".into(),
            hash: 0,
        };
        let name = d.display_name();
        assert!(name.starts_with("mega:"));
        assert!(name.len() <= 35); // "mega:" + up to 30 chars
    }

    #[test]
    fn directive_name_direct_url() {
        let d = DownloadDirective::DirectURL {
            url: "https://example.com/files/mod.zip".into(),
            headers: HashMap::new(),
            mirror_resolver: None,
            hash: 0,
        };
        let name = d.display_name();
        assert!(name.starts_with("http:"));
        assert!(name.len() <= 35); // "http:" + up to 30 chars
    }

    #[test]
    fn directive_name_mega_short_url() {
        let d = DownloadDirective::Mega {
            url: "https://mega.nz/short".into(),
            hash: 0,
        };
        let name = d.display_name();
        assert_eq!(name, "mega:https://mega.nz/short");
    }

    // -----------------------------------------------------------------------
    // 8. directive_hash extraction for each directive type
    // -----------------------------------------------------------------------
    #[test]
    fn directive_hash_nexus() {
        let d = DownloadDirective::Nexus {
            game_id: "s".into(),
            mod_id: 1,
            file_id: 1,
            hash: 0xABCD,
        };
        assert_eq!(d.hash(), 0xABCD);
    }

    #[test]
    fn directive_hash_github() {
        let d = DownloadDirective::GitHub {
            user: "u".into(),
            repo: "r".into(),
            tag: "t".into(),
            asset: "a".into(),
            hash: 999,
        };
        assert_eq!(d.hash(), 999);
    }

    #[test]
    fn directive_hash_gdrive() {
        let d = DownloadDirective::GoogleDrive {
            id: "x".into(),
            hash: 42,
        };
        assert_eq!(d.hash(), 42);
    }

    #[test]
    fn directive_hash_mega() {
        let d = DownloadDirective::Mega {
            url: "u".into(),
            hash: 7777,
        };
        assert_eq!(d.hash(), 7777);
    }

    #[test]
    fn directive_hash_direct_url() {
        let d = DownloadDirective::DirectURL {
            url: "u".into(),
            headers: HashMap::new(),
            mirror_resolver: None,
            hash: 0xFFFF,
        };
        assert_eq!(d.hash(), 0xFFFF);
    }

    // -----------------------------------------------------------------------
    // 9. extract_from_zip with valid zip
    // -----------------------------------------------------------------------
    #[test]
    fn extract_from_zip_valid() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("test.zip");
        create_zip_file(&zip_path, &[("inner/file.txt", b"hello world")]);

        let data = extract_from_zip(&zip_path, "inner/file.txt").unwrap();
        assert_eq!(data, b"hello world");
    }

    // -----------------------------------------------------------------------
    // 10. extract_from_zip with nonexistent entry
    // -----------------------------------------------------------------------
    #[test]
    fn extract_from_zip_missing_entry() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("test.zip");
        create_zip_file(&zip_path, &[("exists.txt", b"data")]);

        let result = extract_from_zip(&zip_path, "does_not_exist.txt");
        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(err_msg.contains("not found"), "unexpected error: {err_msg}");
    }

    // -----------------------------------------------------------------------
    // 11. normalize_path
    // -----------------------------------------------------------------------
    #[test]
    fn normalize_path_backslashes() {
        assert_eq!(normalize_path("mods\\test\\file.txt"), "mods/test/file.txt");
    }

    #[test]
    fn normalize_path_already_forward() {
        assert_eq!(normalize_path("mods/test/file.txt"), "mods/test/file.txt");
    }
}
