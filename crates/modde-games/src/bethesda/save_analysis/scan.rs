#![allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn validate_save_header(bytes: &[u8], expected_magic: &[u8]) -> Result<()> {
    anyhow::ensure!(
        bytes.len() >= expected_magic.len() + 14,
        "save is truncated before the Bethesda header is complete"
    );
    anyhow::ensure!(
        bytes.starts_with(expected_magic),
        "save magic mismatch; expected {}",
        String::from_utf8_lossy(expected_magic)
    );
    let header_size_offset = expected_magic.len();
    let header_size = u32::from_le_bytes(
        bytes[header_size_offset..header_size_offset + 4]
            .try_into()
            .expect("slice length checked above"),
    );
    anyhow::ensure!(header_size > 0, "save header size is zero");
    Ok(())
}

pub(super) fn save_scan_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut scan = bytes.to_vec();
    for payload in decompress_embedded_payloads(bytes) {
        scan.push(b' ');
        scan.extend_from_slice(&payload);
    }
    scan
}

pub(super) fn decompress_embedded_payloads(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut payloads = Vec::new();
    for offset in 0..bytes.len().saturating_sub(2) {
        let tail = &bytes[offset..];
        if looks_like_zlib(tail)
            && let Ok(payload) = decompress_zlib(tail)
        {
            payloads.push(payload);
        }
        if tail.starts_with(&[0x04, 0x22, 0x4d, 0x18])
            && let Ok(payload) = decompress_lz4_frame(tail)
        {
            payloads.push(payload);
        }
    }
    payloads
}

pub(super) fn looks_like_zlib(bytes: &[u8]) -> bool {
    bytes.len() >= 2
        && bytes[0] == 0x78
        && matches!(bytes[1], 0x01 | 0x5e | 0x9c | 0xda)
        && u16::from_be_bytes([bytes[0], bytes[1]]).is_multiple_of(31)
}

pub(super) fn decompress_zlib(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = flate2::read::ZlibDecoder::new(bytes);
    let mut payload = Vec::new();
    decoder
        .by_ref()
        .take(MAX_DECOMPRESSED_SAVE_SCAN_BYTES)
        .read_to_end(&mut payload)?;
    anyhow::ensure!(!payload.is_empty(), "zlib payload is empty");
    Ok(payload)
}

pub(super) fn decompress_lz4_frame(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = lz4_flex::frame::FrameDecoder::new(bytes);
    let mut payload = Vec::new();
    decoder
        .by_ref()
        .take(MAX_DECOMPRESSED_SAVE_SCAN_BYTES)
        .read_to_end(&mut payload)?;
    anyhow::ensure!(!payload.is_empty(), "lz4 payload is empty");
    Ok(payload)
}

pub(super) fn extract_names_by_extension(bytes: &[u8], extensions: &[&str]) -> BTreeSet<String> {
    let ascii = lossy_ascii_lower(bytes);
    let mut out = BTreeSet::new();
    for token in ascii.split(|c: char| !is_symbol_char(c)) {
        let token = token.trim_matches('.');
        if token.is_empty() {
            continue;
        }
        if extensions
            .iter()
            .any(|ext| token.ends_with(&format!(".{ext}")))
            && let Some(name) = token.rsplit(['/', '\\', '\0']).next()
        {
            out.insert(name.to_string());
        }
    }
    out
}

pub(super) fn extract_script_names(bytes: &[u8]) -> BTreeSet<String> {
    let ascii = lossy_ascii_lower(bytes);
    let mut out = BTreeSet::new();
    for token in ascii.split(|c: char| !is_symbol_char(c)) {
        let token = token.trim_matches('.');
        if token.ends_with(".pex") || token.ends_with(".psc") {
            if let Some(stem) = Path::new(token)
                .file_stem()
                .and_then(OsStr::to_str)
                .filter(|s| !s.is_empty())
            {
                out.insert(stem.to_string());
            }
        } else if looks_like_papyrus_script_name(token) {
            out.insert(token.to_string());
        }
    }
    out
}

pub(super) fn extract_tagged_script_names(bytes: &[u8], tags: &[&str]) -> BTreeSet<String> {
    let ascii = lossy_ascii_lower(bytes);
    let mut out = BTreeSet::new();
    for tag in tags {
        let mut search = ascii.as_str();
        while let Some(idx) = search.find(tag) {
            let after = &search[idx + tag.len()..];
            for token in after
                .split(|c: char| !is_symbol_char(c))
                .filter(|s| !s.is_empty())
                .take(8)
            {
                let token = token.trim_matches('.');
                if token.ends_with(".pex") || looks_like_papyrus_script_name(token) {
                    let stem = token.strip_suffix(".pex").unwrap_or(token);
                    out.insert(stem.to_string());
                    break;
                }
            }
            search = &after[after.len().min(1)..];
        }
    }
    out
}

pub(super) fn looks_like_papyrus_script_name(token: &str) -> bool {
    token.len() >= 3
        && token.len() <= 128
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        && !token.chars().all(|c| c.is_ascii_digit())
        && (token.contains("script")
            || token.starts_with("qf_")
            || token.starts_with("pf_")
            || token.starts_with("tif_"))
}

pub(super) fn lossy_ascii_lower(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| {
            if byte.is_ascii_graphic() || *byte == b' ' {
                (*byte as char).to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect()
}

pub(super) fn is_symbol_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '\\')
}

pub(super) fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
}

pub(super) fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
