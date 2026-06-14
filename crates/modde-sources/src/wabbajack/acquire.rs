//! Acquiring Wabbajack source archives that are missing from the local store:
//! resolving direct downloads, driving browser-assisted downloads, and
//! importing/verifying the resulting files.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use modde_core::manifest::wabbajack::{ArchiveEntry, ArchiveState, WabbajackManifest};
use modde_core::{NexusFileId, NexusModId};
use serde::{Deserialize, Serialize};
use tokio::time::MissedTickBehavior;

use super::import::{ArchiveImportStatus, import_archives};
mod direct;
pub use direct::try_acquire_manual_direct;

#[cfg(test)]
use direct::*;

use super::installer::archive_path;

const STABLE_FOR: Duration = Duration::from_secs(2);
const POLL_EVERY: Duration = Duration::from_millis(500);

/// The kind of source a missing archive must be acquired from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MissingArchiveSourceKind {
    Manual,
    Nexus,
}

/// An archive required by a modlist that is not yet present in the store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingArchive {
    pub hash: u64,
    pub name: String,
    pub size: u64,
    pub source_kind: MissingArchiveSourceKind,
    pub url: Option<String>,
    pub source_hint: String,
}

/// Outcome status for an attempt to acquire a single missing archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AcquireStatus {
    AlreadyPresent,
    OpenedBrowser,
    WaitingForDownload,
    DirectResolved,
    DirectFailed,
    BrowserRequired,
    DnsUnresolved,
    LoginRequired,
    CaptchaRequired,
    Imported,
    Mismatched,
    TimedOut,
    NexusCredentialsMissing,
    UnsupportedSource,
}

/// The result of acquiring one archive: its status and, if obtained, the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcquireResult {
    pub archive: MissingArchive,
    pub status: AcquireStatus,
    pub path: Option<PathBuf>,
    pub computed_xxh64: Option<u64>,
    pub message: Option<String>,
}

/// A newly-observed download file matched against the expected archives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedDownload {
    pub path: PathBuf,
    pub computed_xxh64: u64,
    pub matched: bool,
    pub name_matched: bool,
    pub matched_hash: Option<u64>,
}

/// An event signalling that a browser-driven download produced a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserDownloadEvent {
    pub path: PathBuf,
}

/// The outcome of attempting to acquire an archive via a direct download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectAcquireOutcome {
    Resolved(AcquireResult),
    NeedsBrowser {
        archive: MissingArchive,
        message: String,
    },
    Final(AcquireResult),
    Unsupported,
}

#[derive(Debug, Clone)]
struct CandidateState {
    len: u64,
    unchanged_since: Instant,
    hashed_len: Option<u64>,
}

/// Return manual and optionally Nexus archives that are not present in the store.
///
/// Existing store entries are treated as present by path, matching install-time
/// resumability. Import and install paths still verify hashes before trusting
/// bytes.
pub fn missing_archives(
    manifest: &WabbajackManifest,
    store_dir: &Path,
    include_nexus: bool,
) -> Vec<MissingArchive> {
    manifest
        .archives
        .iter()
        .filter(|archive| !archive_path(store_dir, &archive.hash).exists())
        .filter_map(|archive| missing_archive_entry(archive, include_nexus))
        .collect()
}

fn missing_archive_entry(archive: &ArchiveEntry, include_nexus: bool) -> Option<MissingArchive> {
    let state = archive.state.as_ref()?;
    match state {
        ArchiveState::ManualDownloader { url, .. } => Some(MissingArchive {
            hash: archive.hash,
            name: archive.name.clone(),
            size: archive.size,
            source_kind: MissingArchiveSourceKind::Manual,
            url: Some(url.clone()),
            source_hint: url.clone(),
        }),
        ArchiveState::NexusDownloader {
            game_name,
            mod_id,
            file_id,
        } if include_nexus => Some(MissingArchive {
            hash: archive.hash,
            name: archive.name.clone(),
            size: archive.size,
            source_kind: MissingArchiveSourceKind::Nexus,
            url: nexus_browser_url(game_name, *mod_id, *file_id),
            source_hint: format!("Nexus {game_name} mod_id={mod_id} file_id={file_id}"),
        }),
        _ => None,
    }
}

/// Normalize a Wabbajack game name into its canonical Nexus game domain.
pub fn normalize_nexus_game_domain(game_name: &str) -> String {
    match game_name.to_ascii_lowercase().as_str() {
        "moddingtools" | "modding-tools" | "site" => "site".to_string(),
        other => other.to_string(),
    }
}

/// Build the Nexus mod-file page URL a user can open to download manually.
pub fn nexus_browser_url(
    game_name: &str,
    mod_id: NexusModId,
    file_id: NexusFileId,
) -> Option<String> {
    let domain = normalize_nexus_game_domain(game_name);
    Some(format!(
        "https://www.nexusmods.com/{domain}/mods/{mod_id}?tab=files&file_id={file_id}"
    ))
}

/// Return `true` if `path` looks like an in-progress/partial download file
/// (e.g. `.part`, `.crdownload`).
pub fn partial_download_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".part")
        || lower.ends_with(".crdownload")
        || lower.ends_with(".download")
        || lower.ends_with(".tmp")
        || lower.ends_with(".opdownload")
}

/// Wait for a browser-created file in `download_dir` whose xxh64 matches
/// `archive.hash`.
pub async fn wait_for_matching_download(
    download_dir: &Path,
    archive: &MissingArchive,
    timeout: Duration,
) -> Result<WatchedDownload> {
    tokio::fs::create_dir_all(download_dir)
        .await
        .with_context(|| format!("failed to create {}", download_dir.display()))?;

    let deadline = Instant::now() + timeout;
    let mut interval = tokio::time::interval(POLL_EVERY);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut candidates: HashMap<PathBuf, CandidateState> = HashMap::new();
    let mut mismatched_by_name: Option<WatchedDownload> = None;

    loop {
        if Instant::now() >= deadline {
            if let Some(mismatch) = mismatched_by_name {
                return Ok(mismatch);
            }
            anyhow::bail!("timed out waiting for {}", archive.name);
        }
        interval.tick().await;

        let paths = list_download_candidates(download_dir).await?;
        let current: HashSet<PathBuf> = paths.iter().cloned().collect();
        candidates.retain(|path, _| current.contains(path));

        for path in paths {
            let Ok(meta) = tokio::fs::metadata(&path).await else {
                continue;
            };
            if !meta.is_file() || partial_download_path(&path) {
                continue;
            }
            let len = meta.len();
            let now = Instant::now();
            let state = candidates.entry(path.clone()).or_insert(CandidateState {
                len,
                unchanged_since: now,
                hashed_len: None,
            });
            if state.len != len {
                state.len = len;
                state.unchanged_since = now;
                state.hashed_len = None;
                continue;
            }
            if now.duration_since(state.unchanged_since) < STABLE_FOR {
                continue;
            }
            if state.hashed_len == Some(len) {
                continue;
            }
            state.hashed_len = Some(len);

            let computed = modde_core::hash::hash_file_xxh64(&path)
                .await
                .with_context(|| format!("failed to hash {}", path.display()))?;
            let name_matched = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == archive.name);
            let watched = WatchedDownload {
                path: path.clone(),
                computed_xxh64: computed,
                matched: computed == archive.hash,
                name_matched,
                matched_hash: (computed == archive.hash).then_some(archive.hash),
            };
            if watched.matched {
                return Ok(watched);
            }
            if name_matched {
                mismatched_by_name = Some(watched);
            }
        }
    }
}

/// Wait for the next completed browser download that matches any pending
/// archive by manifest hash.
pub async fn wait_for_next_matching_download(
    download_dir: &Path,
    archives: &[MissingArchive],
    timeout: Duration,
) -> Result<WatchedDownload> {
    tokio::fs::create_dir_all(download_dir)
        .await
        .with_context(|| format!("failed to create {}", download_dir.display()))?;

    let deadline = Instant::now() + timeout;
    let mut interval = tokio::time::interval(POLL_EVERY);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut candidates: HashMap<PathBuf, CandidateState> = HashMap::new();
    let mut mismatched_by_name: Option<WatchedDownload> = None;

    loop {
        if Instant::now() >= deadline {
            if let Some(mismatch) = mismatched_by_name {
                return Ok(mismatch);
            }
            anyhow::bail!("timed out waiting for {} archive(s)", archives.len());
        }
        interval.tick().await;

        let paths = list_download_candidates(download_dir).await?;
        let current: HashSet<PathBuf> = paths.iter().cloned().collect();
        candidates.retain(|path, _| current.contains(path));

        for path in paths {
            let Ok(meta) = tokio::fs::metadata(&path).await else {
                continue;
            };
            if !meta.is_file() || partial_download_path(&path) {
                continue;
            }
            let len = meta.len();
            let now = Instant::now();
            let state = candidates.entry(path.clone()).or_insert(CandidateState {
                len,
                unchanged_since: now,
                hashed_len: None,
            });
            if state.len != len {
                state.len = len;
                state.unchanged_since = now;
                state.hashed_len = None;
                continue;
            }
            if now.duration_since(state.unchanged_since) < STABLE_FOR
                || state.hashed_len == Some(len)
            {
                continue;
            }
            state.hashed_len = Some(len);

            let computed = modde_core::hash::hash_file_xxh64(&path)
                .await
                .with_context(|| format!("failed to hash {}", path.display()))?;
            let matched_archive = archives.iter().find(|archive| archive.hash == computed);
            let name_matched = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| archives.iter().any(|archive| archive.name == name));
            let watched = WatchedDownload {
                path: path.clone(),
                computed_xxh64: computed,
                matched: matched_archive.is_some(),
                name_matched,
                matched_hash: matched_archive.map(|archive| archive.hash),
            };
            if watched.matched {
                return Ok(watched);
            }
            if name_matched {
                mismatched_by_name = Some(watched);
            }
        }
    }
}

/// Return the downloaded file path carried by a [`BrowserDownloadEvent`].
pub fn browser_event_download_path(event: &BrowserDownloadEvent) -> &Path {
    &event.path
}

async fn list_download_candidates(download_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut entries = tokio::fs::read_dir(download_dir)
        .await
        .with_context(|| format!("failed to read {}", download_dir.display()))?;
    let mut paths = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        paths.push(entry.path());
    }
    Ok(paths)
}

pub async fn import_acquired_archive(
    manifest: &WabbajackManifest,
    store_dir: &Path,
    archive: &MissingArchive,
    source_path: &Path,
) -> Result<AcquireResult> {
    let results = import_archives(manifest, store_dir, &[source_path.to_path_buf()]).await?;
    let Some(result) = results.into_iter().next() else {
        anyhow::bail!("archive import returned no result");
    };

    let status = match result.status {
        ArchiveImportStatus::Imported => AcquireStatus::Imported,
        ArchiveImportStatus::AlreadyPresent => AcquireStatus::AlreadyPresent,
        ArchiveImportStatus::Mismatched | ArchiveImportStatus::Unused => AcquireStatus::Mismatched,
    };

    Ok(AcquireResult {
        archive: archive.clone(),
        status,
        path: result
            .store_path
            .or_else(|| Some(source_path.to_path_buf())),
        computed_xxh64: Some(result.computed_xxh64),
        message: result.matched_archive,
    })
}

#[cfg(test)]
mod tests;
