use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt as _;

pub const OFFICIAL_MODLISTS_URL: &str =
    "https://raw.githubusercontent.com/wabbajack-tools/mod-lists/master/modlists.json";
pub const REPOSITORIES_URL: &str =
    "https://raw.githubusercontent.com/wabbajack-tools/mod-lists/master/repositories.json";
pub const AUTHORED_FILES_URL: &str = "https://build.wabbajack.org/authored_files";
const AUTHORED_FILES_CDN_PREFIX: &str = "https://authored-files.wabbajack.org/";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogSource {
    Official,
    Authored,
    Both,
}

impl CatalogSource {
    #[must_use]
    pub fn includes_official(self) -> bool {
        matches!(self, Self::Official | Self::Both)
    }

    #[must_use]
    pub fn includes_authored(self) -> bool {
        matches!(self, Self::Authored | Self::Both)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogEntrySource {
    Official,
    Authored,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WabbajackSizeMetadata {
    pub modlist_size: Option<u64>,
    pub archive_count: Option<u64>,
    pub archive_size: Option<u64>,
    pub installed_file_count: Option<u64>,
    pub installed_size: Option<u64>,
    pub total_size: Option<u64>,
}

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

pub fn parse_authored_files(input: &str) -> Vec<WabbajackCatalogEntry> {
    input
        .lines()
        .filter(|line| line.contains(".wabbajack"))
        .filter_map(parse_authored_line)
        .collect()
}

fn parse_authored_line(line: &str) -> Option<WabbajackCatalogEntry> {
    let clean = strip_html(line);
    let lower = clean.to_ascii_lowercase();
    let ext = lower.find(".wabbajack")?;
    let title_start = clean[..ext].rfind(['>', '\n']).map_or(0, |idx| idx + 1);
    let title = clean[title_start..ext + ".wabbajack".len()]
        .trim()
        .trim_matches('|')
        .trim()
        .to_string();
    if title.is_empty() {
        return None;
    }

    let download_url = extract_url(line).unwrap_or_else(|| {
        let encoded = title.replace(' ', "%20");
        format!("https://authored-files.wabbajack.org/{encoded}")
    });

    let author = clean
        .split('|')
        .map(str::trim)
        .find(|part| part.starts_with("github/"))
        .map(ToString::to_string);

    Some(WabbajackCatalogEntry {
        title,
        game: None,
        author,
        version: None,
        tags: Vec::new(),
        image_url: None,
        readme_url: None,
        download_url,
        machine_url: None,
        discord_url: None,
        website_url: None,
        repository_name: None,
        official: false,
        nsfw: false,
        force_down: false,
        size: WabbajackSizeMetadata::default(),
        source: CatalogEntrySource::Authored,
    })
}

fn strip_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn extract_url(input: &str) -> Option<String> {
    for marker in ["href=\"", "href='"] {
        if let Some(start) = input.find(marker) {
            let rest = &input[start + marker.len()..];
            let quote = if marker.ends_with('"') { '"' } else { '\'' };
            if let Some(end) = rest.find(quote) {
                let url = rest[..end].replace("&amp;", "&");
                if url.contains(".wabbajack") {
                    return Some(url);
                }
            }
        }
    }
    input
        .split_whitespace()
        .find(|part| part.starts_with("http") && part.contains(".wabbajack"))
        .map(|part| part.trim_matches(['"', '\'', '<', '>']).to_string())
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
    if let Some(munged_name) = authored_files_munged_name(url) {
        return download_authored_wabbajack_file(
            client,
            &munged_name,
            AUTHORED_FILES_URL,
            output_dir,
        )
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

fn authored_files_munged_name(url: &str) -> Option<String> {
    url.strip_prefix(AUTHORED_FILES_CDN_PREFIX)
        .filter(|name| !name.is_empty())
        .map(percent_decode_lossy)
}

fn authored_files_download_page_url(base_url: &str, munged_name: &str) -> String {
    format!(
        "{}/download/{}",
        base_url.trim_end_matches('/'),
        encode_path_segment(munged_name)
    )
}

fn authored_files_part_url(base_url: &str, munged_name: &str, index: u64) -> String {
    format!(
        "{}/{}/parts/{index}",
        base_url.trim_end_matches('/'),
        encode_path_segment(munged_name)
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AuthoredFilePart {
    size: u64,
    offset: u64,
    index: u64,
}

#[derive(Debug)]
struct AuthoredFilesDownloadPage {
    munged_name: String,
    file_name: String,
    file_size_bytes: u64,
    parts: Vec<AuthoredFilePart>,
}

async fn download_authored_wabbajack_file(
    client: &reqwest::Client,
    munged_name: &str,
    base_url: &str,
    output_dir: &Path,
) -> Result<PathBuf> {
    let page_url = authored_files_download_page_url(base_url, munged_name);
    let page_response = client.get(&page_url).send().await.with_context(|| {
        format!("failed to fetch Wabbajack authored-files metadata page {page_url}")
    })?;
    let page_status = page_response.status();
    let page_text = page_response
        .error_for_status()
        .with_context(|| {
            format!("Wabbajack authored-files metadata page returned {page_status} for {page_url}")
        })?
        .text()
        .await
        .with_context(|| {
            format!("failed to read Wabbajack authored-files metadata page {page_url}")
        })?;

    let page = parse_authored_files_download_page(&page_text).with_context(|| {
        format!("failed to parse Wabbajack authored-files metadata page {page_url}")
    })?;

    let munged_name = if page.munged_name.is_empty() {
        munged_name
    } else {
        &page.munged_name
    };
    let file_name = sanitize_file_name(&page.file_name);
    let dest = output_dir.join(file_name);
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let part_dest = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&part_dest)
        .await
        .with_context(|| format!("failed to create {}", part_dest.display()))?;
    let mut written = 0_u64;

    let mut parts = page.parts;
    parts.sort_by_key(|part| part.offset);
    for part in parts {
        if part.offset != written {
            anyhow::bail!(
                "Wabbajack authored-files part {} has offset {}, expected {}",
                part.index,
                part.offset,
                written
            );
        }
        let part_url = authored_files_part_url(base_url, munged_name, part.index);
        let mut response = client.get(&part_url).send().await.with_context(|| {
            format!(
                "failed to download Wabbajack authored-files part {} from {part_url}",
                part.index
            )
        })?;
        let status = response.status();
        response = response.error_for_status().with_context(|| {
            format!(
                "Wabbajack authored-files part {} returned {status} for {part_url}",
                part.index
            )
        })?;

        let mut part_written = 0_u64;
        while let Some(chunk) = response.chunk().await.with_context(|| {
            format!(
                "failed to read Wabbajack authored-files part {} from {part_url}",
                part.index
            )
        })? {
            part_written += chunk.len() as u64;
            file.write_all(&chunk).await?;
        }
        if part_written != part.size {
            anyhow::bail!(
                "Wabbajack authored-files part {} size mismatch: expected {} bytes, downloaded {} bytes from {}",
                part.index,
                part.size,
                part_written,
                part_url
            );
        }
        written += part_written;
    }
    file.flush().await?;
    drop(file);

    if written != page.file_size_bytes {
        anyhow::bail!(
            "Wabbajack authored-files final size mismatch: expected {} bytes, downloaded {} bytes",
            page.file_size_bytes,
            written
        );
    }

    tokio::fs::rename(&part_dest, &dest)
        .await
        .with_context(|| {
            format!(
                "failed to move completed Wabbajack download from {} to {}",
                part_dest.display(),
                dest.display()
            )
        })?;
    Ok(dest)
}

fn parse_authored_files_download_page(input: &str) -> Result<AuthoredFilesDownloadPage> {
    let munged_name =
        extract_js_string_const(input, "MUNGED_NAME").context("missing MUNGED_NAME")?;
    let file_name = extract_js_string_const(input, "FILE_NAME").context("missing FILE_NAME")?;
    let file_size_bytes =
        extract_js_u64_const(input, "FILE_SIZE_BYTES").context("missing FILE_SIZE_BYTES")?;
    let parts_json = extract_js_const_value(input, "PARTS").context("missing PARTS")?;
    let parts: Vec<AuthoredFilePart> =
        serde_json::from_str(parts_json).context("failed to parse PARTS JSON")?;
    Ok(AuthoredFilesDownloadPage {
        munged_name,
        file_name,
        file_size_bytes,
        parts,
    })
}

fn extract_js_const_value<'a>(input: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("const {name} = ");
    let start = input.find(&marker)? + marker.len();
    let rest = &input[start..];
    let end = rest.find(';')?;
    Some(rest[..end].trim())
}

fn extract_js_u64_const(input: &str, name: &str) -> Option<u64> {
    extract_js_const_value(input, name)?.parse().ok()
}

fn extract_js_string_const(input: &str, name: &str) -> Option<String> {
    let value = extract_js_const_value(input, name)?;
    parse_js_string_literal(value)
}

fn parse_js_string_literal(value: &str) -> Option<String> {
    let mut chars = value.chars();
    let quote = chars.next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for ch in chars {
        if escaped {
            match ch {
                '"' => out.push('"'),
                '\'' => out.push('\''),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => out.push(other),
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

fn percent_decode_lossy(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'%'
            && idx + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex_value(bytes[idx + 1]), hex_value(bytes[idx + 2]))
        {
            out.push(high * 16 + low);
            idx += 3;
            continue;
        }
        out.push(bytes[idx]);
        idx += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn encode_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn sanitize_file_name(name: &str) -> String {
    let mut name = name.replace("%20", " ");
    if let Some((prefix, _uuid)) = name.rsplit_once(".wabbajack_") {
        name = format!("{prefix}.wabbajack");
    }
    name.chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => ch,
        })
        .collect()
}

pub async fn resolve_download_target(
    client: &reqwest::Client,
    url_or_machine: &str,
    source: CatalogSource,
) -> Result<String> {
    if is_remote_url(url_or_machine) {
        return Ok(url_or_machine.to_string());
    }
    let entries = fetch_catalog(client, source).await?;
    find_entry(&entries, url_or_machine)
        .map(|entry| entry.download_url.clone())
        .ok_or_else(|| anyhow::anyhow!("no Wabbajack catalog entry matches '{url_or_machine}'"))
}

#[must_use]
pub fn is_remote_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

pub async fn hm_snippet_for_source(
    client: &reqwest::Client,
    source: &str,
    profile: &str,
    game: &str,
    game_dir: Option<&Path>,
    cache_dir: &Path,
) -> Result<(String, Option<PathBuf>)> {
    let (url, path) = if is_remote_url(source) {
        let path = download_wabbajack_file(client, source, cache_dir).await?;
        (source.to_string(), Some(path))
    } else {
        let path = PathBuf::from(source);
        if path.exists() {
            (path.to_string_lossy().to_string(), Some(path))
        } else {
            let url = resolve_download_target(client, source, CatalogSource::Both).await?;
            let path = download_wabbajack_file(client, &url, cache_dir).await?;
            (url, Some(path))
        }
    };

    let path = path.as_ref().context("no file available to hash")?;
    let hash = nix_sha256_sri(path).await?;
    Ok((
        format_hm_snippet(profile, game, game_dir, &url, &hash),
        Some(path.clone()),
    ))
}

pub async fn nix_sha256_sri(path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        use tokio::io::AsyncReadExt as _;
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!(
        "sha256-{}",
        base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
    ))
}

#[must_use]
pub fn format_hm_snippet(
    profile: &str,
    game: &str,
    game_dir: Option<&Path>,
    url: &str,
    hash: &str,
) -> String {
    let mut out = format!(
        "programs.modde.profiles.{profile} = {{\n  game = \"{game}\";\n  installMode = \"auto\";\n"
    );
    if let Some(game_dir) = game_dir {
        out.push_str(&format!(
            "  gameDir = \"{}\";\n",
            escape_nix_string(&game_dir.display().to_string())
        ));
    }
    out.push_str(&format!(
        "  wabbajackList = {{\n    url = \"{}\";\n    hash = \"{}\";\n  }};\n}};\n",
        escape_nix_string(url),
        escape_nix_string(hash)
    ));
    out
}

fn escape_nix_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub fn summarize_by_machine(entries: &[WabbajackCatalogEntry]) -> HashMap<String, String> {
    entries
        .iter()
        .filter_map(|entry| {
            entry
                .machine_url
                .as_ref()
                .map(|machine| (machine.clone(), entry.title.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::thread;

    #[test]
    fn official_catalog_parses_and_filters() {
        let json = r#"[{
          "title":"Legends of the Frost",
          "author":"Phoenix",
          "game":"skyrimspecialedition",
          "official":false,
          "tags":["Official","Lightweight"],
          "nsfw":false,
          "force_down":false,
          "links":{"download":"https://example/lotf.wabbajack","machineURL":"lotf"},
          "download_metadata":{"Size":10,"NumberOfArchives":2,"TotalSize":30},
          "version":"1.0"
        }]"#;
        let entries = parse_official_catalog(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].machine_url.as_deref(), Some("lotf"));
        let filtered = filter_entries(
            &entries,
            &CatalogFilter {
                query: Some("frost".into()),
                include_nsfw: false,
                include_down: false,
                ..Default::default()
            },
        );
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn filter_entries_matches_normalized_wabbajack_game_names() {
        let entries = vec![WabbajackCatalogEntry {
            title: "Legends of the Frost".to_string(),
            game: Some("skyrimspecialedition".to_string()),
            author: None,
            version: None,
            tags: Vec::new(),
            image_url: None,
            readme_url: None,
            download_url: "https://example/lotf.wabbajack".to_string(),
            repository_name: None,
            machine_url: None,
            discord_url: None,
            website_url: None,
            official: true,
            nsfw: false,
            force_down: false,
            size: WabbajackSizeMetadata::default(),
            source: CatalogEntrySource::Official,
        }];

        let filtered = filter_entries(
            &entries,
            &CatalogFilter {
                game: Some("skyrim-se".to_string()),
                include_nsfw: false,
                include_down: false,
                ..Default::default()
            },
        );

        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn repositories_json_parses_named_catalog_sources() {
        let json = r#"{
          "wj-featured": "https://raw.githubusercontent.com/wabbajack-tools/mod-lists/master/modlists.json",
          "WakingDreams": "https://raw.githubusercontent.com/Oghma-Infinium/modlists/main/modlists.json"
        }"#;
        let repositories = parse_repositories(json).unwrap();
        assert_eq!(
            repositories.get("WakingDreams").map(String::as_str),
            Some("https://raw.githubusercontent.com/Oghma-Infinium/modlists/main/modlists.json")
        );
    }

    #[test]
    fn repository_catalog_parses_twisted_skyrim_metadata() {
        let entries =
            parse_repository_catalog(Some("WakingDreams"), twisted_skyrim_json()).unwrap();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.title, "Twisted Skyrim");
        assert_eq!(entry.repository_name.as_deref(), Some("WakingDreams"));
        assert_eq!(entry.machine_url.as_deref(), Some("TwistedSkyrim"));
        assert_eq!(entry.game.as_deref(), Some("skyrimspecialedition"));
        assert_eq!(entry.author.as_deref(), Some("TwistedModding"));
        assert_eq!(entry.version.as_deref(), Some("1.7.0.0"));
        assert!(entry.nsfw);
        assert!(!entry.official);
        assert_eq!(
            entry.image_url.as_deref(),
            Some(
                "https://raw.githubusercontent.com/Oghma-Infinium/Twisted-Skyrim/refs/heads/main/Twisted%20Skyrim%20Logo%20(1).webp"
            )
        );
        assert_eq!(
            entry.readme_url.as_deref(),
            Some("https://twistedskyrim.com/#installation")
        );
        assert_eq!(
            entry.download_url,
            "https://authored-files.wabbajack.org/Twisted Skyrim.wabbajack_2f42b496-5d7b-4588-bde0-b90a445a04cb"
        );
        assert_eq!(entry.size.modlist_size, Some(123));
        assert_eq!(entry.size.archive_count, Some(456));
        assert_eq!(entry.size.total_size, Some(789));
    }

    #[test]
    fn repository_catalog_search_returns_metadata_entry() {
        let entries =
            parse_repository_catalog(Some("WakingDreams"), twisted_skyrim_json()).unwrap();
        let filtered = filter_entries(
            &entries,
            &CatalogFilter {
                query: Some("twisted".into()),
                include_nsfw: true,
                include_down: false,
                ..Default::default()
            },
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].source, CatalogEntrySource::Official);
        assert_eq!(filtered[0].author.as_deref(), Some("TwistedModding"));
        assert_eq!(filtered[0].machine_url.as_deref(), Some("TwistedSkyrim"));

        let by_repo_machine = find_entry(&entries, "WakingDreams/TwistedSkyrim").unwrap();
        assert_eq!(by_repo_machine.title, "Twisted Skyrim");
    }

    #[test]
    fn authored_files_keep_only_wabbajack_rows() {
        let html = r#"
          <tr><td><a href="https://authored-files.wabbajack.org/List.wabbajack_abc">List.wabbajack abc</a></td><td>github/me</td></tr>
          <tr><td>Not a list.7z</td></tr>
        "#;
        let entries = parse_authored_files(html);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].download_url.contains(".wabbajack"));
    }

    #[test]
    fn authored_files_url_is_detected() {
        assert_eq!(
            authored_files_munged_name(
                "https://authored-files.wabbajack.org/Twisted%20Skyrim.wabbajack_abc"
            )
            .as_deref(),
            Some("Twisted Skyrim.wabbajack_abc")
        );
        assert!(authored_files_munged_name("https://example/lotf.wabbajack").is_none());
    }

    #[test]
    fn authored_files_download_page_url_encodes_munged_name() {
        assert_eq!(
            authored_files_download_page_url(
                "https://build.wabbajack.org/authored_files",
                "Twisted Skyrim.wabbajack_abc"
            ),
            "https://build.wabbajack.org/authored_files/download/Twisted%20Skyrim.wabbajack_abc"
        );
    }

    #[test]
    fn authored_files_download_page_parses_js_constants() {
        let page = parse_authored_files_download_page(
            r#"
            <script>
              const MUNGED_NAME = "Twisted Skyrim.wabbajack_abc";
              const FILE_NAME = "Twisted Skyrim.wabbajack";
              const FILE_SIZE_BYTES = 5;
              const PARTS = [{"Size":2,"Offset":0,"Hash":"a","Index":0},{"Size":3,"Offset":2,"Hash":"b","Index":1}];
            </script>
            "#,
        )
        .unwrap();

        assert_eq!(page.munged_name, "Twisted Skyrim.wabbajack_abc");
        assert_eq!(page.file_name, "Twisted Skyrim.wabbajack");
        assert_eq!(page.file_size_bytes, 5);
        assert_eq!(page.parts.len(), 2);
        assert_eq!(page.parts[1].index, 1);
    }

    #[test]
    fn dedupe_prefers_first_matching_url() {
        let entry = WabbajackCatalogEntry {
            title: "A".into(),
            game: None,
            author: None,
            version: Some("1".into()),
            tags: Vec::new(),
            image_url: None,
            readme_url: None,
            download_url: "https://example/a.wabbajack".into(),
            repository_name: None,
            machine_url: Some("a".into()),
            discord_url: None,
            website_url: None,
            official: true,
            nsfw: false,
            force_down: false,
            size: WabbajackSizeMetadata::default(),
            source: CatalogEntrySource::Official,
        };
        let entries = deduplicate_entries(vec![entry.clone(), entry]);
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn snippet_contains_hm_fields() {
        let snippet = format_hm_snippet(
            "lotf",
            "skyrim-se",
            Some(Path::new("/games/Skyrim")),
            "https://example/lotf.wabbajack",
            "sha256-abc",
        );
        assert!(snippet.contains("programs.modde.profiles.lotf"));
        assert!(snippet.contains("gameDir"));
        assert!(snippet.contains("sha256-abc"));
    }

    #[tokio::test]
    async fn authored_files_downloader_assembles_parts() {
        let (base_url, request_count) = start_authored_files_test_server();
        let temp = tempfile::tempdir().unwrap();
        let client = reqwest::Client::new();

        let path = download_authored_wabbajack_file(
            &client,
            "Test List.wabbajack_abc",
            &format!("{base_url}/authored_files"),
            temp.path(),
        )
        .await
        .unwrap();

        let bytes = tokio::fs::read(&path).await.unwrap();
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("Test List.wabbajack")
        );
        assert_eq!(bytes, b"hello world");
        assert_eq!(request_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn hm_snippet_preserves_original_authored_files_url() {
        let (base_url, _request_count) = start_authored_files_test_server();
        let temp = tempfile::tempdir().unwrap();
        let client = reqwest::Client::new();
        let selected_url = "https://authored-files.wabbajack.org/Test List.wabbajack_abc";

        let path = download_authored_wabbajack_file(
            &client,
            &authored_files_munged_name(selected_url).unwrap(),
            &format!("{base_url}/authored_files"),
            temp.path(),
        )
        .await
        .unwrap();
        let hash = nix_sha256_sri(&path).await.unwrap();
        let snippet = format_hm_snippet("test", "skyrim-se", None, selected_url, &hash);

        assert!(
            snippet.contains(
                "url = \"https://authored-files.wabbajack.org/Test List.wabbajack_abc\";"
            )
        );
        assert!(snippet.contains("hash = \"sha256-"));
    }

    fn start_authored_files_test_server() -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let request_count = Arc::new(AtomicUsize::new(0));
        let thread_count = Arc::clone(&request_count);
        thread::spawn(move || {
            for stream in listener.incoming().take(3) {
                let mut stream = stream.unwrap();
                let mut buf = [0_u8; 2048];
                let n = stream.read(&mut buf).unwrap();
                let request = String::from_utf8_lossy(&buf[..n]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");
                thread_count.fetch_add(1, Ordering::SeqCst);
                match path {
                    "/authored_files/download/Test%20List.wabbajack_abc" => {
                        let body = r#"
                            <script>
                              const MUNGED_NAME = "Test List.wabbajack_abc";
                              const FILE_NAME = "Test List.wabbajack";
                              const FILE_SIZE_BYTES = 11;
                              const PARTS = [{"Size":5,"Offset":0,"Hash":"a","Index":0},{"Size":6,"Offset":5,"Hash":"b","Index":1}];
                            </script>
                        "#;
                        write_response(&mut stream, "200 OK", "text/html", body.as_bytes());
                    }
                    "/authored_files/Test%20List.wabbajack_abc/parts/0" => {
                        write_response(&mut stream, "200 OK", "application/octet-stream", b"hello");
                    }
                    "/authored_files/Test%20List.wabbajack_abc/parts/1" => {
                        write_response(
                            &mut stream,
                            "200 OK",
                            "application/octet-stream",
                            b" world",
                        );
                    }
                    _ => write_response(&mut stream, "404 Not Found", "text/plain", b"not found"),
                }
            }
        });
        (format!("http://{addr}"), request_count)
    }

    fn write_response(
        stream: &mut std::net::TcpStream,
        status: &str,
        content_type: &str,
        body: &[u8],
    ) {
        let headers = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(headers.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    }

    fn twisted_skyrim_json() -> &'static str {
        r#"[{
          "title":"Twisted Skyrim",
          "description":"A personal modlist for Skyrim Special Edition.",
          "author":"TwistedModding",
          "game":"skyrimspecialedition",
          "official":false,
          "tags":["NSFW","Combat"],
          "nsfw":true,
          "force_down":false,
          "links":{
            "image":"https://raw.githubusercontent.com/Oghma-Infinium/Twisted-Skyrim/refs/heads/main/Twisted%20Skyrim%20Logo%20(1).webp",
            "readme":"https://twistedskyrim.com/#installation",
            "download":"https://authored-files.wabbajack.org/Twisted Skyrim.wabbajack_2f42b496-5d7b-4588-bde0-b90a445a04cb",
            "machineURL":"TwistedSkyrim",
            "discordURL":"https://discord.gg/4WwqfK5yHg",
            "websiteURL":"https://twistedskyrim.com/#overview"
          },
          "download_metadata":{"Size":123,"NumberOfArchives":456,"TotalSize":789},
          "version":"1.7.0.0"
        }]"#
    }
}
