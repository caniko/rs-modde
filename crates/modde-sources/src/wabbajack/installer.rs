use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use futures::stream::{self, StreamExt};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use modde_core::manifest::wabbajack::{
    ArchiveState, DownloadDirective, InstallDirective, WabbajackManifest,
};

use crate::traits::{AnySource, DownloadHandle, DownloadSource};

use super::bsa_repack;
use super::patcher;

/// Default maximum number of concurrent downloads.
const DEFAULT_CONCURRENCY: usize = 4;

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
    /// Cache of fully-extracted non-zip archives (archive path → temp dir with all contents).
    extract_cache: Arc<std::sync::Mutex<HashMap<PathBuf, PathBuf>>>,
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
            extract_cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
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
        let downloads = self.manifest.download_directives();
        let installs = self.manifest.install_directives();

        self.validate_game_file_sources()?;
        self.verify_game_file_sources().await?;

        progress_tx
            .send(InstallProgress::Starting {
                total_downloads: downloads.len(),
            })
            .ok();

        // Step 1: Resolve and download all archives in parallel
        self.download_archives(&downloads, &progress_tx).await?;

        // Step 2: Verify all downloaded archives
        info!("verifying downloaded archives");
        self.verify_archives(&downloads, &progress_tx).await?;

        // Step 3: Apply install directives
        for (i, directive) in installs.iter().enumerate() {
            progress_tx
                .send(InstallProgress::Applying {
                    directive_index: i,
                    total: installs.len(),
                })
                .ok();

            match directive {
                InstallDirective::FromArchive {
                    archive_hash,
                    from,
                    to,
                } => {
                    self.apply_from_archive(*archive_hash, from, to).await?;
                }
                InstallDirective::InlineFile { source_data_id, to } => {
                    progress_tx
                        .send(InstallProgress::InlineFile { name: to.clone() })
                        .ok();
                    self.apply_inline_file(source_data_id, to).await?;
                }
                InstallDirective::PatchedFromArchive {
                    archive_hash,
                    from,
                    to,
                    patch_id,
                } => {
                    progress_tx
                        .send(InstallProgress::Patching { name: to.clone() })
                        .ok();
                    self.apply_patched_from_archive(*archive_hash, from, to, patch_id)
                        .await?;
                }
                InstallDirective::CreateBSA {
                    temp_id,
                    to,
                    file_states,
                } => {
                    progress_tx
                        .send(InstallProgress::CreatingBSA { name: to.clone() })
                        .ok();
                    self.apply_create_bsa(temp_id, to, file_states).await?;
                }
            }
        }

        // Clean up extraction cache temp dirs
        {
            let cache = self.extract_cache.lock().unwrap();
            for (_, dir) in cache.iter() {
                if let Err(e) = std::fs::remove_dir_all(dir) {
                    warn!(dir = %dir.display(), "failed to clean up extract cache: {e}");
                }
            }
        }

        progress_tx.send(InstallProgress::Complete).ok();
        info!("wabbajack installation complete");
        Ok(())
    }

    /// Resolve and download all archives using parallel tasks.
    async fn download_archives(
        &self,
        downloads: &[DownloadDirective],
        progress_tx: &mpsc::UnboundedSender<InstallProgress>,
    ) -> Result<()> {
        let results: Vec<Result<()>> = stream::iter(downloads.iter().enumerate())
            .map(|(idx, directive)| {
                let sources = self.sources.clone();
                let store_dir = self.store_dir.clone();
                let progress_tx = progress_tx.clone();
                async move {
                    let name = directive.display_name().into_owned();

                    // Find a source that can handle this directive
                    let source = sources
                        .iter()
                        .find(|s| s.can_handle(directive));

                    let Some(source) = source else {
                        warn!(name = %name, idx, "no download source registered for directive, skipping");
                        return Ok(());
                    };

                    // Resolve the download URL
                    let handle: DownloadHandle = source
                        .resolve(directive)
                        .await
                        .with_context(|| format!("failed to resolve download: {name}"))?;

                    let dest = archive_path(&store_dir, &handle.expected_hash);
                    if let Some(parent) = dest.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }

                    // Skip if already downloaded
                    if dest.exists() {
                        info!(name = %name, "archive already exists, skipping download");
                        progress_tx
                            .send(InstallProgress::DownloadComplete { name })
                            .ok();
                        return Ok(());
                    }

                    // Report download start
                    progress_tx
                        .send(InstallProgress::Downloading {
                            name: name.clone(),
                            bytes: 0,
                            total: handle.size_hint.unwrap_or(0),
                        })
                        .ok();

                    // Download the file
                    source
                        .download(handle, &dest)
                        .await
                        .with_context(|| format!("failed to download: {name}"))?;

                    progress_tx
                        .send(InstallProgress::DownloadComplete { name })
                        .ok();

                    Ok(())
                }
            })
            .buffer_unordered(self.concurrency)
            .collect()
            .await;

        // Collect any errors
        let errors: Vec<_> = results
            .into_iter()
            .filter_map(std::result::Result::err)
            .collect();

        if !errors.is_empty() {
            let msg = errors
                .iter()
                .map(|e| format!("  - {e:#}"))
                .collect::<Vec<_>>()
                .join("\n");
            error!(count = errors.len(), "download errors occurred");
            anyhow::bail!("failed to download {} archives:\n{msg}", errors.len());
        }

        Ok(())
    }

    /// Verify downloaded archives match expected hashes.
    async fn verify_archives(
        &self,
        downloads: &[DownloadDirective],
        progress_tx: &mpsc::UnboundedSender<InstallProgress>,
    ) -> Result<()> {
        for directive in downloads {
            let hash = directive.hash();
            let name = directive.display_name().into_owned();
            let path = archive_path(&self.store_dir, &hash);

            progress_tx
                .send(InstallProgress::Verifying { name: name.clone() })
                .ok();

            if !path.exists() {
                warn!(name = %name, "archive file not found for verification, skipping");
                continue;
            }

            modde_core::hash::verify_xxh64(&path, hash)
                .await
                .with_context(|| format!("hash verification failed for: {name}"))?;
        }

        info!("all archive hashes verified");
        Ok(())
    }

    /// Extract a file from a downloaded archive and place it in the staging directory.
    async fn apply_from_archive(&self, archive_hash: u64, from: &str, to: &str) -> Result<()> {
        // Validate paths against traversal attacks
        validate_archive_entry(from)?;
        validate_archive_entry(to)?;

        let output_path = self.staging_dir.join(normalize_path(to));

        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
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

    /// Extract inline file data from the `.wabbajack` zip and write to staging.
    async fn apply_inline_file(&self, source_data_id: &str, to: &str) -> Result<()> {
        let wj_path = self.wabbajack_path.clone();
        let sid = source_data_id.to_string();
        let output_path = self.staging_dir.join(normalize_path(to));

        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Validate the output path against traversal attacks
        validate_archive_entry(to)?;

        let data = tokio::task::spawn_blocking(move || {
            let file = std::fs::File::open(&wj_path)
                .with_context(|| format!("failed to open wabbajack file: {}", wj_path.display()))?;
            let mut archive = zip::ZipArchive::new(file)?;
            let mut entry = archive
                .by_name(&sid)
                .with_context(|| format!("inline data entry '{sid}' not found in wabbajack zip"))?;
            validate_zip_entry(&entry)?;
            let mut data = Vec::with_capacity(entry.size() as usize);
            std::io::Read::read_to_end(&mut entry, &mut data)?;
            Ok::<_, anyhow::Error>(data)
        })
        .await??;

        tokio::fs::write(&output_path, &data).await?;

        info!(source_data_id = %source_data_id, to = %to, "wrote inline file");
        Ok(())
    }

    /// Extract a file from archive, apply a binary delta patch, and write to staging.
    async fn apply_patched_from_archive(
        &self,
        archive_hash: u64,
        from: &str,
        to: &str,
        patch_id: &str,
    ) -> Result<()> {
        let output_path = self.staging_dir.join(normalize_path(to));

        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Extract source file from a downloaded archive or local game-file source.
        let source_data = self
            .read_archive_source(archive_hash, from)
            .await
            .with_context(|| {
                format!("failed to extract '{from}' from archive {archive_hash:016x} for patching")
            })?;

        // Validate the output path against traversal attacks
        validate_archive_entry(to)?;

        // Read patch data from the .wabbajack zip
        let wj_path = self.wabbajack_path.clone();
        let pid = patch_id.to_string();
        let patch_data = tokio::task::spawn_blocking(move || {
            let file = std::fs::File::open(&wj_path)?;
            let mut archive = zip::ZipArchive::new(file)?;
            let mut entry = archive
                .by_name(&pid)
                .with_context(|| format!("patch data '{pid}' not found in wabbajack zip"))?;
            validate_zip_entry(&entry)?;
            let mut data = Vec::with_capacity(entry.size() as usize);
            std::io::Read::read_to_end(&mut entry, &mut data)?;
            Ok::<_, anyhow::Error>(data)
        })
        .await??;

        // Apply the binary delta patch
        let patched = patcher::apply_patch(&source_data, &patch_data)
            .with_context(|| format!("failed to apply patch for: {to}"))?;

        tokio::fs::write(&output_path, &patched).await?;

        info!(from = %from, to = %to, patch_id = %patch_id, "applied binary patch");
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

    async fn verify_game_file_sources(&self) -> Result<()> {
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
                bail!(
                    "game-file source '{}' is missing at {}",
                    source.rel_path,
                    source.path.display()
                );
            }

            modde_core::hash::verify_xxh64(&source.path, archive.hash)
                .await
                .with_context(|| {
                    format!(
                        "game-file source '{}' failed hash verification (expected xxh64 {:016x})",
                        source.rel_path, archive.hash
                    )
                })?;
        }

        Ok(())
    }

    async fn read_archive_source(&self, archive_hash: u64, from: &str) -> Result<Vec<u8>> {
        if let Some(source) = self.game_file_source_path(archive_hash)? {
            return read_game_file_source(&source.path, from).await;
        }

        let archive_path = archive_path(&self.store_dir, &archive_hash);
        extract_from_archive_cached(&archive_path, from, &self.extract_cache).await
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

    // Most GameFileSourceDownloader entries represent the whole game file as
    // the source archive. If Wabbajack records the same relative path in the
    // ArchiveHashPath, treat that as a whole-file copy too.
    let normalized_from = normalize_path(from);
    let normalized_path = normalize_path(&path.to_string_lossy());
    let path_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(normalize_path);

    if normalized_path.ends_with(&normalized_from)
        || path_name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(&normalized_from))
    {
        return tokio::fs::read(path)
            .await
            .with_context(|| format!("failed to read game-file source: {}", path.display()));
    }

    extract_from_archive_cached(path, from, &Arc::new(std::sync::Mutex::new(HashMap::new()))).await
}

/// Compute the storage path for an archive by its hash.
fn archive_path(store_dir: &Path, hash: &u64) -> PathBuf {
    store_dir.join(format!("{hash:016x}.archive"))
}

/// Extract a named file from an archive (zip, 7z, or rar) with caching.
///
/// For zip archives, extracts the single file directly.
/// For non-zip archives (7z, rar), extracts the entire archive to a cache directory
/// on first access, then reads from cache on subsequent accesses. This avoids
/// re-parsing the full archive for every individual file.
async fn extract_from_archive_cached(
    archive_path: &Path,
    inner_path: &str,
    cache: &Arc<std::sync::Mutex<HashMap<PathBuf, PathBuf>>>,
) -> Result<Vec<u8>> {
    // Validate the inner path before any extraction
    validate_archive_entry(inner_path)?;

    let archive_path = archive_path.to_path_buf();
    let inner_path = inner_path.to_string();
    let cache = Arc::clone(cache);

    tokio::task::spawn_blocking(move || {
        // Try zip first (fast per-file extraction)
        if let Ok(data) = extract_from_zip(&archive_path, &inner_path) {
            return Ok(data);
        }

        if modde_core::bethesda_archive::ArchiveIndex::has_bethesda_magic(&archive_path)
            .unwrap_or(false)
        {
            let index = modde_core::bethesda_archive::ArchiveIndex::read(&archive_path)
                .with_context(|| {
                    format!("failed to read Bethesda archive {}", archive_path.display())
                })?;
            return index.extract_file(&inner_path);
        }

        // For non-zip: check or populate the extraction cache
        let cache_dir = {
            let mut cache = cache.lock().unwrap();
            if let Some(dir) = cache.get(&archive_path) {
                dir.clone()
            } else {
                // Extract entire archive to a temp dir under the modde data directory
                // (avoids /tmp tmpfs space limits)
                let extract_base = modde_core::paths::modde_data_dir().join("tmp");
                std::fs::create_dir_all(&extract_base)
                    .context("failed to create extract temp base dir")?;
                let tmp_dir = tempfile::Builder::new()
                    .prefix("modde-extract-")
                    .tempdir_in(&extract_base)
                    .context("failed to create temp dir for archive extraction")?;

                if !extract_full_archive(&archive_path, tmp_dir.path())? {
                    anyhow::bail!(
                        "archive extraction failed for {} (tried 7zz, 7z, unrar)",
                        archive_path.display()
                    );
                }

                info!(archive = %archive_path.display(), cache_dir = %tmp_dir.path().display(),
                      "cached full archive extraction");

                // Leak the tempdir so it persists for the duration of the install
                let dir = tmp_dir.keep();
                cache.insert(archive_path.clone(), dir.clone());
                dir
            }
        };

        // Read the file from the cached extraction
        let normalized = inner_path.replace('\\', "/");
        let file_path = cache_dir.join(&normalized);

        if file_path.exists() {
            // Reject symlinks extracted by external tools (7z, unrar)
            if file_path.symlink_metadata()?.file_type().is_symlink() {
                anyhow::bail!("extracted file is a symlink (rejected for security): {inner_path}");
            }
            return Ok(std::fs::read(&file_path)?);
        }

        // Try case-insensitive search
        find_file_case_insensitive(&cache_dir, &normalized).with_context(|| {
            format!(
                "file '{}' not found in cached extraction of {}",
                inner_path,
                archive_path.display()
            )
        })
    })
    .await?
}

/// Find and read a file by case-insensitive path matching in a directory tree.
fn find_file_case_insensitive(base: &Path, relative_path: &str) -> Result<Vec<u8>> {
    validate_archive_entry(relative_path)?;

    let current = find_path_case_insensitive(base, relative_path)?;

    // Final resolved file must not be a symlink either
    if current.symlink_metadata()?.file_type().is_symlink() {
        anyhow::bail!("resolved file is a symlink (rejected for security): {relative_path}");
    }

    Ok(std::fs::read(&current)?)
}

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

/// Try to run a command, returning `true` if it exits successfully.
fn try_extract(cmd: &str, args: &[&std::ffi::OsStr]) -> Result<bool> {
    let status = std::process::Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match status {
        Ok(s) => Ok(s.success()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(anyhow::Error::new(e).context(format!("failed to run {cmd}"))),
    }
}

/// Collect 7-Zip binary candidates for the current platform.
fn platform_7z_candidates() -> Vec<String> {
    #[allow(unused_mut)]
    let mut candidates = vec!["7zz".into(), "7z".into()];

    #[cfg(target_os = "windows")]
    for path in [
        r"C:\Program Files\7-Zip\7z.exe",
        r"C:\Program Files (x86)\7-Zip\7z.exe",
    ] {
        if std::path::Path::new(path).exists() {
            candidates.push(path.into());
        }
    }

    #[cfg(target_os = "macos")]
    for path in ["/opt/homebrew/bin/7z", "/usr/local/bin/7z"] {
        if std::path::Path::new(path).exists() {
            candidates.push(path.into());
        }
    }

    candidates
}

/// Extract an entire archive to `dest_dir`, trying multiple tools.
///
/// Returns `true` if extraction succeeded with any tool.
/// Tries well-known archive tools in order: 7zz → 7z → unrar.
/// On Windows, also checks standard 7-Zip install paths.
/// On macOS, also checks Homebrew paths.
fn extract_full_archive(archive_path: &Path, dest_dir: &Path) -> Result<bool> {
    let out_flag = format!("-o{}", dest_dir.display());
    let archive = archive_path.as_os_str();

    // Build a list of 7z candidates: PATH first, then platform-specific locations
    let sevenz_candidates = platform_7z_candidates();

    for cmd in &sevenz_candidates {
        if try_extract(
            cmd,
            &["x".as_ref(), out_flag.as_ref(), "-y".as_ref(), archive],
        )? {
            return Ok(true);
        }
    }

    // unrar has a different argument layout
    let unrar_dest = format!("{}/", dest_dir.display());
    if try_extract(
        "unrar",
        &["x".as_ref(), "-o+".as_ref(), archive, unrar_dest.as_ref()],
    )? {
        return Ok(true);
    }

    Ok(false)
}

/// Find a file entry in a zip archive, trying multiple path formats.
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
    use std::collections::HashMap;
    use std::io::Write as _;
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
            }],
            ..minimal_manifest()
        }
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
