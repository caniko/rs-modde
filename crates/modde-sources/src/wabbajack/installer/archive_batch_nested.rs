#![allow(clippy::wildcard_imports)]
use super::*;

impl WabbajackInstaller {
    pub(super) async fn apply_nested_patch_groups(
        &self,
        nested_patch_groups: HashMap<String, Vec<ArchiveRequest>>,
        trusted_archive: Option<TrustedArchive>,
        inline_source: Option<&InlineSource>,
        patch_chunk_size: usize,
        patch_directives: &mut HashMap<usize, (u64, String, Option<String>, String, String, u64)>,
        results: &mut Vec<(usize, Result<()>)>,
        extraction_ms: &mut u128,
        patch_ms: &mut u128,
        extracted_patch_source_bytes: &mut u64,
    ) {
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
            *extraction_ms += extraction_started.elapsed().as_millis();
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
                *extraction_ms += extraction_started.elapsed().as_millis();
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
                        *extracted_patch_source_bytes += bytes.len() as u64;
                        let cache_key = inner_path.as_deref().unwrap_or(&from);
                        let bytes =
                            self.cache_patch_source(archive_hash, cache_key, Bytes::from(bytes));
                        let patch_started = Instant::now();
                        results.push((
                            request.directive_index,
                            self.write_patched_output(inline_source, bytes, &to, &patch_id, size)
                                .await,
                        ));
                        *patch_ms += patch_started.elapsed().as_millis();
                    }
                }
            }
        }
    }
}
