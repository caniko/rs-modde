use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _, AsyncWriteExt as _};
use xxhash_rust::xxh3::Xxh3;
use xxhash_rust::xxh64::Xxh64;

use super::{CatalogEntrySource, WabbajackCatalogEntry, WabbajackSizeMetadata};
use crate::ProgressCallback;

mod availability;
mod page;

pub(crate) use availability::{authored_file_target, check_authored_file_available};
#[cfg(test)]
pub(in crate::wabbajack::catalog) use page::authored_files_munged_name;
pub(in crate::wabbajack::catalog) use page::{
    AuthoredDownloadState, AuthoredFilePart, AuthoredFilesDownloadPage,
    authored_files_download_page_url, authored_files_download_target, authored_files_part_url,
    parse_authored_files_download_page, sanitize_file_name,
};

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

pub(in crate::wabbajack::catalog) async fn download_authored_wabbajack_file(
    client: &reqwest::Client,
    munged_name: &str,
    base_url: &str,
    output_dir: &Path,
) -> Result<PathBuf> {
    let page = fetch_authored_files_download_page(client, munged_name, base_url).await?;
    let file_name = sanitize_file_name(&page.file_name);
    let dest = output_dir.join(file_name);
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    download_authored_file_parts_from_page(client, page, munged_name, base_url, &dest, None, None)
        .await?;
    Ok(dest)
}

/// Download a Wabbajack authored-files URL to an exact destination path.
///
/// This is used by the installer for `WabbajackCDNDownloader` archives, where
/// the store path is hash-addressed and cannot be derived from the authored
/// file's original filename.
pub async fn download_authored_file_to_path(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected_hash: Option<u64>,
    progress: Option<&ProgressCallback>,
) -> Result<()> {
    let target = authored_file_target(url)
        .with_context(|| format!("not a Wabbajack authored-files URL: {url}"))?;
    download_authored_file_parts(
        client,
        &target.munged_name,
        &target.base_url,
        dest,
        expected_hash,
        progress,
    )
    .await
}

async fn download_authored_file_parts(
    client: &reqwest::Client,
    munged_name: &str,
    base_url: &str,
    dest: &Path,
    expected_hash: Option<u64>,
    progress: Option<&ProgressCallback>,
) -> Result<()> {
    let page = fetch_authored_files_download_page(client, munged_name, base_url).await?;
    download_authored_file_parts_from_page(
        client,
        page,
        munged_name,
        base_url,
        dest,
        expected_hash,
        progress,
    )
    .await
}

async fn fetch_authored_files_download_page(
    client: &reqwest::Client,
    munged_name: &str,
    base_url: &str,
) -> Result<AuthoredFilesDownloadPage> {
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

    parse_authored_files_download_page(&page_text).with_context(|| {
        format!("failed to parse Wabbajack authored-files metadata page {page_url}")
    })
}

async fn download_authored_file_parts_from_page(
    client: &reqwest::Client,
    page: AuthoredFilesDownloadPage,
    munged_name: &str,
    base_url: &str,
    dest: &Path,
    expected_hash: Option<u64>,
    progress: Option<&ProgressCallback>,
) -> Result<()> {
    let munged_name = if page.munged_name.is_empty() {
        munged_name
    } else {
        &page.munged_name
    };
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    // Authored CDN files can be multi-gigabyte generated outputs. Keep the
    // interrupted body and sidecar next to the final hash-addressed store path
    // so reruns can resume only the missing chunk suffix.
    let part_dest = dest.with_extension("part");
    let sidecar_dest = dest.with_extension("part.json");

    let mut parts = page.parts;
    parts.sort_by_key(|part| part.offset);
    validate_authored_parts(&parts, page.file_size_bytes)?;

    let mut state =
        AuthoredDownloadState::load_valid(&sidecar_dest, munged_name, page.file_size_bytes, &parts)
            .await
            .unwrap_or_default();
    state.munged_name = munged_name.to_string();
    state.file_size_bytes = page.file_size_bytes;
    state.parts = parts.clone();
    let expected_completed_bytes = parts
        .iter()
        .take(state.completed_parts.len())
        .map(|part| part.size)
        .sum::<u64>();

    let existing_len = tokio::fs::metadata(&part_dest)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if existing_len != expected_completed_bytes {
        let _ = tokio::fs::remove_file(&part_dest).await;
        let _ = tokio::fs::remove_file(&sidecar_dest).await;
        state = AuthoredDownloadState::default();
        state.munged_name = munged_name.to_string();
        state.file_size_bytes = page.file_size_bytes;
        state.parts = parts.clone();
    }

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&part_dest)
        .await
        .with_context(|| format!("failed to create {}", part_dest.display()))?;
    let mut written = tokio::fs::metadata(&part_dest)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    file.seek(std::io::SeekFrom::End(0)).await?;

    let mut xxh64 = Xxh64::new(0);
    let mut xxh3 = Xxh3::new();
    if expected_hash.is_some() && written > 0 {
        let mut existing = tokio::fs::File::open(&part_dest)
            .await
            .with_context(|| format!("failed to open {}", part_dest.display()))?;
        let mut buf = vec![0_u8; 1024 * 1024];
        loop {
            let read = existing.read(&mut buf).await?;
            if read == 0 {
                break;
            }
            xxh64.update(&buf[..read]);
            xxh3.update(&buf[..read]);
        }
    }

    for part in parts.into_iter().skip(state.completed_parts.len()) {
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
            if expected_hash.is_some() {
                xxh64.update(&chunk);
                xxh3.update(&chunk);
            }
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
        state.completed_parts.push(part.index);
        state
            .save(&sidecar_dest, munged_name, page.file_size_bytes)
            .await?;
        if let Some(progress) = progress {
            progress(written, page.file_size_bytes);
        }
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
    if let Some(expected_hash) = expected_hash {
        let h64 = xxh64.digest();
        let h3 = xxh3.digest();
        if h64 != expected_hash && h3 != expected_hash {
            let _ = tokio::fs::remove_file(dest).await;
            anyhow::bail!(
                "hash verification failed for {} (expected {:016x}, got xxh64 {:016x})",
                dest.display(),
                expected_hash,
                h64
            );
        }
    }
    let _ = tokio::fs::remove_file(&sidecar_dest).await;
    Ok(())
}

fn validate_authored_parts(parts: &[AuthoredFilePart], file_size_bytes: u64) -> Result<()> {
    let mut offset = 0_u64;
    for part in parts {
        if part.offset != offset {
            bail!(
                "Wabbajack authored-files part {} has offset {}, expected {}",
                part.index,
                part.offset,
                offset
            );
        }
        offset += part.size;
    }
    if offset != file_size_bytes {
        bail!(
            "Wabbajack authored-files parts total {offset} bytes, expected {file_size_bytes} bytes"
        );
    }
    Ok(())
}
