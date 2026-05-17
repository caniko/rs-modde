use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Serialize;

use modde_core::manifest::wabbajack::{ArchiveState, RawDirective, WabbajackManifest};
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
struct ManualLinkReport {
    name: String,
    hash: u64,
    hash_hex: String,
    size: u64,
    domain: String,
    url: String,
    store_path: PathBuf,
}

fn manual_links(manifest_path: PathBuf, data_dir: Option<PathBuf>, json: bool) -> Result<()> {
    let manifest = parse_wabbajack_manifest(&manifest_path)?;
    let store_dir = data_dir
        .unwrap_or_else(modde_core::paths::modde_data_dir)
        .join("store");
    let links = manual_link_reports(&manifest, &store_dir);

    if json {
        println!("{}", serde_json::to_string_pretty(&links)?);
        return Ok(());
    }

    for link in links {
        println!(
            "{} {:016x} {} bytes {} {}",
            link.name, link.hash, link.size, link.domain, link.url
        );
    }
    Ok(())
}

fn manual_link_reports(manifest: &WabbajackManifest, store_dir: &Path) -> Vec<ManualLinkReport> {
    manifest
        .archives
        .iter()
        .filter_map(|archive| {
            let store_path = store_dir.join(format!("{:016x}.archive", archive.hash));
            if store_path.exists() {
                return None;
            }
            let ArchiveState::ManualDownloader { url, .. } = archive.state.as_ref()? else {
                return None;
            };
            let domain = manual_intervention_domain(url)?;
            Some(ManualLinkReport {
                name: archive.name.clone(),
                hash: archive.hash,
                hash_hex: format!("{:016x}", archive.hash),
                size: archive.size,
                domain: domain.to_string(),
                url: url.clone(),
                store_path,
            })
        })
        .collect()
}

fn manual_intervention_domain(url: &str) -> Option<&'static str> {
    let host = url::Url::parse(url).ok()?.host_str()?.to_ascii_lowercase();
    match host.as_str() {
        "workupload.com" | "www.workupload.com" => Some("workupload.com"),
        "sharemods.com" | "www.sharemods.com" => Some("sharemods.com"),
        "loverslab.com" | "www.loverslab.com" => Some("loverslab.com"),
        _ => None,
    }
}

fn missing_impact(
    manifest_path: PathBuf,
    data_dir: Option<PathBuf>,
    json: bool,
    nix_snippet: bool,
) -> Result<()> {
    let manifest = parse_wabbajack_manifest(&manifest_path)?;
    let store_dir = data_dir
        .unwrap_or_else(modde_core::paths::modde_data_dir)
        .join("store");
    let impact = MissingArchiveImpact::analyze(&manifest, &store_dir);

    if nix_snippet {
        print_missing_archive_nix_snippet(&impact);
        return Ok(());
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&impact)?);
        return Ok(());
    }

    println!("Wabbajack missing archive impact");
    println!("  archives: {}", impact.total_archives);
    println!("  directives: {}", impact.total_directives);
    println!(
        "  missing archives: {} ({} bytes)",
        impact.missing_archives.len(),
        impact.missing_archive_bytes
    );
    println!(
        "  directly blocked directives: {} ({} output bytes)",
        impact.blocked_archive_directives, impact.blocked_output_bytes
    );
    println!(
        "  affected CreateBSA outputs: {}",
        impact.affected_create_bsa.len()
    );
    println!(
        "  omit-mods impact: {} roots, {} directives, {} output bytes",
        impact.omit_mod_roots.len(),
        impact.omit_mod_directives,
        impact.omit_mod_output_bytes
    );
    if !impact.missing_archives.is_empty() {
        println!("  missing inputs:");
        for archive in &impact.missing_archives {
            println!(
                "    {:016x} {} ({} bytes) {}",
                archive.hash, archive.name, archive.size, archive.source_hint
            );
            println!("      store: {}", archive.store_path.display());
            println!(
                "      import: modde wabbajack import-archive '{}' <downloaded-file>",
                manifest_path.display()
            );
        }
    }
    if !impact.omit_mod_roots.is_empty() {
        println!("  omit-mods roots:");
        for root in &impact.omit_mod_roots {
            println!(
                "    {}: {} directives, {} bytes",
                root.name, root.directives, root.output_bytes
            );
        }
    }
    Ok(())
}

fn print_missing_archive_nix_snippet(impact: &MissingArchiveImpact) {
    println!("manualArchives = {{");
    for archive in &impact.missing_archives {
        println!("  # source: {}", archive.source_hint);
        println!("  # expected size: {} bytes", archive.size);
        println!("  {} = {{", nix_string(&archive.name));
        println!("    hash = {};", nix_string(&archive.hash_hex));
        println!("    # path = /path/to/{};", archive.name);
        println!("    optional = true;");
        println!("  }};");
    }
    println!("}};");
}

fn nix_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '$' if chars.peek() == Some(&'{') => out.push_str("\\$"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[derive(Debug, Serialize)]
struct AssessReport {
    manifest_path: String,
    name: String,
    game: String,
    profile_name: String,
    store_path: String,
    archives: usize,
    directives: usize,
    archive_states: BTreeMap<String, usize>,
    directive_types: BTreeMap<String, usize>,
    archive_extensions: BTreeMap<String, usize>,
    downloadable_archives: usize,
    store_present: usize,
    store_missing: Vec<MissingArchive>,
    manual_downloads: Vec<ManualArchive>,
    game_file_sources: GameFileSourceReport,
    staging: StagingReport,
    rar_enabled: bool,
    hard_blockers: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct MissingArchive {
    hash: String,
    name: String,
    state: String,
    store_path: String,
    source: Option<String>,
    remediation: String,
}

#[derive(Debug, Serialize)]
struct ManualArchive {
    hash: String,
    name: String,
    url: String,
    prompt: String,
}

#[derive(Debug, Serialize)]
struct GameFileSourceReport {
    total: usize,
    present: usize,
    missing: Vec<String>,
}

#[derive(Debug, Serialize)]
struct StagingReport {
    path: String,
    exists: bool,
    compatible_layout: bool,
    archive_batch_sentinels: usize,
    archive_batch_total: usize,
    create_bsa_sentinels: usize,
    create_bsa_total: usize,
    layout_action: String,
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

fn analyze_diagnostics(dir: &Path, json: bool) -> Result<()> {
    let heartbeats = read_jsonl::<modde_sources::wabbajack::diagnostics::HeartbeatRecord>(
        &dir.join("heartbeat.jsonl"),
    )?;
    let mut batches = read_jsonl::<modde_sources::wabbajack::diagnostics::ArchiveBatchRecord>(
        &dir.join("archive-batches.jsonl"),
    )?;

    let mut summary = DiagnosticsSummary {
        diagnostics_dir: dir.display().to_string(),
        heartbeat_count: heartbeats.len(),
        archive_batch_count: batches.len(),
        first_unix_ms: heartbeats.first().map(|record| record.unix_ms),
        last_unix_ms: heartbeats.last().map(|record| record.unix_ms),
        last_phase: heartbeats.last().map(|record| record.phase.clone()),
        abort_requested: heartbeats.iter().any(|record| record.abort_requested),
        max_idle_ms: heartbeats
            .iter()
            .map(|record| record.idle_ms)
            .max()
            .unwrap_or(0),
        peak_rss_kib: heartbeats
            .iter()
            .filter_map(|record| record.process.vm_rss_kib)
            .max(),
        peak_swap_kib: heartbeats
            .iter()
            .filter_map(|record| record.process.vm_swap_kib)
            .max(),
        peak_cgroup_memory_bytes: heartbeats
            .iter()
            .filter_map(|record| record.cgroup.as_ref()?.memory_current)
            .max(),
        peak_cgroup_swap_bytes: heartbeats
            .iter()
            .filter_map(|record| record.cgroup.as_ref()?.memory_swap_current)
            .max(),
        peak_byte_cache_bytes: heartbeats
            .iter()
            .map(|record| record.byte_cache_used)
            .max()
            .unwrap_or(0),
        slowest_batches: Vec::new(),
    };

    batches.sort_by_key(|record| std::cmp::Reverse(record.elapsed_ms));
    summary.slowest_batches = batches
        .into_iter()
        .take(10)
        .map(|record| BatchSummary {
            archive_hash: record.archive_hash,
            directive_count: record.directive_count,
            patch_count: record.patch_count,
            elapsed_ms: record.elapsed_ms,
            trust_check_ms: record.trust_check_ms,
            extraction_ms: record.extraction_ms,
            patch_ms: record.patch_ms,
            prune_ms: record.prune_ms,
            extracted_patch_source_bytes: record.extracted_patch_source_bytes,
            streamed_hash_bytes: record.streamed_hash_bytes,
            sidecar_hit: record.sidecar_hit,
            memory_archive_hit: record.memory_archive_hit,
            disk_archive_fallback: record.disk_archive_fallback,
            pruned_bytes: record.pruned_bytes,
            rss_delta_kib: signed_delta(record.rss_before_kib, record.rss_after_kib),
            swap_delta_kib: signed_delta(record.swap_before_kib, record.swap_after_kib),
            error_count: record.error_count,
            first_error: record.first_error,
        })
        .collect();

    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        print_diagnostics_summary(&summary);
    }
    Ok(())
}

fn read_jsonl<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    contents
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(line_number, line)| {
            serde_json::from_str(line).with_context(|| {
                format!(
                    "failed to parse {} line {}",
                    path.display(),
                    line_number + 1
                )
            })
        })
        .collect()
}

fn signed_delta(before: Option<u64>, after: Option<u64>) -> Option<i64> {
    Some(after? as i64 - before? as i64)
}

fn print_diagnostics_summary(summary: &DiagnosticsSummary) {
    println!("Wabbajack diagnostics: {}", summary.diagnostics_dir);
    println!(
        "  heartbeats: {}, archive batches: {}",
        summary.heartbeat_count, summary.archive_batch_count
    );
    println!(
        "  last phase: {}{}",
        summary.last_phase.as_deref().unwrap_or("unknown"),
        if summary.abort_requested {
            " (abort requested)"
        } else {
            ""
        }
    );
    println!("  max idle: {:.1}s", summary.max_idle_ms as f64 / 1000.0);
    println!(
        "  peaks: rss {}, swap {}, cgroup memory {}, cgroup swap {}, byte cache {}",
        format_kib(summary.peak_rss_kib),
        format_kib(summary.peak_swap_kib),
        format_bytes(summary.peak_cgroup_memory_bytes),
        format_bytes(summary.peak_cgroup_swap_bytes),
        format_bytes(Some(summary.peak_byte_cache_bytes))
    );
    if summary.archive_batch_count == 0
        && matches!(
            summary.last_phase.as_deref(),
            Some("download" | "verify" | "trust-check")
        )
    {
        println!("  bottleneck: archive trust/download verification wall before extraction");
    }
    if !summary.slowest_batches.is_empty() {
        println!("  slowest archive batches:");
        for batch in &summary.slowest_batches {
            println!(
                "    {} elapsed {:.1}s trust {:.1}s extract {:.1}s patch {:.1}s prune {:.1}s directives {} patches {} hash-read {} patch-source {} pruned {} sidecar {} memory {} disk {} rss-delta {} swap-delta {} errors {}",
                batch.archive_hash,
                batch.elapsed_ms as f64 / 1000.0,
                batch.trust_check_ms as f64 / 1000.0,
                batch.extraction_ms as f64 / 1000.0,
                batch.patch_ms as f64 / 1000.0,
                batch.prune_ms as f64 / 1000.0,
                batch.directive_count,
                batch.patch_count,
                format_bytes(Some(batch.streamed_hash_bytes)),
                format_bytes(Some(batch.extracted_patch_source_bytes)),
                format_bytes(Some(batch.pruned_bytes)),
                batch.sidecar_hit,
                batch.memory_archive_hit,
                batch.disk_archive_fallback,
                format_signed_kib(batch.rss_delta_kib),
                format_signed_kib(batch.swap_delta_kib),
                batch.error_count,
            );
            if let Some(error) = &batch.first_error {
                println!("      first error: {error}");
            }
        }
    }
}

fn format_kib(value: Option<u64>) -> String {
    value.map_or_else(
        || "n/a".to_string(),
        |kib| format!("{:.1} MiB", kib as f64 / 1024.0),
    )
}

fn format_signed_kib(value: Option<i64>) -> String {
    value.map_or_else(
        || "n/a".to_string(),
        |kib| format!("{:+.1} MiB", kib as f64 / 1024.0),
    )
}

fn format_bytes(value: Option<u64>) -> String {
    value.map_or_else(
        || "n/a".to_string(),
        |bytes| format!("{:.1} MiB", bytes as f64 / 1024.0 / 1024.0),
    )
}

async fn assess(
    manifest_path: PathBuf,
    profile: Option<String>,
    game_dir: Option<PathBuf>,
    json: bool,
) -> Result<()> {
    let manifest = parse_wabbajack_manifest(&manifest_path)?;
    let profile_name = profile.unwrap_or_else(|| manifest.name.clone());
    let staging_path = modde_core::paths::staging_dir().join(&profile_name);
    let store = modde_core::paths::store_dir();

    let mut archive_states = BTreeMap::new();
    let mut archive_extensions = BTreeMap::new();
    let mut downloadable_archives = 0;
    let mut store_present = 0;
    let mut store_missing = Vec::new();
    let mut manual_downloads = Vec::new();
    let mut hard_blockers = Vec::new();
    let mut warnings = Vec::new();

    for archive in &manifest.archives {
        *archive_states
            .entry(archive_state_label(archive.state.as_ref()))
            .or_default() += 1;
        *archive_extensions
            .entry(archive_extension(&archive.name))
            .or_default() += 1;

        if matches!(
            archive.state,
            Some(ArchiveState::GameFileSourceDownloader { .. })
        ) {
            continue;
        }
        downloadable_archives += 1;
        let store_path = store.join(format!("{:016x}.archive", archive.hash));
        if store_path.exists() {
            store_present += 1;
        } else {
            store_missing.push(missing_archive_report(archive, &store));
            if let Some(ArchiveState::ManualDownloader { url, prompt }) = &archive.state {
                manual_downloads.push(ManualArchive {
                    hash: format!("{:016x}", archive.hash),
                    name: archive.name.clone(),
                    url: url.clone(),
                    prompt: prompt.clone(),
                });
            }
        }
    }

    let mut directive_types = BTreeMap::new();
    for directive in &manifest.directives {
        *directive_types
            .entry(directive_label(directive))
            .or_default() += 1;
    }

    let unknown_directives = directive_types.get("Unknown").copied().unwrap_or(0);
    if unknown_directives > 0 {
        hard_blockers.push(format!(
            "{unknown_directives} unsupported Wabbajack directive(s) parsed as Unknown"
        ));
    }
    if !cfg!(feature = "rar")
        && manifest
            .archives
            .iter()
            .any(|archive| archive_extension(&archive.name) == "rar")
    {
        hard_blockers.push("RAR archives are present but the CLI was built without `rar`".into());
    }
    if !store_missing.is_empty() {
        warnings.push(format!(
            "{} downloadable archive(s) are missing from the store",
            store_missing.len()
        ));
    }

    let game_sources = assess_game_file_sources(&manifest, game_dir.as_deref());
    if !game_sources.missing.is_empty() {
        hard_blockers.push(format!(
            "{} game-file source(s) are missing",
            game_sources.missing.len()
        ));
    }

    let compatible_layout = modde_sources::wabbajack::staging::StagingStore::new(&staging_path)
        .has_compatible_layout()
        .await;
    let archive_batch_total = manifest.install_directives_grouped_by_archive().len();
    let create_bsa_total = manifest
        .install_directives()
        .iter()
        .filter(|directive| {
            matches!(
                directive,
                modde_core::manifest::wabbajack::InstallDirective::CreateBSA { .. }
            )
        })
        .count();
    let staging = StagingReport {
        path: staging_path.display().to_string(),
        exists: staging_path.exists(),
        compatible_layout,
        archive_batch_sentinels: count_json_files(staging_path.join("_state/archive-batches")),
        archive_batch_total,
        create_bsa_sentinels: count_json_files(staging_path.join("_state/create-bsa")),
        create_bsa_total,
        layout_action: if !staging_path.exists() {
            "create".into()
        } else if compatible_layout {
            "resume".into()
        } else {
            "adopt".into()
        },
    };
    if staging.exists && !staging.compatible_layout {
        warnings.push("existing staging will be adopted instead of deleted".into());
    }

    let report = AssessReport {
        manifest_path: manifest_path.display().to_string(),
        name: manifest.name.clone(),
        game: manifest.game.clone(),
        profile_name,
        store_path: store.display().to_string(),
        archives: manifest.archives.len(),
        directives: manifest.directives.len(),
        archive_states,
        directive_types,
        archive_extensions,
        downloadable_archives,
        store_present,
        store_missing,
        manual_downloads,
        game_file_sources: game_sources,
        staging,
        rar_enabled: cfg!(feature = "rar"),
        hard_blockers,
        warnings,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_assess_report(&report);
    }

    if !report.hard_blockers.is_empty() {
        anyhow::bail!(
            "assessment found {} hard blocker(s)",
            report.hard_blockers.len()
        );
    }
    Ok(())
}

fn assess_game_file_sources(
    manifest: &WabbajackManifest,
    game_dir: Option<&Path>,
) -> GameFileSourceReport {
    let mut total = 0;
    let mut present = 0;
    let mut missing = Vec::new();
    let Some(game_dir) = game_dir else {
        let required = manifest
            .archives
            .iter()
            .filter(|archive| {
                matches!(
                    archive.state,
                    Some(ArchiveState::GameFileSourceDownloader { .. })
                )
            })
            .count();
        return GameFileSourceReport {
            total: required,
            present: 0,
            missing: if required == 0 {
                Vec::new()
            } else {
                vec!["--game-dir was not provided".into()]
            },
        };
    };

    for archive in &manifest.archives {
        let Some(state @ ArchiveState::GameFileSourceDownloader { .. }) = archive.state.as_ref()
        else {
            continue;
        };
        total += 1;
        let Some(rel) = state.game_file_path() else {
            missing.push(format!("{} has no recognized game-file path", archive.name));
            continue;
        };
        let normalized = rel.replace('\\', "/");
        if game_dir.join(&normalized).exists() {
            present += 1;
        } else {
            missing.push(rel.to_string());
        }
    }

    GameFileSourceReport {
        total,
        present,
        missing,
    }
}

fn print_assess_report(report: &AssessReport) {
    println!("Wabbajack assessment: {} ({})", report.name, report.game);
    println!("  manifest: {}", report.manifest_path);
    println!("  profile: {}", report.profile_name);
    println!("  store: {}", report.store_path);
    println!(
        "  archives: {} (downloadable {}, store present {}, missing {})",
        report.archives,
        report.downloadable_archives,
        report.store_present,
        report.store_missing.len()
    );
    println!("  directives: {}", report.directives);
    println!(
        "  RAR support: {}",
        if report.rar_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  game-file sources: {}/{} present",
        report.game_file_sources.present, report.game_file_sources.total
    );
    println!(
        "  staging: {} (exists: {}, layout: {}, archive sentinels: {}/{}, BSA sentinels: {}/{})",
        report.staging.path,
        report.staging.exists,
        report.staging.layout_action,
        report.staging.archive_batch_sentinels,
        report.staging.archive_batch_total,
        report.staging.create_bsa_sentinels,
        report.staging.create_bsa_total
    );

    if !report.store_missing.is_empty() {
        println!("  missing archives:");
        for archive in report.store_missing.iter().take(20) {
            println!("    {}  {}  {}", archive.hash, archive.state, archive.name);
            println!("      store: {}", archive.store_path);
            if let Some(source) = &archive.source {
                println!("      source: {source}");
            }
            println!("      action: {}", archive.remediation);
        }
        if report.store_missing.len() > 20 {
            println!("    ... and {} more", report.store_missing.len() - 20);
        }
    }
    if !report.hard_blockers.is_empty() {
        println!("  hard blockers:");
        for blocker in &report.hard_blockers {
            println!("    - {blocker}");
        }
    }
    if !report.warnings.is_empty() {
        println!("  warnings:");
        for warning in &report.warnings {
            println!("    - {warning}");
        }
    }
    println!("  readiness:");
    if report.hard_blockers.is_empty() && report.store_missing.is_empty() {
        println!("    - ready for validated staging/deploy");
    } else if report.hard_blockers.is_empty() {
        println!(
            "    - partial staging is possible with `install wabbajack --no-deploy --continue-on-error --skip-validate`"
        );
        println!("    - validated deploy requires resolving the missing archives above");
    } else {
        println!("    - fix hard blockers before starting the install");
    }
    if report.staging.layout_action == "adopt" {
        println!(
            "    - existing staging will be rescued in place; add `--reset-staging` only for a deliberate rebuild"
        );
    }
}

fn missing_archive_report(
    archive: &modde_core::manifest::wabbajack::ArchiveEntry,
    store: &Path,
) -> MissingArchive {
    let hash = format!("{:016x}", archive.hash);
    let store_path = store.join(format!("{hash}.archive"));
    let state = archive_state_label(archive.state.as_ref());
    let source = archive_source_hint(archive.state.as_ref());
    let remediation = match &archive.state {
        Some(ArchiveState::ManualDownloader { .. }) => {
            format!("download manually, then import or place it at {}", store_path.display())
        }
        Some(ArchiveState::NexusDownloader { .. }) => {
            "rerun with a valid Nexus API key/premium session, or import the matching archive manually".into()
        }
        Some(ArchiveState::GameFileSourceDownloader { .. }) => {
            "fix --game-dir or restore the game file source".into()
        }
        Some(_) => "rerun download/import for this archive, then reassess".into(),
        None => "archive has no downloader metadata; import the exact matching file manually".into(),
    };

    MissingArchive {
        hash,
        name: archive.name.clone(),
        state,
        store_path: store_path.display().to_string(),
        source,
        remediation,
    }
}

fn archive_source_hint(state: Option<&ArchiveState>) -> Option<String> {
    match state? {
        ArchiveState::NexusDownloader {
            game_name,
            mod_id,
            file_id,
        } => Some(format!(
            "Nexus game={game_name}, mod_id={mod_id}, file_id={file_id}"
        )),
        ArchiveState::GitHubDownloader {
            user,
            repo,
            tag,
            asset,
        } => Some(format!("GitHub {user}/{repo} tag={tag} asset={asset}")),
        ArchiveState::GoogleDriveDownloader { id } => Some(format!("Google Drive id={id}")),
        ArchiveState::MegaDownloader { url }
        | ArchiveState::MediaFireDownloader { url }
        | ArchiveState::ManualDownloader { url, .. }
        | ArchiveState::HttpDownloader { url, .. } => Some(url.clone()),
        ArchiveState::ModDBDownloader { url, .. } => Some(url.clone()),
        ArchiveState::GameFileSourceDownloader { metadata } => metadata
            .get("File")
            .and_then(serde_json::Value::as_str)
            .map(|file| format!("game file: {file}")),
        ArchiveState::WabbajackCDNDownloader { metadata } => metadata
            .get("Url")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    }
}

fn archive_state_label(state: Option<&ArchiveState>) -> String {
    match state {
        Some(ArchiveState::NexusDownloader { .. }) => "Nexus".into(),
        Some(ArchiveState::GitHubDownloader { .. }) => "GitHub".into(),
        Some(ArchiveState::GoogleDriveDownloader { .. }) => "GoogleDrive".into(),
        Some(ArchiveState::MegaDownloader { .. }) => "Mega".into(),
        Some(ArchiveState::MediaFireDownloader { .. }) => "MediaFire".into(),
        Some(ArchiveState::ManualDownloader { .. }) => "Manual".into(),
        Some(ArchiveState::HttpDownloader { .. }) => "Http".into(),
        Some(ArchiveState::ModDBDownloader { .. }) => "ModDB".into(),
        Some(ArchiveState::GameFileSourceDownloader { .. }) => "GameFileSource".into(),
        Some(ArchiveState::WabbajackCDNDownloader { .. }) => "WabbajackCDN".into(),
        None => "<none>".into(),
    }
}

fn directive_label(directive: &RawDirective) -> String {
    match directive {
        RawDirective::FromArchive { .. } => "FromArchive",
        RawDirective::InlineFile { .. } => "InlineFile",
        RawDirective::RemappedInlineFile { .. } => "RemappedInlineFile",
        RawDirective::PatchedFromArchive { .. } => "PatchedFromArchive",
        RawDirective::CreateBSA { .. } => "CreateBSA",
        RawDirective::Unknown => "Unknown",
    }
    .into()
}

fn archive_extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "<none>".into())
}

fn count_json_files(path: PathBuf) -> usize {
    std::fs::read_dir(path).map_or(0, |entries| {
        entries
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
            .count()
    })
}

async fn search(
    query: Option<String>,
    game: Option<String>,
    source: String,
    json: bool,
) -> Result<()> {
    let source = parse_source(&source)?;
    let client = reqwest::Client::new();
    let entries = fetch_catalog(&client, source).await?;
    let filtered = filter_entries(
        &entries,
        &CatalogFilter {
            query,
            game,
            include_nsfw: true,
            include_down: true,
            ..Default::default()
        },
    );

    if json {
        println!("{}", serde_json::to_string_pretty(&filtered)?);
        return Ok(());
    }

    for entry in filtered {
        println!(
            "{}{}{}",
            entry.title,
            entry
                .version
                .as_ref()
                .map(|v| format!(" v{v}"))
                .unwrap_or_default(),
            entry
                .game
                .as_ref()
                .map(|g| format!(" [{g}]"))
                .unwrap_or_default()
        );
        if let Some(machine) = &entry.machine_url {
            if let Some(repository) = &entry.repository_name {
                println!("  id: {repository}/{machine}");
            } else {
                println!("  id: {machine}");
            }
        }
        println!("  source: {:?}  official: {}", entry.source, entry.official);
        println!("  download: {}", entry.download_url);
    }
    Ok(())
}

async fn import_archive(manifest_path: PathBuf, archives: Vec<PathBuf>) -> Result<()> {
    if archives.is_empty() {
        anyhow::bail!("at least one archive path is required");
    }
    let manifest = parse_wabbajack_manifest(&manifest_path)?;
    let store = modde_core::paths::store_dir();
    let results = import_archives(&manifest, &store, &archives).await?;

    let mut refused = 0_usize;
    for result in &results {
        match result.status {
            ArchiveImportStatus::Imported => {
                println!(
                    "imported {} -> {} ({})",
                    result.source_path.display(),
                    result
                        .store_path
                        .as_ref()
                        .map_or_else(|| "<missing>".into(), |p| p.display().to_string()),
                    result.matched_archive.as_deref().unwrap_or("<unknown>")
                );
            }
            ArchiveImportStatus::AlreadyPresent => {
                println!(
                    "already-present {} -> {} ({})",
                    result.source_path.display(),
                    result
                        .store_path
                        .as_ref()
                        .map_or_else(|| "<missing>".into(), |p| p.display().to_string()),
                    result.matched_archive.as_deref().unwrap_or("<unknown>")
                );
            }
            ArchiveImportStatus::Mismatched => {
                refused += 1;
                eprintln!(
                    "mismatched {}: filename appears in manifest, but computed xxh64 {:016x} does not match any archive hash",
                    result.source_path.display(),
                    result.computed_xxh64
                );
            }
            ArchiveImportStatus::Unused => {
                refused += 1;
                eprintln!(
                    "unused {}: computed xxh64 {:016x} is not referenced by the manifest",
                    result.source_path.display(),
                    result.computed_xxh64
                );
            }
        }
    }

    if refused > 0 {
        anyhow::bail!("refused {refused} archive import(s)");
    }

    Ok(())
}

pub(crate) async fn acquire_missing(
    manifest_path: PathBuf,
    download_dir: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    browser_profile: Option<PathBuf>,
    include_nexus: bool,
    browser_controller: bool,
    timeout_secs: u64,
    json: bool,
) -> Result<Vec<AcquireResult>> {
    let manifest = parse_wabbajack_manifest(&manifest_path)?;
    let data_dir = data_dir.unwrap_or_else(modde_core::paths::modde_data_dir);
    let store_dir = data_dir.join("store");
    let download_dir = download_dir.unwrap_or_else(|| data_dir.join("downloads"));
    tokio::fs::create_dir_all(&download_dir).await?;
    tokio::fs::create_dir_all(&store_dir).await?;

    if browser_profile.is_some() && !browser_controller && !json {
        eprintln!(
            "browser-profile is recorded for operator context; modde uses the system browser opener"
        );
    }

    let missing = missing_archives(&manifest, &store_dir, include_nexus);
    if browser_controller {
        let results = acquire_missing_with_browser_controller(
            &manifest,
            &store_dir,
            &download_dir,
            &data_dir,
            browser_profile.as_deref(),
            missing,
            Duration::from_secs(timeout_secs),
            json,
        )
        .await?;
        if json {
            println!("{}", serde_json::to_string_pretty(&results)?);
        }
        return Ok(results);
    }

    let mut results = Vec::with_capacity(missing.len());
    for archive in missing {
        let result = match archive.source_kind {
            MissingArchiveSourceKind::Manual => {
                match try_acquire_manual_direct(&manifest, &store_dir, &download_dir, &archive)
                    .await?
                {
                    DirectAcquireOutcome::Resolved(result)
                    | DirectAcquireOutcome::Final(result) => {
                        if !json {
                            print_acquire_result(&result);
                        }
                        results.push(result);
                        continue;
                    }
                    DirectAcquireOutcome::NeedsBrowser {
                        archive: _,
                        message,
                    } => {
                        if !json {
                            eprintln!(
                                "browser-required {:016x} {} message={}",
                                archive.hash, archive.name, message
                            );
                        }
                    }
                    DirectAcquireOutcome::Unsupported => {}
                }
                acquire_manual_archive(
                    &manifest,
                    &store_dir,
                    &download_dir,
                    &archive,
                    browser_profile.as_deref(),
                    Duration::from_secs(timeout_secs),
                    json,
                )
                .await?
            }
            MissingArchiveSourceKind::Nexus => {
                acquire_nexus_archive(&manifest, &store_dir, &download_dir, &archive, json).await?
            }
        };
        if !json {
            print_acquire_result(&result);
        }
        results.push(result);
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    }
    Ok(results)
}

async fn acquire_manual_archive(
    manifest: &WabbajackManifest,
    store_dir: &Path,
    download_dir: &Path,
    archive: &AcquireMissingArchive,
    browser_profile: Option<&Path>,
    timeout: Duration,
    json: bool,
) -> Result<AcquireResult> {
    let Some(url) = archive.url.as_deref() else {
        return Ok(acquire_message(
            archive,
            AcquireStatus::UnsupportedSource,
            "manual archive has no URL",
        ));
    };

    if !json {
        eprintln!(
            "opened-browser {:016x} {} -> {}",
            archive.hash, archive.name, url
        );
        if let Some(profile) = browser_profile {
            eprintln!("browser-profile hint: {}", profile.display());
        }
    }
    if let Err(err) = open::that(url) {
        eprintln!("browser-open-failed {url}: {err:#}");
        eprintln!("open this URL manually, then save the archive into the watched directory");
    }

    if !json {
        eprintln!(
            "waiting-for-download {:016x} {} in {}",
            archive.hash,
            archive.name,
            download_dir.display()
        );
    }

    match wait_for_matching_download(download_dir, archive, timeout).await {
        Ok(found) if found.matched => {
            import_acquired_archive(manifest, store_dir, archive, &found.path).await
        }
        Ok(found) => Ok(AcquireResult {
            archive: archive.clone(),
            status: AcquireStatus::Mismatched,
            path: Some(found.path),
            computed_xxh64: Some(found.computed_xxh64),
            message: Some("downloaded file name matched but hash did not".into()),
        }),
        Err(err) => Ok(AcquireResult {
            archive: archive.clone(),
            status: AcquireStatus::TimedOut,
            path: None,
            computed_xxh64: None,
            message: Some(err.to_string()),
        }),
    }
}

async fn acquire_missing_with_browser_controller(
    manifest: &WabbajackManifest,
    store_dir: &Path,
    download_dir: &Path,
    data_dir: &Path,
    browser_profile: Option<&Path>,
    missing: Vec<AcquireMissingArchive>,
    timeout: Duration,
    json: bool,
) -> Result<Vec<AcquireResult>> {
    let mut results = Vec::with_capacity(missing.len());
    let mut pending = Vec::new();

    for archive in missing {
        match archive.source_kind {
            MissingArchiveSourceKind::Manual => {
                match try_acquire_manual_direct(manifest, store_dir, download_dir, &archive).await?
                {
                    DirectAcquireOutcome::Resolved(result)
                    | DirectAcquireOutcome::Final(result) => {
                        if !json {
                            print_acquire_result(&result);
                        }
                        results.push(result);
                    }
                    DirectAcquireOutcome::NeedsBrowser { archive, message } => {
                        if !json {
                            eprintln!(
                                "browser-required {:016x} {} message={}",
                                archive.hash, archive.name, message
                            );
                        }
                        pending.push(archive);
                    }
                    DirectAcquireOutcome::Unsupported => pending.push(archive),
                }
            }
            MissingArchiveSourceKind::Nexus => {
                let result =
                    acquire_nexus_archive(manifest, store_dir, download_dir, &archive, json)
                        .await?;
                if matches!(
                    result.status,
                    AcquireStatus::Imported | AcquireStatus::AlreadyPresent
                ) {
                    if !json {
                        print_acquire_result(&result);
                    }
                    results.push(result);
                } else if archive.url.is_some() {
                    if !json {
                        eprintln!(
                            "nexus-browser-fallback {:016x} {}: {}",
                            archive.hash,
                            archive.name,
                            result
                                .message
                                .as_deref()
                                .unwrap_or("Nexus API did not resolve")
                        );
                    }
                    pending.push(archive);
                } else {
                    if !json {
                        print_acquire_result(&result);
                    }
                    results.push(result);
                }
            }
        }
    }

    if pending.is_empty() {
        return Ok(results);
    }

    if !json {
        eprintln!("pending browser downloads:");
        for archive in &pending {
            eprintln!(
                "  {:016x} {} {}",
                archive.hash,
                archive.name,
                archive.url.as_deref().unwrap_or("<no url>")
            );
        }
    }

    let urls = pending
        .iter()
        .filter_map(|archive| archive.url.clone())
        .collect::<Vec<_>>();
    let default_browser_profile = data_dir.join("browser-profiles/wabbajack-acquire");
    let browser_profile = browser_profile.unwrap_or(&default_browser_profile);
    let mut browser = match launch_chromium_controller(&urls, download_dir, browser_profile) {
        Ok(child) => Some(child),
        Err(err) => {
            eprintln!("browser-controller-unavailable: {err:#}");
            eprintln!(
                "open the listed URLs manually, then save archives into the watched directory"
            );
            None
        }
    };

    let deadline = std::time::Instant::now() + timeout;
    while !pending.is_empty() {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        match wait_for_next_matching_download(download_dir, &pending, remaining).await {
            Ok(found) if found.matched => {
                let Some(hash) = found.matched_hash else {
                    continue;
                };
                let Some(pos) = pending.iter().position(|archive| archive.hash == hash) else {
                    continue;
                };
                let archive = pending.remove(pos);
                let result =
                    import_acquired_archive(manifest, store_dir, &archive, &found.path).await?;
                if !json {
                    print_acquire_result(&result);
                }
                results.push(result);
            }
            Ok(found) => {
                if !json {
                    eprintln!(
                        "mismatched-download path={} computed={:016x}",
                        found.path.display(),
                        found.computed_xxh64
                    );
                }
            }
            Err(err) => {
                if !json {
                    eprintln!("browser-download-wait-ended: {err:#}");
                }
                break;
            }
        }
    }

    if let Some(browser) = &mut browser {
        let _ = browser.kill();
    }
    for archive in pending {
        let result = AcquireResult {
            archive,
            status: AcquireStatus::TimedOut,
            path: None,
            computed_xxh64: None,
            message: Some(
                "browser-controlled acquisition did not observe a matching download".into(),
            ),
        };
        if !json {
            print_acquire_result(&result);
        }
        results.push(result);
    }

    Ok(results)
}

fn launch_chromium_controller(
    urls: &[String],
    download_dir: &Path,
    browser_profile: &Path,
) -> Result<std::process::Child> {
    if urls.is_empty() {
        anyhow::bail!("no URLs to open");
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        anyhow::bail!("no graphical display found ($DISPLAY or $WAYLAND_DISPLAY is required)");
    }
    std::fs::create_dir_all(download_dir)?;
    write_chromium_preferences(browser_profile, download_dir)?;
    let chromium = find_chromium().context("no Chromium-compatible browser found in PATH")?;
    let mut command = std::process::Command::new(chromium);
    command
        .arg(format!("--user-data-dir={}", browser_profile.display()))
        .arg("--no-first-run")
        .arg("--new-window");
    for url in urls {
        command.arg(url);
    }
    command
        .spawn()
        .context("failed to launch Chromium browser controller")
}

fn write_chromium_preferences(browser_profile: &Path, download_dir: &Path) -> Result<()> {
    let default_dir = browser_profile.join("Default");
    std::fs::create_dir_all(&default_dir)?;
    let prefs = serde_json::json!({
        "download": {
            "default_directory": download_dir,
            "directory_upgrade": true,
            "prompt_for_download": false
        },
        "profile": {
            "default_content_setting_values": {
                "automatic_downloads": 1
            }
        }
    });
    std::fs::write(
        default_dir.join("Preferences"),
        serde_json::to_vec_pretty(&prefs)?,
    )?;
    Ok(())
}

fn find_chromium() -> Option<String> {
    if let Ok(path) = std::env::var("MODDE_CHROMIUM")
        && std::process::Command::new(&path)
            .arg("--version")
            .output()
            .is_ok()
    {
        return Some(path);
    }
    [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
        "brave-browser",
        "brave",
    ]
    .into_iter()
    .find(|candidate| {
        std::process::Command::new(candidate)
            .arg("--version")
            .output()
            .is_ok()
    })
    .map(str::to_string)
}

async fn acquire_nexus_archive(
    manifest: &WabbajackManifest,
    store_dir: &Path,
    download_dir: &Path,
    archive: &AcquireMissingArchive,
    json: bool,
) -> Result<AcquireResult> {
    let Some(directive) = manifest
        .download_directives()
        .into_iter()
        .find(|directive| directive.hash() == archive.hash)
    else {
        return Ok(acquire_message(
            archive,
            AcquireStatus::UnsupportedSource,
            "Nexus archive did not produce a download directive",
        ));
    };

    let client = reqwest::Client::new();
    let source = match modde_sources::nexus::NexusSource::new(client) {
        Ok(source) => source,
        Err(err) => {
            return Ok(acquire_message(
                archive,
                AcquireStatus::NexusCredentialsMissing,
                &format!("{err:#}"),
            ));
        }
    };

    if !json {
        eprintln!(
            "resolving-nexus {:016x} {}",
            archive.hash, archive.source_hint
        );
    }
    let handle = match source.resolve(&directive).await {
        Ok(handle) => handle,
        Err(err) => {
            return Ok(acquire_message(
                archive,
                AcquireStatus::NexusCredentialsMissing,
                &format!("{err:#}"),
            ));
        }
    };

    let dest = download_dir.join(safe_archive_file_name(&archive.name));
    let verified = match source.download(handle, &dest).await {
        Ok(verified) => verified,
        Err(err) => {
            return Ok(acquire_message(
                archive,
                AcquireStatus::Mismatched,
                &format!("{err:#}"),
            ));
        }
    };
    import_acquired_archive(manifest, store_dir, archive, &verified.path).await
}

fn safe_archive_file_name(name: &str) -> String {
    Path::new(name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("archive.download")
        .to_string()
}

fn acquire_message(
    archive: &AcquireMissingArchive,
    status: AcquireStatus,
    message: &str,
) -> AcquireResult {
    AcquireResult {
        archive: archive.clone(),
        status,
        path: None,
        computed_xxh64: None,
        message: Some(message.to_string()),
    }
}

fn print_acquire_result(result: &AcquireResult) {
    let archive = &result.archive;
    let path = result
        .path
        .as_ref()
        .map_or_else(String::new, |path| format!(" path={}", path.display()));
    let hash = result
        .computed_xxh64
        .map_or_else(String::new, |hash| format!(" computed={hash:016x}"));
    let message = result
        .message
        .as_ref()
        .map_or_else(String::new, |message| format!(" message={message}"));
    println!(
        "{} {:016x} {}{}{}{}",
        acquire_status_label(&result.status),
        archive.hash,
        archive.name,
        path,
        hash,
        message
    );
}

pub(crate) fn acquire_status_label(status: &AcquireStatus) -> &'static str {
    match status {
        AcquireStatus::AlreadyPresent => "already-present",
        AcquireStatus::OpenedBrowser => "opened-browser",
        AcquireStatus::WaitingForDownload => "waiting-for-download",
        AcquireStatus::DirectResolved => "direct-resolved",
        AcquireStatus::DirectFailed => "direct-failed",
        AcquireStatus::BrowserRequired => "browser-required",
        AcquireStatus::DnsUnresolved => "dns-unresolved",
        AcquireStatus::LoginRequired => "login-required",
        AcquireStatus::CaptchaRequired => "captcha-required",
        AcquireStatus::Imported => "imported",
        AcquireStatus::Mismatched => "mismatched",
        AcquireStatus::TimedOut => "timed-out",
        AcquireStatus::NexusCredentialsMissing => "nexus-credentials-missing",
        AcquireStatus::UnsupportedSource => "unsupported-source",
    }
}

fn parse_source(source: &str) -> Result<CatalogSource> {
    match source {
        "official" => Ok(CatalogSource::Official),
        "authored" => Ok(CatalogSource::Authored),
        "both" => Ok(CatalogSource::Both),
        other => anyhow::bail!(
            "invalid Wabbajack source '{other}' (expected official, authored, or both)"
        ),
    }
}

async fn download(url_or_machine_url: String, output: Option<PathBuf>) -> Result<()> {
    let client = reqwest::Client::new();
    let url = resolve_download_target(&client, &url_or_machine_url, CatalogSource::Both).await?;
    let output = output.unwrap_or_else(modde_core::paths::downloads_dir);
    let path = download_wabbajack_file(&client, &url, &output).await?;
    println!("{}", path.display());
    Ok(())
}

async fn hm_snippet(
    url_or_file: String,
    profile: String,
    game: String,
    game_dir: Option<PathBuf>,
    output: Option<PathBuf>,
) -> Result<()> {
    let client = reqwest::Client::new();
    let cache_dir = modde_core::paths::downloads_dir().join("wabbajack");
    let source = if std::path::Path::new(&url_or_file).exists()
        || url_or_file.starts_with("http://")
        || url_or_file.starts_with("https://")
    {
        url_or_file
    } else {
        let entries = fetch_catalog(&client, CatalogSource::Both).await?;
        find_entry(&entries, &url_or_file)
            .map(|entry| entry.download_url.clone())
            .with_context(|| format!("no Wabbajack catalog entry matches '{url_or_file}'"))?
    };
    let (snippet, cached_path) = hm_snippet_for_source(
        &client,
        &source,
        &profile,
        &game,
        game_dir.as_deref(),
        &cache_dir,
    )
    .await?;

    if let Some(output) = output {
        if let Some(parent) = output.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&output, &snippet)
            .await
            .with_context(|| format!("failed to write {}", output.display()))?;
        println!("{}", output.display());
    } else {
        print!("{snippet}");
    }

    if let Some(path) = cached_path {
        eprintln!("hashed: {}", path.display());
    }
    Ok(())
}
