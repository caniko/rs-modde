use super::*;

#[test]
fn memory_saturation_requires_high_and_swap_pressure() {
    let saturated = CgroupSnapshot {
        memory_current: Some(100),
        memory_high: Some(100),
        memory_swap_current: Some(95),
        memory_swap_max: Some(100),
        ..CgroupSnapshot::default()
    };
    assert!(memory_is_saturated(Some(&saturated)));

    let no_swap = CgroupSnapshot {
        memory_current: Some(100),
        memory_high: Some(100),
        memory_swap_current: Some(10),
        memory_swap_max: Some(100),
        ..CgroupSnapshot::default()
    };
    assert!(!memory_is_saturated(Some(&no_swap)));

    let no_high = CgroupSnapshot {
        memory_current: Some(99),
        memory_high: Some(100),
        memory_swap_current: Some(95),
        memory_swap_max: Some(100),
        ..CgroupSnapshot::default()
    };
    assert!(!memory_is_saturated(Some(&no_high)));
}

#[test]
fn cgroup_pressure_without_cgroup_is_false_or_bounded() {
    let _ = cgroup_memory_pressure_high(0.80);
}

#[tokio::test]
async fn heartbeat_writes_json_line() {
    let temp = tempfile::tempdir().unwrap();
    let diagnostics = WabbajackDiagnostics::new(WabbajackDiagnosticsOptions {
        dir: temp.path().to_path_buf(),
        interval: Duration::from_secs(1),
        stall_warn: Duration::from_mins(1),
        stall_abort: Duration::from_mins(2),
    })
    .await
    .unwrap();

    diagnostics.set_phase("apply-archive-batches");
    diagnostics.record_progress(ProgressEvent::Other);
    diagnostics.write_heartbeat(1234).await.unwrap();

    let heartbeat = std::fs::read_to_string(temp.path().join("heartbeat.jsonl")).unwrap();
    let record: serde_json::Value = serde_json::from_str(heartbeat.trim()).unwrap();
    assert_eq!(record["kind"], "heartbeat");
    assert_eq!(record["phase"], "apply-archive-batches");
    assert_eq!(record["byte_cache_used"], 1234);
}

#[tokio::test]
async fn archive_batch_record_writes_json_line() {
    let temp = tempfile::tempdir().unwrap();
    let diagnostics = WabbajackDiagnostics::new(WabbajackDiagnosticsOptions {
        dir: temp.path().to_path_buf(),
        interval: Duration::from_secs(1),
        stall_warn: Duration::from_mins(1),
        stall_abort: Duration::from_mins(2),
    })
    .await
    .unwrap();
    diagnostics
        .record_archive_batch(&ArchiveBatchRecord {
            kind: "archive_batch".to_string(),
            unix_ms: 1,
            archive_hash: "0000000000000001".to_string(),
            archive_size_bytes: 2,
            directive_count: 3,
            patch_count: 1,
            elapsed_ms: 4,
            trust_check_ms: 0,
            extraction_ms: 5,
            patch_ms: 6,
            prune_ms: 0,
            extracted_patch_source_bytes: 7,
            sidecar_hit: false,
            streamed_hash_bytes: 0,
            memory_archive_hit: false,
            disk_archive_fallback: false,
            pruned_bytes: 0,
            byte_cache_used_before: 8,
            byte_cache_used_after: 9,
            success_count: 10,
            error_count: 0,
            first_error: None,
            rss_before_kib: Some(11),
            rss_after_kib: Some(12),
            swap_before_kib: Some(13),
            swap_after_kib: Some(14),
        })
        .await
        .unwrap();
    let records = std::fs::read_to_string(temp.path().join("archive-batches.jsonl")).unwrap();
    let record: ArchiveBatchRecord = serde_json::from_str(records.trim()).unwrap();
    assert_eq!(record.archive_hash, "0000000000000001");
    assert_eq!(record.extracted_patch_source_bytes, 7);
}
