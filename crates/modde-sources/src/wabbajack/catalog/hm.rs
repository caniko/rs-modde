use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use base64::Engine as _;
use sha2::{Digest, Sha256};

use super::{
    CatalogSource, WabbajackCatalogEntry, download_wabbajack_file, fetch_catalog, find_entry,
};

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
            return Ok((
                format_hm_path_snippet(profile, game, game_dir, &path),
                Some(path),
            ));
        }
        let url = resolve_download_target(client, source, CatalogSource::Both).await?;
        let path = download_wabbajack_file(client, &url, cache_dir).await?;
        (url, Some(path))
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
    let mut out = format_hm_profile_prefix(profile, game, game_dir);
    out.push_str(&format!(
        "  wabbajackList = {{\n    url = \"{}\";\n    hash = \"{}\";\n  }};\n}};\n",
        escape_nix_string(url),
        escape_nix_string(hash)
    ));
    out
}

#[must_use]
pub fn format_hm_path_snippet(
    profile: &str,
    game: &str,
    game_dir: Option<&Path>,
    path: &Path,
) -> String {
    let mut out = format_hm_profile_prefix(profile, game, game_dir);
    out.push_str(&format!(
        "  wabbajackList = {{\n    path = \"{}\";\n  }};\n}};\n",
        escape_nix_string(&path.display().to_string())
    ));
    out
}

fn format_hm_profile_prefix(profile: &str, game: &str, game_dir: Option<&Path>) -> String {
    let mut out = format!(
        "programs.modde.profiles.{profile} = {{\n  game = \"{game}\";\n  installMode = \"auto\";\n"
    );
    if let Some(game_dir) = game_dir {
        out.push_str(&format!(
            "  gameDir = \"{}\";\n",
            escape_nix_string(&game_dir.display().to_string())
        ));
    }
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
