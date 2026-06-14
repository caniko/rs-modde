use super::*;

impl WabbajackInstaller {
    pub(super) async fn ensure_archive_trusted(
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
            .archives_by_hash
            .get(&archive_hash)
            .and_then(|&i| self.manifest.archives[i].download_directive())
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

    pub(super) async fn trusted_archive_for_path(
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

    pub(super) async fn verified_sidecar_valid(&self, path: &Path, archive_hash: u64) -> bool {
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

    pub(super) async fn write_verified_sidecar(
        &self,
        path: &Path,
        archive_hash: u64,
    ) -> Result<()> {
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

    pub(super) async fn maybe_prune_archive(&self, batch: &ArchiveInstallBatch) -> Result<u64> {
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
}
