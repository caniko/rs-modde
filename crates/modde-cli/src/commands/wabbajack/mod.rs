use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Serialize;

use modde_core::manifest::wabbajack::{ArchiveState, WabbajackManifest};
use modde_sources::DownloadSource;
use modde_sources::wabbajack::acquire::{
    AcquireResult, AcquireStatus, DirectAcquireOutcome, MissingArchive as AcquireMissingArchive,
    MissingArchiveSourceKind, import_acquired_archive, missing_archives, try_acquire_manual_direct,
    wait_for_matching_download, wait_for_next_matching_download,
};
use modde_sources::wabbajack::catalog::{
    CatalogFilter, CatalogSource, download_wabbajack_file, fetch_catalog, filter_entries,
    find_entry, hm_snippet_for_source, resolve_download_target,
};
use modde_sources::wabbajack::impact::MissingArchiveImpact;
use modde_sources::wabbajack::import::{ArchiveImportStatus, import_archives};
use modde_sources::wabbajack::runner::parse_wabbajack_manifest;

use crate::WabbajackAction;

mod acquire;
mod assess;
mod catalog;
mod diagnostics;
mod import;
mod manual;

pub(crate) use acquire::{acquire_missing, acquire_status_label};

use assess::assess;
use catalog::{download, hm_snippet, search};
use diagnostics::analyze_diagnostics;
use import::import_archive;
use manual::{manual_links, missing_impact};

pub async fn handle(action: WabbajackAction) -> Result<()> {
    match action {
        WabbajackAction::Search {
            query,
            game,
            source,
            json,
        } => search(query, game, source, json).await,
        WabbajackAction::Download {
            url_or_machine_url,
            output,
        } => download(url_or_machine_url, output).await,
        WabbajackAction::HmSnippet {
            url_or_file,
            profile,
            game,
            game_dir,
            output,
        } => hm_snippet(url_or_file, profile, game, game_dir, output).await,
        WabbajackAction::ImportArchive { manifest, archives } => {
            import_archive(manifest, archives).await
        }
        WabbajackAction::AcquireMissing {
            manifest,
            download_dir,
            data_dir,
            browser_profile,
            include_nexus,
            browser_controller,
            timeout,
            json,
        } => acquire_missing(
            manifest,
            download_dir,
            data_dir,
            browser_profile,
            include_nexus,
            browser_controller,
            timeout,
            json,
        )
        .await
        .map(|_| ()),
        WabbajackAction::Assess {
            manifest,
            profile,
            game_dir,
            json,
        } => assess(manifest, profile, game_dir, json).await,
        WabbajackAction::MissingImpact {
            manifest,
            data_dir,
            json,
            nix_snippet,
        } => missing_impact(manifest, data_dir, json, nix_snippet),
        WabbajackAction::ManualLinks {
            manifest,
            data_dir,
            json,
        } => manual_links(manifest, data_dir, json),
        WabbajackAction::AnalyzeDiagnostics {
            diagnostics_dir,
            json,
        } => analyze_diagnostics(&diagnostics_dir, json),
    }
}

#[derive(Debug, Serialize)]
struct DiagnosticsSummary {
    diagnostics_dir: String,
    heartbeat_count: usize,
    archive_batch_count: usize,
    first_unix_ms: Option<u128>,
    last_unix_ms: Option<u128>,
    last_phase: Option<String>,
    abort_requested: bool,
    max_idle_ms: u128,
    peak_rss_kib: Option<u64>,
    peak_swap_kib: Option<u64>,
    peak_cgroup_memory_bytes: Option<u64>,
    peak_cgroup_swap_bytes: Option<u64>,
    peak_byte_cache_bytes: u64,
    slowest_batches: Vec<BatchSummary>,
}

#[derive(Debug, Serialize)]
struct BatchSummary {
    archive_hash: String,
    directive_count: usize,
    patch_count: usize,
    elapsed_ms: u128,
    trust_check_ms: u128,
    extraction_ms: u128,
    patch_ms: u128,
    prune_ms: u128,
    extracted_patch_source_bytes: u64,
    streamed_hash_bytes: u64,
    sidecar_hit: bool,
    memory_archive_hit: bool,
    disk_archive_fallback: bool,
    pruned_bytes: u64,
    rss_delta_kib: Option<i64>,
    swap_delta_kib: Option<i64>,
    error_count: usize,
    first_error: Option<String>,
}
