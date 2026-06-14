//! Wabbajack modlist catalog: fetching and parsing the official, repository,
//! and authored-file listings, filtering and deduplicating entries, and
//! emitting Home Manager configuration snippets for a chosen modlist.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt as _;

mod authored;
mod hm;
pub(crate) use authored::{authored_file_target, check_authored_file_available};
pub use authored::{download_authored_file_to_path, parse_authored_files};
pub use hm::{
    format_hm_path_snippet, format_hm_snippet, hm_snippet_for_source, is_remote_url,
    nix_sha256_sri, resolve_download_target, summarize_by_machine,
};

use authored::{
    authored_files_download_target, download_authored_wabbajack_file, sanitize_file_name,
};

#[cfg(test)]
use authored::*;

pub const OFFICIAL_MODLISTS_URL: &str =
    "https://raw.githubusercontent.com/wabbajack-tools/mod-lists/master/modlists.json";
pub const REPOSITORIES_URL: &str =
    "https://raw.githubusercontent.com/wabbajack-tools/mod-lists/master/repositories.json";
pub const AUTHORED_FILES_URL: &str = "https://build.wabbajack.org/authored_files";
const AUTHORED_FILES_CDN_PREFIX: &str = "https://authored-files.wabbajack.org/";
const AUTHORED_FILES_DOWNLOAD_MARKER: &str = "/authored_files/download/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthoredFileTarget {
    pub(crate) base_url: String,
    pub(crate) munged_name: String,
    pub(crate) metadata_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthoredFileAvailability {
    pub(crate) target: AuthoredFileTarget,
    pub(crate) status: reqwest::StatusCode,
}

/// Which catalog listings to draw modlist entries from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogSource {
    Official,
    Authored,
    Both,
}

impl CatalogSource {
    /// Whether this source includes official/repository modlists.
    #[must_use]
    pub fn includes_official(self) -> bool {
        matches!(self, Self::Official | Self::Both)
    }

    /// Whether this source includes authored-file modlists.
    #[must_use]
    pub fn includes_authored(self) -> bool {
        matches!(self, Self::Authored | Self::Both)
    }
}

/// Which listing a particular catalog entry originated from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogEntrySource {
    Official,
    Authored,
}

/// Reported size figures for a modlist (download, installed, totals).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WabbajackSizeMetadata {
    pub modlist_size: Option<u64>,
    pub archive_count: Option<u64>,
    pub archive_size: Option<u64>,
    pub installed_file_count: Option<u64>,
    pub installed_size: Option<u64>,
    pub total_size: Option<u64>,
}

/// A single modlist entry in the catalog, with its metadata and download URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WabbajackCatalogEntry {
    pub title: String,
    pub game: Option<String>,
    pub author: Option<String>,
    pub version: Option<String>,
    pub tags: Vec<String>,
    pub image_url: Option<String>,
    pub readme_url: Option<String>,
    pub download_url: String,
    pub repository_name: Option<String>,
    pub machine_url: Option<String>,
    pub discord_url: Option<String>,
    pub website_url: Option<String>,
    pub official: bool,
    pub nsfw: bool,
    pub force_down: bool,
    pub size: WabbajackSizeMetadata,
    pub source: CatalogEntrySource,
}

/// Criteria for filtering catalog entries (query, game, and content toggles).
#[derive(Debug, Clone, Default)]
pub struct CatalogFilter {
    pub query: Option<String>,
    pub game: Option<String>,
    pub official_only: bool,
    pub include_nsfw: bool,
    pub include_down: bool,
}

#[derive(Debug, Deserialize)]
struct OfficialEntry {
    title: String,
    description: Option<String>,
    author: Option<String>,
    game: Option<String>,
    official: Option<bool>,
    tags: Option<Vec<String>>,
    nsfw: Option<bool>,
    #[serde(default)]
    force_down: bool,
    links: OfficialLinks,
    download_metadata: Option<OfficialDownloadMetadata>,
    version: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OfficialLinks {
    image: Option<String>,
    readme: Option<String>,
    download: Option<String>,
    #[serde(rename = "machineURL")]
    machine_url: Option<String>,
    #[serde(rename = "discordURL")]
    discord_url: Option<String>,
    #[serde(rename = "websiteURL")]
    website_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OfficialDownloadMetadata {
    size: Option<u64>,
    number_of_archives: Option<u64>,
    size_of_archives: Option<u64>,
    number_of_installed_files: Option<u64>,
    size_of_installed_files: Option<u64>,
    total_size: Option<u64>,
}

impl From<OfficialDownloadMetadata> for WabbajackSizeMetadata {
    fn from(value: OfficialDownloadMetadata) -> Self {
        Self {
            modlist_size: value.size,
            archive_count: value.number_of_archives,
            archive_size: value.size_of_archives,
            installed_file_count: value.number_of_installed_files,
            installed_size: value.size_of_installed_files,
            total_size: value.total_size,
        }
    }
}

pub async fn fetch_catalog(
    client: &reqwest::Client,
    source: CatalogSource,
) -> Result<Vec<WabbajackCatalogEntry>> {
    let mut entries = Vec::new();
    if source.includes_official() {
        let body = client
            .get(REPOSITORIES_URL)
            .send()
            .await
            .context("failed to fetch Wabbajack repository index")?
            .error_for_status()
            .context("Wabbajack repository index returned an error")?
            .text()
            .await
            .context("failed to read Wabbajack repository index")?;
        entries.extend(fetch_repository_catalogs(client, &body).await?);
    }
    if source.includes_authored() {
        let body = client
            .get(AUTHORED_FILES_URL)
            .send()
            .await
            .context("failed to fetch Wabbajack authored-files report")?
            .error_for_status()
            .context("authored-files report returned an error")?
            .text()
            .await
            .context("failed to read Wabbajack authored-files report")?;
        entries.extend(parse_authored_files(&body));
    }
    Ok(deduplicate_entries(entries))
}

pub fn parse_official_catalog(input: &str) -> Result<Vec<WabbajackCatalogEntry>> {
    parse_repository_catalog(None, input)
}

pub fn parse_repositories(input: &str) -> Result<HashMap<String, String>> {
    serde_json::from_str(input).context("failed to parse Wabbajack repositories JSON")
}

pub fn parse_repository_catalog(
    repository_name: Option<&str>,
    input: &str,
) -> Result<Vec<WabbajackCatalogEntry>> {
    let raw: Vec<OfficialEntry> =
        serde_json::from_str(input).context("failed to parse Wabbajack modlist catalog JSON")?;
    Ok(raw
        .into_iter()
        .filter_map(|entry| {
            let download_url = entry.links.download?;
            let mut tags = entry.tags.unwrap_or_default();
            if let Some(description) = entry.description
                && !description.is_empty()
                && !tags.iter().any(|tag| tag == "has-description")
            {
                tags.push("has-description".to_string());
            }
            Some(WabbajackCatalogEntry {
                title: entry.title,
                game: entry.game,
                author: entry.author,
                version: entry.version,
                tags,
                image_url: entry.links.image,
                readme_url: entry.links.readme,
                download_url,
                repository_name: repository_name.map(ToString::to_string),
                machine_url: entry.links.machine_url,
                discord_url: entry.links.discord_url,
                website_url: entry.links.website_url,
                official: entry.official.unwrap_or(false),
                nsfw: entry.nsfw.unwrap_or(false),
                force_down: entry.force_down,
                size: entry
                    .download_metadata
                    .map_or_else(Default::default, Into::into),
                source: CatalogEntrySource::Official,
            })
        })
        .collect())
}

async fn fetch_repository_catalogs(
    client: &reqwest::Client,
    repositories_json: &str,
) -> Result<Vec<WabbajackCatalogEntry>> {
    let repositories = parse_repositories(repositories_json)?;
    let mut entries = Vec::new();
    for (repository_name, url) in repositories {
        let body = match client.get(&url).send().await {
            Ok(response) => match response.error_for_status() {
                Ok(response) => response.text().await.with_context(|| {
                    format!("failed to read Wabbajack repository catalog '{repository_name}'")
                })?,
                Err(err) => {
                    tracing::warn!(%repository_name, %url, "Wabbajack repository catalog returned an error: {err}");
                    continue;
                }
            },
            Err(err) => {
                tracing::warn!(%repository_name, %url, "failed to fetch Wabbajack repository catalog: {err}");
                continue;
            }
        };
        match parse_repository_catalog(Some(&repository_name), &body) {
            Ok(mut parsed) => entries.append(&mut parsed),
            Err(err) => {
                tracing::warn!(%repository_name, %url, "failed to parse Wabbajack repository catalog: {err}");
            }
        }
    }
    Ok(entries)
}

pub fn deduplicate_entries(entries: Vec<WabbajackCatalogEntry>) -> Vec<WabbajackCatalogEntry> {
    let mut seen_urls = HashSet::new();
    let mut seen_keys = HashSet::new();
    let mut out = Vec::new();
    for entry in entries {
        let url_key = entry.download_url.to_ascii_lowercase();
        let key = entry.machine_url.clone().map_or_else(
            || {
                format!(
                    "{}:{}",
                    entry.title.to_ascii_lowercase(),
                    entry.version.clone().unwrap_or_default()
                )
            },
            |machine| {
                format!(
                    "{}:{machine}",
                    entry.repository_name.clone().unwrap_or_default()
                )
            },
        );
        if seen_urls.insert(url_key) && seen_keys.insert(key.to_ascii_lowercase()) {
            out.push(entry);
        }
    }
    out
}

pub fn filter_entries(
    entries: &[WabbajackCatalogEntry],
    filter: &CatalogFilter,
) -> Vec<WabbajackCatalogEntry> {
    let query = filter.query.as_ref().map(|q| q.to_ascii_lowercase());
    let game = filter.game.as_ref().map(|g| normalized_game_key(g));
    entries
        .iter()
        .filter(|entry| {
            if filter.official_only && !entry.official {
                return false;
            }
            if !filter.include_nsfw && entry.nsfw {
                return false;
            }
            if !filter.include_down && entry.force_down {
                return false;
            }
            if let Some(game) = &game
                && entry.game.as_ref().map(|g| normalized_game_key(g)) != Some(game.clone())
            {
                return false;
            }
            if let Some(query) = &query {
                let haystack = format!(
                    "{} {} {} {} {} {} {}",
                    entry.title,
                    entry.author.clone().unwrap_or_default(),
                    entry.game.clone().unwrap_or_default(),
                    entry.version.clone().unwrap_or_default(),
                    entry.repository_name.clone().unwrap_or_default(),
                    entry.machine_url.clone().unwrap_or_default(),
                    entry.tags.join(" ")
                )
                .to_ascii_lowercase();
                haystack.contains(query)
            } else {
                true
            }
        })
        .cloned()
        .collect()
}

fn normalized_game_key(game: &str) -> String {
    modde_games::normalize_wabbajack_game(game)
        .unwrap_or(game)
        .to_ascii_lowercase()
}

pub fn find_entry<'a>(
    entries: &'a [WabbajackCatalogEntry],
    url_or_machine: &str,
) -> Option<&'a WabbajackCatalogEntry> {
    entries.iter().find(|entry| {
        entry.download_url == url_or_machine
            || entry.machine_url.as_deref() == Some(url_or_machine)
            || entry
                .repository_name
                .as_ref()
                .zip(entry.machine_url.as_ref())
                .is_some_and(|(repo, machine)| {
                    format!("{repo}/{machine}").eq_ignore_ascii_case(url_or_machine)
                })
            || entry.title.eq_ignore_ascii_case(url_or_machine)
    })
}

pub async fn download_wabbajack_file(
    client: &reqwest::Client,
    url: &str,
    output_dir: &Path,
) -> Result<PathBuf> {
    if let Some((base_url, munged_name)) = authored_files_download_target(url) {
        return download_authored_wabbajack_file(client, &munged_name, &base_url, output_dir)
            .await
            .with_context(|| format!("failed to download Wabbajack authored-files archive {url}"));
    }

    let file_name = url
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .map(sanitize_file_name)
        .unwrap_or_else(|| "modlist.wabbajack".to_string());
    let dest = output_dir.join(file_name);
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to download {url}"))?
        .error_for_status()
        .with_context(|| format!("download returned an error for {url}"))?;
    let mut file = tokio::fs::File::create(&dest)
        .await
        .with_context(|| format!("failed to create {}", dest.display()))?;
    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    Ok(dest)
}

#[cfg(test)]
mod tests;
