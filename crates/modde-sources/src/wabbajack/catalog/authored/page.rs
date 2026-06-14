use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::super::{AUTHORED_FILES_CDN_PREFIX, AUTHORED_FILES_DOWNLOAD_MARKER, AUTHORED_FILES_URL};

#[cfg(test)]
pub(in crate::wabbajack::catalog) fn authored_files_munged_name(url: &str) -> Option<String> {
    authored_files_download_target(url).map(|(_, munged_name)| munged_name)
}

pub(in crate::wabbajack::catalog) fn authored_files_download_target(
    url: &str,
) -> Option<(String, String)> {
    if let Some(munged_name) = url
        .strip_prefix(AUTHORED_FILES_CDN_PREFIX)
        .filter(|name| !name.is_empty())
        .map(percent_decode_lossy)
    {
        return Some((AUTHORED_FILES_URL.to_string(), munged_name));
    }

    let marker_start = url.find(AUTHORED_FILES_DOWNLOAD_MARKER)?;
    let munged_start = marker_start + AUTHORED_FILES_DOWNLOAD_MARKER.len();
    let munged_name = url.get(munged_start..)?.split(['?', '#']).next()?;
    if munged_name.is_empty() {
        return None;
    }
    let base_end = marker_start + "/authored_files".len();
    Some((
        url[..base_end].to_string(),
        percent_decode_lossy(munged_name),
    ))
}

pub(in crate::wabbajack::catalog) fn authored_files_download_page_url(
    base_url: &str,
    munged_name: &str,
) -> String {
    format!(
        "{}/download/{}",
        base_url.trim_end_matches('/'),
        encode_path_segment(munged_name)
    )
}

pub(in crate::wabbajack::catalog) fn authored_files_part_url(
    base_url: &str,
    munged_name: &str,
    index: u64,
) -> String {
    format!(
        "{}/{}/parts/{index}",
        base_url.trim_end_matches('/'),
        encode_path_segment(munged_name)
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub(in crate::wabbajack::catalog) struct AuthoredFilePart {
    pub(in crate::wabbajack::catalog) size: u64,
    pub(in crate::wabbajack::catalog) offset: u64,
    pub(in crate::wabbajack::catalog) index: u64,
}

#[derive(Debug, Clone)]
pub(in crate::wabbajack::catalog) struct AuthoredFilesDownloadPage {
    pub(in crate::wabbajack::catalog) munged_name: String,
    pub(in crate::wabbajack::catalog) file_name: String,
    pub(in crate::wabbajack::catalog) file_size_bytes: u64,
    pub(in crate::wabbajack::catalog) parts: Vec<AuthoredFilePart>,
}

/// Resume metadata for the `.part` file beside an authored-files download.
/// It is accepted only when the metadata page still describes the same file and
/// the completed prefix length matches the downloaded body.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(in crate::wabbajack::catalog) struct AuthoredDownloadState {
    pub(in crate::wabbajack::catalog) munged_name: String,
    pub(in crate::wabbajack::catalog) file_size_bytes: u64,
    pub(in crate::wabbajack::catalog) parts: Vec<AuthoredFilePart>,
    pub(in crate::wabbajack::catalog) completed_parts: Vec<u64>,
}

impl AuthoredDownloadState {
    pub(in crate::wabbajack::catalog) async fn load_valid(
        path: &Path,
        munged_name: &str,
        file_size_bytes: u64,
        parts: &[AuthoredFilePart],
    ) -> Option<Self> {
        let bytes = tokio::fs::read(path).await.ok()?;
        let state: Self = serde_json::from_slice(&bytes).ok()?;
        if state.munged_name == munged_name
            && state.file_size_bytes == file_size_bytes
            && state.parts == parts
            && state.completed_parts.len() <= parts.len()
            && state
                .completed_parts
                .iter()
                .zip(parts.iter())
                .all(|(completed, part)| *completed == part.index)
        {
            Some(state)
        } else {
            None
        }
    }

    pub(in crate::wabbajack::catalog) async fn save(
        &self,
        path: &Path,
        munged_name: &str,
        file_size_bytes: u64,
    ) -> Result<()> {
        let state = Self {
            munged_name: munged_name.to_string(),
            file_size_bytes,
            parts: self.parts.clone(),
            completed_parts: self.completed_parts.clone(),
        };
        tokio::fs::write(path, serde_json::to_vec_pretty(&state)?)
            .await
            .with_context(|| format!("failed to write {}", path.display()))
    }
}

pub(in crate::wabbajack::catalog) fn parse_authored_files_download_page(
    input: &str,
) -> Result<AuthoredFilesDownloadPage> {
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

pub(in crate::wabbajack::catalog) fn sanitize_file_name(name: &str) -> String {
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
