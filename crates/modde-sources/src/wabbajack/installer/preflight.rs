use super::*;

impl WabbajackInstaller {
    pub(super) fn validate_game_file_sources(&self) -> Result<()> {
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

    pub(super) fn validate_download_sources(&self, downloads: &[DownloadDirective]) -> Result<()> {
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

    pub(super) async fn preflight_authored_files(&self) -> Result<()> {
        for source in self.sources.iter() {
            if let AnySource::WabbajackCdn(source) = source {
                return source.preflight_archives(&self.manifest.archives).await;
            }
        }
        Ok(())
    }

    pub(super) async fn verify_game_file_sources(&self) -> Result<()> {
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

    pub(super) async fn read_archive_source(
        &self,
        archive_hash: u64,
        from: &str,
    ) -> Result<Vec<u8>> {
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

    pub(super) async fn read_archive_source_cached(
        &self,
        archive_hash: u64,
        from: &str,
    ) -> Result<Bytes> {
        if let Some(bytes) = self.patch_source_from_cache(archive_hash, from) {
            return Ok(bytes);
        }
        let data = self.read_archive_source(archive_hash, from).await?;
        Ok(self.cache_patch_source(archive_hash, from, Bytes::from(data)))
    }

    pub(super) fn patch_source_from_cache(&self, archive_hash: u64, from: &str) -> Option<Bytes> {
        self.byte_cache.get(&ByteCacheKey {
            archive_hash,
            inner_path: normalize_path(from),
        })
    }

    pub(super) fn cache_patch_source(&self, archive_hash: u64, from: &str, bytes: Bytes) -> Bytes {
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

    pub(super) fn check_diagnostics_abort(&self) -> Result<()> {
        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.check_abort()?;
        }
        Ok(())
    }

    pub(super) fn game_file_source_path(
        &self,
        archive_hash: u64,
    ) -> Result<Option<GameFileSourcePath>> {
        let Some(archive) = self
            .archives_by_hash
            .get(&archive_hash)
            .map(|&i| &self.manifest.archives[i])
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
