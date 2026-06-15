#![allow(clippy::wildcard_imports)]
use super::*;

impl WabbajackInstaller {
    pub(super) async fn apply_archive_batch(
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

        self.apply_nested_patch_groups(
            nested_patch_groups,
            trusted_archive.clone(),
            inline_source,
            patch_chunk_size,
            &mut patch_directives,
            &mut results,
            &mut extraction_ms,
            &mut patch_ms,
            &mut extracted_patch_source_bytes,
        )
        .await;

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
}
