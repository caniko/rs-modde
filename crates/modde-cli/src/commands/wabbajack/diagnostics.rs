#![allow(clippy::wildcard_imports)]
//! Wabbajack diagnostics summary rendering.

use super::*;

pub(super) fn analyze_diagnostics(dir: &Path, json: bool) -> Result<()> {
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
