//! Acquiring Wabbajack source archives that are missing from the local store:
//! resolving direct downloads, driving browser-assisted downloads, and
//! importing/verifying the resulting files.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use futures::StreamExt;
use modde_core::manifest::wabbajack::{ArchiveEntry, ArchiveState, WabbajackManifest};
use modde_core::{NexusFileId, NexusModId};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::time::MissedTickBehavior;
use xxhash_rust::xxh64::{Xxh64, xxh64};

use super::import::{ArchiveImportStatus, import_archives};
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

pub async fn try_acquire_manual_direct(
    manifest: &WabbajackManifest,
    store_dir: &Path,
    download_dir: &Path,
    archive: &MissingArchive,
) -> Result<DirectAcquireOutcome> {
    let Some(url) = archive.url.as_deref() else {
        return Ok(DirectAcquireOutcome::Unsupported);
    };
    let parsed = Url::parse(url).with_context(|| format!("invalid manual URL: {url}"))?;
    let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) else {
        return Ok(DirectAcquireOutcome::Unsupported);
    };

    if host == "loverslab.com" || host == "www.loverslab.com" {
        return resolve_loverslab_manual(&host, archive).await;
    }

    if !matches!(
        host.as_str(),
        "workupload.com" | "www.workupload.com" | "sharemods.com" | "www.sharemods.com"
    ) {
        return Ok(DirectAcquireOutcome::Unsupported);
    }

    tokio::fs::create_dir_all(download_dir)
        .await
        .with_context(|| format!("failed to create {}", download_dir.display()))?;
    let client = Client::builder()
        .cookie_store(true)
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) modde/manual-acquire")
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("failed to build manual acquisition HTTP client")?;
    let safe_name = safe_archive_file_name(&archive.name);
    let final_path = download_dir.join(safe_name);

    match resolve_manual_http_flow(&client, parsed, archive, &final_path).await? {
        Some((path, computed)) => {
            let mut result = import_acquired_archive(manifest, store_dir, archive, &path).await?;
            if matches!(
                result.status,
                AcquireStatus::Imported | AcquireStatus::AlreadyPresent
            ) {
                result.status = AcquireStatus::DirectResolved;
                result.computed_xxh64 = Some(computed);
                result.message = Some("direct HTTP flow resolved and verified".into());
            }
            Ok(DirectAcquireOutcome::Resolved(result))
        }
        None => Ok(DirectAcquireOutcome::NeedsBrowser {
            archive: archive.clone(),
            message: "direct HTTP flow did not expose a matching archive".into(),
        }),
    }
}

async fn resolve_loverslab_manual(
    host: &str,
    archive: &MissingArchive,
) -> Result<DirectAcquireOutcome> {
    if tokio::net::lookup_host((host, 443)).await.is_err() {
        return Ok(DirectAcquireOutcome::Final(AcquireResult {
            archive: archive.clone(),
            status: AcquireStatus::DnsUnresolved,
            path: None,
            computed_xxh64: None,
            message: Some(format!(
                "{host} did not resolve; fix DNS/network access before acquiring this archive"
            )),
        }));
    }
    Ok(DirectAcquireOutcome::NeedsBrowser {
        archive: archive.clone(),
        message: "LoversLab downloads require a live browser login/session; stale manifest CSRF URLs are not trusted".into(),
    })
}

async fn resolve_manual_http_flow(
    client: &Client,
    start_url: Url,
    archive: &MissingArchive,
    final_path: &Path,
) -> Result<Option<(PathBuf, u64)>> {
    let mut pages = vec![start_url];
    let mut seen = HashSet::new();

    for _ in 0..4 {
        let Some(url) = pages.pop() else {
            break;
        };
        if !seen.insert(url.clone()) {
            continue;
        }

        let html = client
            .get(url.clone())
            .send()
            .await
            .with_context(|| format!("failed to fetch {url}"))?
            .error_for_status()
            .with_context(|| format!("{url} returned an HTTP error"))?
            .text()
            .await
            .with_context(|| format!("failed to read {url}"))?;

        for action in extract_manual_actions(&html, &url, archive)? {
            match action.method {
                ManualActionMethod::Get => {
                    if let Some(download) =
                        try_direct_download(client, action.url, final_path, archive.hash).await?
                    {
                        return Ok(Some(download));
                    }
                }
                ManualActionMethod::Post(fields) => {
                    let response = client
                        .post(action.url.clone())
                        .form(&fields)
                        .send()
                        .await
                        .with_context(|| format!("failed to submit {}", action.url))?
                        .error_for_status()
                        .with_context(|| format!("{} returned an HTTP error", action.url))?;
                    let body = response.bytes().await.with_context(|| {
                        format!("failed to read form response from {}", action.url)
                    })?;
                    if xxh64(&body, 0) == archive.hash
                        && let Some(download) =
                            write_verified_bytes(&body, final_path, archive.hash).await?
                    {
                        return Ok(Some(download));
                    }
                    if response_looks_like_html_bytes(&body) {
                        let html = String::from_utf8_lossy(&body);
                        for nested in extract_manual_actions(&html, &action.url, archive)? {
                            match nested.method {
                                ManualActionMethod::Get => {
                                    if let Some(download) = try_direct_download(
                                        client,
                                        nested.url,
                                        final_path,
                                        archive.hash,
                                    )
                                    .await?
                                    {
                                        return Ok(Some(download));
                                    }
                                }
                                ManualActionMethod::Post(_) => {
                                    pages.push(nested.url);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(None)
}

#[derive(Debug, Clone)]
struct ManualAction {
    url: Url,
    method: ManualActionMethod,
}

#[derive(Debug, Clone)]
enum ManualActionMethod {
    Get,
    Post(Vec<(String, String)>),
}

fn extract_manual_actions(
    html: &str,
    base_url: &Url,
    archive: &MissingArchive,
) -> Result<Vec<ManualAction>> {
    let mut actions = Vec::new();
    actions.extend(
        extract_page_links(html, base_url, archive)?
            .into_iter()
            .map(|url| ManualAction {
                url,
                method: ManualActionMethod::Get,
            }),
    );

    let mut rest = html;
    while let Some(start) = rest.find("<form") {
        let fragment = &rest[start..];
        let Some(open_end) = fragment.find('>') else {
            break;
        };
        let tag = &fragment[..=open_end];
        let Some(close) = fragment.find("</form>") else {
            rest = &fragment[open_end + 1..];
            continue;
        };
        let form = &fragment[..close + "</form>".len()];
        rest = &fragment[close + "</form>".len()..];
        if !fragment_looks_download_related(form, archive) {
            continue;
        }
        let action = attr_value(tag, "action").unwrap_or_else(|| base_url.as_str().to_string());
        let url = absolutize_url(base_url, &action)?;
        let method = attr_value(tag, "method").unwrap_or_else(|| "get".into());
        if method.eq_ignore_ascii_case("post") {
            actions.push(ManualAction {
                url,
                method: ManualActionMethod::Post(extract_form_fields(form)),
            });
        } else {
            actions.push(ManualAction {
                url,
                method: ManualActionMethod::Get,
            });
        }
    }

    Ok(actions)
}

fn extract_page_links(html: &str, base_url: &Url, archive: &MissingArchive) -> Result<Vec<Url>> {
    let mut urls = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<a") {
        let fragment = &rest[start..];
        let Some(end) = fragment.find('>') else {
            break;
        };
        let tag = &fragment[..=end];
        let link_fragment = fragment
            .find("</a>")
            .map_or(tag, |close| &fragment[..close + "</a>".len()]);
        rest = &fragment[end + 1..];
        if !fragment_looks_download_related(link_fragment, archive) {
            continue;
        }
        let Some(href) = attr_value(tag, "href") else {
            continue;
        };
        if href.starts_with('#')
            || href.starts_with("javascript:")
            || href.starts_with("mailto:")
            || href.starts_with("tel:")
        {
            continue;
        }
        urls.push(absolutize_url(base_url, &href)?);
    }
    Ok(urls)
}

fn fragment_looks_download_related(fragment: &str, archive: &MissingArchive) -> bool {
    let lower = fragment.to_ascii_lowercase();
    lower.contains("download")
        || lower.contains("create download link")
        || lower.contains("start download")
        || lower.contains(&archive.name.to_ascii_lowercase())
        || lower.contains(".7z")
        || lower.contains(".zip")
        || lower.contains(".rar")
}

fn extract_form_fields(form: &str) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let mut rest = form;
    while let Some(start) = rest.find("<input") {
        let fragment = &rest[start..];
        let Some(end) = fragment.find('>') else {
            break;
        };
        let tag = &fragment[..=end];
        rest = &fragment[end + 1..];
        let Some(name) = attr_value(tag, "name") else {
            continue;
        };
        let value = attr_value(tag, "value").unwrap_or_default();
        fields.push((name, value));
    }
    fields
}

async fn try_direct_download(
    client: &Client,
    url: Url,
    final_path: &Path,
    expected_hash: u64,
) -> Result<Option<(PathBuf, u64)>> {
    let response = client
        .get(url.clone())
        .send()
        .await
        .with_context(|| format!("failed to fetch candidate download {url}"))?
        .error_for_status()
        .with_context(|| format!("candidate download {url} returned an HTTP error"))?;
    if response_looks_like_html(&response) {
        return Ok(None);
    }
    stream_response_to_verified_path(response, final_path, expected_hash).await
}

async fn stream_response_to_verified_path(
    response: reqwest::Response,
    final_path: &Path,
    expected_hash: u64,
) -> Result<Option<(PathBuf, u64)>> {
    let Some(parent) = final_path.parent() else {
        anyhow::bail!("download path has no parent: {}", final_path.display());
    };
    tokio::fs::create_dir_all(parent).await?;
    let temp_path = final_path.with_extension("modde-direct-part");
    let mut file = tokio::fs::File::create(&temp_path)
        .await
        .with_context(|| format!("failed to create {}", temp_path.display()))?;
    let mut hasher = Xxh64::new(0);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("failed to read direct download chunk")?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);

    let computed = hasher.digest();
    if computed != expected_hash {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Ok(None);
    }
    if final_path.exists() {
        tokio::fs::remove_file(final_path)
            .await
            .with_context(|| format!("failed to replace {}", final_path.display()))?;
    }
    tokio::fs::rename(&temp_path, final_path)
        .await
        .with_context(|| {
            format!(
                "failed to move {} to {}",
                temp_path.display(),
                final_path.display()
            )
        })?;
    Ok(Some((final_path.to_path_buf(), computed)))
}

async fn write_verified_bytes(
    bytes: &[u8],
    final_path: &Path,
    expected_hash: u64,
) -> Result<Option<(PathBuf, u64)>> {
    let computed = xxh64(bytes, 0);
    if computed != expected_hash {
        return Ok(None);
    }
    if let Some(parent) = final_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(final_path, bytes)
        .await
        .with_context(|| format!("failed to write {}", final_path.display()))?;
    Ok(Some((final_path.to_path_buf(), computed)))
}

fn response_looks_like_html(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/html"))
}

fn response_looks_like_html_bytes(bytes: &[u8]) -> bool {
    let prefix_len = bytes.len().min(1024);
    let prefix = String::from_utf8_lossy(&bytes[..prefix_len]).to_ascii_lowercase();
    prefix.contains("<html") || prefix.contains("<form") || prefix.contains("<a ")
}

fn safe_archive_file_name(name: &str) -> String {
    Path::new(name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("archive.download")
        .to_string()
}

fn absolutize_url(base: &Url, href: &str) -> Result<Url> {
    base.join(href)
        .with_context(|| format!("invalid manual download href {href}"))
}

fn attr_value(tag: &str, attr: &str) -> Option<String> {
    let mut rest = tag;
    loop {
        let idx = rest.find(attr)?;
        let before = rest[..idx].chars().next_back();
        let after = rest[idx + attr.len()..].chars().next();
        if before.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            || after.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        {
            rest = &rest[idx + attr.len()..];
            continue;
        }

        let mut value = rest[idx + attr.len()..].trim_start();
        if !value.starts_with('=') {
            rest = &rest[idx + attr.len()..];
            continue;
        }
        value = value[1..].trim_start();
        let quote = value.chars().next()?;
        if quote == '"' || quote == '\'' {
            let value = &value[quote.len_utf8()..];
            let end = value.find(quote)?;
            return Some(value[..end].to_string());
        }
        let end = value
            .find(|ch: char| ch.is_whitespace() || ch == '>')
            .unwrap_or(value.len());
        return Some(value[..end].to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use modde_core::manifest::wabbajack::WabbajackManifest;
    use tempfile::TempDir;
    use tokio::io::AsyncWriteExt;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use xxhash_rust::xxh64::xxh64;

    fn manifest_for(bytes: &[u8]) -> WabbajackManifest {
        WabbajackManifest {
            name: "test".into(),
            author: "a".into(),
            description: "d".into(),
            game: "SkyrimSE".into(),
            version: "1".into(),
            archives: vec![ArchiveEntry {
                hash: xxh64(bytes, 0),
                name: "manual.7z".into(),
                size: bytes.len() as u64,
                state: Some(ArchiveState::ManualDownloader {
                    url: "https://example.test/manual.7z".into(),
                    prompt: String::new(),
                }),
            }],
            directives: vec![],
        }
    }

    fn manifest_with_manual_and_nexus(manual: &[u8], nexus: &[u8]) -> WabbajackManifest {
        WabbajackManifest {
            name: "test".into(),
            author: "a".into(),
            description: "d".into(),
            game: "SkyrimSE".into(),
            version: "1".into(),
            archives: vec![
                ArchiveEntry {
                    hash: xxh64(manual, 0),
                    name: "manual.7z".into(),
                    size: manual.len() as u64,
                    state: Some(ArchiveState::ManualDownloader {
                        url: "https://example.test/manual.7z".into(),
                        prompt: String::new(),
                    }),
                },
                ArchiveEntry {
                    hash: xxh64(nexus, 0),
                    name: "nexus.7z".into(),
                    size: nexus.len() as u64,
                    state: Some(ArchiveState::NexusDownloader {
                        game_name: "SkyrimSpecialEdition".into(),
                        mod_id: 631.into(),
                        file_id: 5118.into(),
                    }),
                },
            ],
            directives: vec![],
        }
    }

    #[test]
    fn missing_archive_scanner_skips_existing_store_file() {
        let bytes = b"archive";
        let manifest = manifest_for(bytes);
        let temp = TempDir::new().unwrap();
        let store = temp.path().join("store");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(archive_path(&store, &manifest.archives[0].hash), bytes).unwrap();

        assert!(missing_archives(&manifest, &store, false).is_empty());
    }

    #[test]
    fn missing_archive_scanner_reports_manual_archive() {
        let manifest = manifest_for(b"archive");
        let temp = TempDir::new().unwrap();
        let missing = missing_archives(&manifest, &temp.path().join("store"), false);

        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].source_kind, MissingArchiveSourceKind::Manual);
        assert_eq!(missing[0].name, "manual.7z");
    }

    #[test]
    fn missing_archive_scanner_omits_nexus_unless_included() {
        let manifest = manifest_with_manual_and_nexus(b"manual archive", b"nexus archive");
        let temp = TempDir::new().unwrap();
        let store = temp.path().join("store");

        let manual_only = missing_archives(&manifest, &store, false);
        assert_eq!(manual_only.len(), 1);
        assert_eq!(manual_only[0].source_kind, MissingArchiveSourceKind::Manual);

        let all = missing_archives(&manifest, &store, true);
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|archive| {
            archive.source_kind == MissingArchiveSourceKind::Nexus
                && archive.source_hint.contains("mod_id=631")
                && archive.source_hint.contains("file_id=5118")
        }));
    }

    #[test]
    fn missing_archive_scanner_reports_only_entries_missing_from_store() {
        let manifest = manifest_with_manual_and_nexus(b"manual archive", b"nexus archive");
        let temp = TempDir::new().unwrap();
        let store = temp.path().join("store");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(
            archive_path(&store, &manifest.archives[0].hash),
            b"manual archive",
        )
        .unwrap();

        let missing = missing_archives(&manifest, &store, true);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].name, "nexus.7z");
        assert_eq!(missing[0].source_kind, MissingArchiveSourceKind::Nexus);
    }

    #[test]
    fn partial_download_extensions_are_rejected() {
        assert!(partial_download_path(Path::new("file.7z.crdownload")));
        assert!(partial_download_path(Path::new("file.7z.part")));
        assert!(partial_download_path(Path::new("file.7z.download")));
        assert!(partial_download_path(Path::new("file.7z.tmp")));
        assert!(partial_download_path(Path::new("file.7z.opdownload")));
        assert!(!partial_download_path(Path::new("file.7z")));
    }

    #[test]
    fn modding_tools_nexus_entries_get_site_browser_url() {
        assert_eq!(normalize_nexus_game_domain("ModdingTools"), "site");
        assert_eq!(
            nexus_browser_url(
                "ModdingTools",
                NexusModId::from(631),
                NexusFileId::from(5118),
            )
            .unwrap(),
            "https://www.nexusmods.com/site/mods/631?tab=files&file_id=5118"
        );
    }

    #[test]
    fn direct_action_parser_reads_form_and_link_text() {
        let archive = MissingArchive {
            hash: xxh64(b"archive", 0),
            name: "archive.7z".into(),
            size: 7,
            source_kind: MissingArchiveSourceKind::Manual,
            url: Some("https://example.test/file".into()),
            source_hint: "test".into(),
        };
        let base = Url::parse("https://example.test/file").unwrap();
        let form_actions = extract_manual_actions(
            r#"<form method="post" action="/create"><button>Create download link</button></form>"#,
            &base,
            &archive,
        )
        .unwrap();
        assert_eq!(form_actions.len(), 1);
        assert!(matches!(
            form_actions[0].method,
            ManualActionMethod::Post(_)
        ));

        let link_actions =
            extract_manual_actions(r#"<a href="/start">Start Download</a>"#, &base, &archive)
                .unwrap();
        assert_eq!(link_actions.len(), 1);
        assert_eq!(link_actions[0].url.as_str(), "https://example.test/start");
    }

    #[test]
    fn browser_event_adapter_returns_download_path() {
        let event = BrowserDownloadEvent {
            path: PathBuf::from("/tmp/archive.7z"),
        };
        assert_eq!(
            browser_event_download_path(&event),
            Path::new("/tmp/archive.7z")
        );
    }

    #[tokio::test]
    async fn watcher_accepts_stable_matching_download() {
        let bytes = b"correct archive";
        let manifest = manifest_for(bytes);
        let archive = missing_archives(&manifest, Path::new("/missing"), false)
            .pop()
            .unwrap();
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("manual.7z");
        tokio::fs::write(&path, bytes).await.unwrap();

        let found = wait_for_matching_download(temp.path(), &archive, Duration::from_secs(5))
            .await
            .unwrap();
        assert!(found.matched);
        assert_eq!(found.path, path);
    }

    #[tokio::test]
    async fn watcher_reports_same_name_mismatch_after_timeout() {
        let manifest = manifest_for(b"correct archive");
        let archive = missing_archives(&manifest, Path::new("/missing"), false)
            .pop()
            .unwrap();
        let temp = TempDir::new().unwrap();
        tokio::fs::write(temp.path().join("manual.7z"), b"wrong archive")
            .await
            .unwrap();

        let found = wait_for_matching_download(temp.path(), &archive, Duration::from_secs(3))
            .await
            .unwrap();
        assert!(!found.matched);
        assert!(found.name_matched);
    }

    #[tokio::test]
    async fn watcher_accepts_file_renamed_from_partial_to_final() {
        let bytes = b"correct archive";
        let manifest = manifest_for(bytes);
        let archive = missing_archives(&manifest, Path::new("/missing"), false)
            .pop()
            .unwrap();
        let temp = TempDir::new().unwrap();
        let partial = temp.path().join("manual.7z.crdownload");
        let final_path = temp.path().join("manual.7z");
        tokio::fs::write(&partial, bytes).await.unwrap();

        let partial_for_task = partial.clone();
        let final_for_task = final_path.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(750)).await;
            tokio::fs::rename(partial_for_task, final_for_task)
                .await
                .unwrap();
        });

        let found = wait_for_matching_download(temp.path(), &archive, Duration::from_secs(6))
            .await
            .unwrap();
        assert!(found.matched);
        assert_eq!(found.path, final_path);
    }

    #[tokio::test]
    async fn watcher_ignores_wrong_name_wrong_hash_and_continues_waiting() {
        let bytes = b"correct archive";
        let manifest = manifest_for(bytes);
        let archive = missing_archives(&manifest, Path::new("/missing"), false)
            .pop()
            .unwrap();
        let temp = TempDir::new().unwrap();
        tokio::fs::write(temp.path().join("unrelated.7z"), b"wrong archive")
            .await
            .unwrap();

        let correct = temp.path().join("renamed-but-correct.7z");
        let correct_for_task = correct.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(750)).await;
            tokio::fs::write(correct_for_task, bytes).await.unwrap();
        });

        let found = wait_for_matching_download(temp.path(), &archive, Duration::from_secs(6))
            .await
            .unwrap();
        assert!(found.matched);
        assert!(!found.name_matched);
        assert_eq!(found.path, correct);
    }

    #[tokio::test]
    async fn watcher_matches_multiple_pending_archives_in_any_order() {
        let manifest = manifest_with_manual_and_nexus(b"manual archive", b"nexus archive");
        let pending = missing_archives(&manifest, Path::new("/missing"), true);
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("second-first.7z");
        tokio::fs::write(&path, b"nexus archive").await.unwrap();

        let found = wait_for_next_matching_download(temp.path(), &pending, Duration::from_secs(5))
            .await
            .unwrap();
        assert!(found.matched);
        assert_eq!(found.path, path);
        assert_eq!(found.matched_hash, Some(xxh64(b"nexus archive", 0)));
    }

    #[tokio::test]
    async fn import_acquired_archive_imports_exact_hash() {
        let bytes = b"correct archive";
        let manifest = manifest_for(bytes);
        let archive = missing_archives(&manifest, Path::new("/missing"), false)
            .pop()
            .unwrap();
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("manual.7z");
        let store = temp.path().join("store");
        let mut file = tokio::fs::File::create(&source).await.unwrap();
        file.write_all(bytes).await.unwrap();
        file.flush().await.unwrap();

        let result = import_acquired_archive(&manifest, &store, &archive, &source)
            .await
            .unwrap();
        assert_eq!(result.status, AcquireStatus::Imported);
        assert!(archive_path(&store, &archive.hash).exists());
    }

    #[tokio::test]
    async fn import_acquired_archive_refuses_same_name_wrong_hash() {
        let manifest = manifest_for(b"correct archive");
        let archive = missing_archives(&manifest, Path::new("/missing"), false)
            .pop()
            .unwrap();
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("manual.7z");
        let store = temp.path().join("store");
        tokio::fs::write(&source, b"wrong archive").await.unwrap();

        let result = import_acquired_archive(&manifest, &store, &archive, &source)
            .await
            .unwrap();
        assert_eq!(result.status, AcquireStatus::Mismatched);
        assert!(!archive_path(&store, &archive.hash).exists());
    }

    #[tokio::test]
    async fn direct_resolver_follows_workupload_style_download_link() {
        let bytes = b"direct archive";
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/file/abc"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"<html><a href="/download/abc">Download</a></html>"#),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/download/abc"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/octet-stream")
                    .set_body_bytes(bytes),
            )
            .mount(&server)
            .await;

        let temp = TempDir::new().unwrap();
        let archive = MissingArchive {
            hash: xxh64(bytes, 0),
            name: "manual.7z".into(),
            size: bytes.len() as u64,
            source_kind: MissingArchiveSourceKind::Manual,
            url: Some(format!("{}/file/abc", server.uri())),
            source_hint: "test".into(),
        };
        let client = Client::builder().cookie_store(true).build().unwrap();
        let final_path = temp.path().join("manual.7z");

        let resolved = resolve_manual_http_flow(
            &client,
            Url::parse(archive.url.as_ref().unwrap()).unwrap(),
            &archive,
            &final_path,
        )
        .await
        .unwrap();

        assert_eq!(resolved, Some((final_path.clone(), archive.hash)));
        assert_eq!(tokio::fs::read(final_path).await.unwrap(), bytes);
    }

    #[tokio::test]
    async fn direct_resolver_handles_sharemods_style_two_step_form() {
        let bytes = b"sharemods archive";
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/file.html"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"
                <form method="post" action="/create">
                  <input name="op" value="download2">
                  <button>Create download link</button>
                </form>
                "#,
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/create"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/html")
                    .set_body_string(r#"<html><a href="/start">Start Download</a></html>"#),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/start"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/octet-stream")
                    .set_body_bytes(bytes),
            )
            .mount(&server)
            .await;

        let temp = TempDir::new().unwrap();
        let archive = MissingArchive {
            hash: xxh64(bytes, 0),
            name: "EyesMod3_FullCompressed1K.7z".into(),
            size: bytes.len() as u64,
            source_kind: MissingArchiveSourceKind::Manual,
            url: Some(format!("{}/file.html", server.uri())),
            source_hint: "test".into(),
        };
        let client = Client::builder().cookie_store(true).build().unwrap();
        let final_path = temp.path().join("EyesMod3_FullCompressed1K.7z");

        let resolved = resolve_manual_http_flow(
            &client,
            Url::parse(archive.url.as_ref().unwrap()).unwrap(),
            &archive,
            &final_path,
        )
        .await
        .unwrap();

        assert_eq!(resolved, Some((final_path.clone(), archive.hash)));
        assert_eq!(tokio::fs::read(final_path).await.unwrap(), bytes);
    }

    #[tokio::test]
    async fn direct_resolver_rejects_wrong_hash_download() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/file"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"<html><a href="/download">Download</a></html>"#),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/download"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/octet-stream")
                    .set_body_bytes(b"wrong archive"),
            )
            .mount(&server)
            .await;

        let temp = TempDir::new().unwrap();
        let archive = MissingArchive {
            hash: xxh64(b"correct archive", 0),
            name: "manual.7z".into(),
            size: 15,
            source_kind: MissingArchiveSourceKind::Manual,
            url: Some(format!("{}/file", server.uri())),
            source_hint: "test".into(),
        };
        let client = Client::builder().cookie_store(true).build().unwrap();
        let final_path = temp.path().join("manual.7z");

        let resolved = resolve_manual_http_flow(
            &client,
            Url::parse(archive.url.as_ref().unwrap()).unwrap(),
            &archive,
            &final_path,
        )
        .await
        .unwrap();

        assert!(resolved.is_none());
        assert!(!final_path.exists());
    }

    #[tokio::test]
    async fn loverslab_dns_failure_is_actionable() {
        let archive = MissingArchive {
            hash: xxh64(b"archive", 0),
            name: "00 - Normal Version.7z".into(),
            size: 7,
            source_kind: MissingArchiveSourceKind::Manual,
            url: Some("https://www.loverslab.com/files/file/5051".into()),
            source_hint: "test".into(),
        };

        let result = resolve_loverslab_manual("definitely-not-a-host.invalid", &archive)
            .await
            .unwrap();

        let DirectAcquireOutcome::Final(result) = result else {
            panic!("expected DNS-unresolved final result");
        };
        assert_eq!(result.status, AcquireStatus::DnsUnresolved);
        assert!(
            result
                .message
                .as_deref()
                .unwrap()
                .contains("did not resolve")
        );
    }
}
