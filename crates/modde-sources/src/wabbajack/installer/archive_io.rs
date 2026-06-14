use super::*;

/// Compute the storage path for an archive by its hash.
pub(crate) fn archive_path(store_dir: &Path, hash: &u64) -> PathBuf {
    store_dir.join(format!("{hash:016x}.archive"))
}

pub(super) fn verified_sidecar_path(path: &Path) -> PathBuf {
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(".verified.json");
    PathBuf::from(sidecar)
}

pub(super) fn metadata_modified_unix_ms(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

pub(super) fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(super) async fn archive_path_is_memory_supported(path: &Path) -> bool {
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

pub(super) async fn extract_trusted_archive_requests(
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

pub(super) async fn extract_archive_path_requests(
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

pub(super) async fn extract_nested_archive_requests(
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

pub(super) fn is_unsupported_in_memory_archive(error: &anyhow::Error) -> bool {
    format!("{error:#}").contains("unsupported in-memory archive format")
}

pub(super) fn archive_patch_chunk_size() -> usize {
    std::env::var("MODDE_ARCHIVE_PATCH_CHUNK_SIZE")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|&value| value > 0)
        .unwrap_or(8)
}

pub(super) async fn write_bytes_maybe_zstd(path: &Path, data: &[u8]) -> Result<()> {
    if super::super::staging::is_compressed_path(path) {
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
