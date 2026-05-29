use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::resolver::GameId;

/// Deserialize a Wabbajack hash — accepts base64 string or plain integer.
fn deserialize_b64_hash<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let val = serde_json::Value::deserialize(deserializer)?;
    match &val {
        serde_json::Value::String(s) => {
            let bytes = BASE64.decode(s).map_err(serde::de::Error::custom)?;
            if bytes.len() != 8 {
                return Err(serde::de::Error::custom(format!(
                    "expected 8 bytes for hash, got {}",
                    bytes.len()
                )));
            }
            Ok(u64::from_le_bytes(
                bytes.try_into().expect("length checked to be 8 above"),
            ))
        }
        serde_json::Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("hash number not a valid u64")),
        _ => Err(serde::de::Error::custom(
            "expected string or number for hash",
        )),
    }
}

/// Deserialize Headers field — Wabbajack uses `[]` (empty array) not `{}` (empty object).
fn deserialize_headers<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<HashMap<String, String>, D::Error> {
    let val = serde_json::Value::deserialize(deserializer)?;
    match val {
        serde_json::Value::Object(map) => {
            let mut result = HashMap::new();
            for (k, v) in map {
                if let Some(s) = v.as_str() {
                    result.insert(k, s.to_string());
                }
            }
            Ok(result)
        }
        serde_json::Value::Array(_) => Ok(HashMap::new()),
        serde_json::Value::Null => Ok(HashMap::new()),
        _ => Err(serde::de::Error::custom(
            "expected object or array for Headers",
        )),
    }
}

/// Serialize a u64 hash back to Wabbajack base64 format.
fn serialize_b64_hash<S: Serializer>(val: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    let encoded = BASE64.encode(val.to_le_bytes());
    serializer.serialize_str(&encoded)
}

/// Parse a base64 hash string into u64 (for use outside serde).
#[must_use]
pub fn parse_b64_hash(s: &str) -> Option<u64> {
    let bytes = BASE64.decode(s).ok()?;
    if bytes.len() != 8 {
        return None;
    }
    Some(u64::from_le_bytes(
        bytes.try_into().expect("length checked to be 8 above"),
    ))
}

/// Top-level manifest from a `.wabbajack` archive (which is a zip containing JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WabbajackManifest {
    pub name: String,
    pub author: String,
    pub description: String,
    #[serde(alias = "GameType")]
    pub game: String,
    pub version: String,
    #[serde(default)]
    pub archives: Vec<ArchiveEntry>,
    #[serde(default)]
    pub directives: Vec<RawDirective>,
}

/// Compute a stable identifier for a Wabbajack manifest, derived from its
/// `name` + `version`. Used as the `manifest_hash` field on
/// [`crate::profile::LockReason::Wabbajack`] so install and retroactive-scan
/// flows produce identical IDs for the same modlist.
///
/// The hashing scheme is `DefaultHasher::hash(name) + hash(version)` rendered
/// in lowercase hex — matching the scheme previously inlined at
/// `crates/modde-cli/src/commands/install.rs:361-367`. Extracted here so
/// `scan --manifest` can produce bit-identical hashes during retroactive
/// lock assignment.
#[must_use]
pub fn compute_manifest_hash(manifest: &WabbajackManifest) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    manifest.name.hash(&mut hasher);
    manifest.version.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Copy a `.wabbajack` source file into the content-addressed cache so a
/// [`crate::profile::LockReason::Wabbajack`] record can be self-verifying even
/// if the original source moves or is deleted.
///
/// Idempotent: if the destination already exists, returns its path without
/// re-copying — `manifest_hash` is a stable content identifier, so two
/// different source files that share a hash are treated as equivalent.
/// Creates the cache directory on demand.
pub fn cache_wabbajack_file(source: &Path, manifest_hash: &str) -> crate::error::Result<PathBuf> {
    let dest = crate::paths::wabbajack_cache_path(manifest_hash);
    if dest.exists() {
        return Ok(dest);
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(source, &dest)?;
    Ok(dest)
}

/// An archive entry referenced by hash in download/install directives.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ArchiveEntry {
    #[serde(
        deserialize_with = "deserialize_b64_hash",
        serialize_with = "serialize_b64_hash"
    )]
    pub hash: u64,
    pub name: String,
    pub size: u64,
    #[serde(default)]
    pub state: Option<ArchiveState>,
}

/// Source-specific metadata for an archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "$type")]
pub enum ArchiveState {
    #[serde(alias = "NexusDownloader, Wabbajack.Lib")]
    NexusDownloader {
        #[serde(rename = "GameName")]
        game_name: String,
        #[serde(rename = "ModID")]
        mod_id: u64,
        #[serde(rename = "FileID")]
        file_id: u64,
    },
    #[serde(alias = "GitHubDownloader, Wabbajack.Lib")]
    GitHubDownloader {
        #[serde(rename = "User")]
        user: String,
        #[serde(rename = "Repo")]
        repo: String,
        #[serde(rename = "Tag")]
        tag: String,
        #[serde(rename = "Asset")]
        asset: String,
    },
    #[serde(alias = "GoogleDriveDownloader, Wabbajack.Lib")]
    GoogleDriveDownloader {
        #[serde(rename = "Id")]
        id: String,
    },
    #[serde(alias = "MegaDownloader, Wabbajack.Lib")]
    MegaDownloader {
        #[serde(rename = "Url")]
        url: String,
    },
    #[serde(alias = "MediaFireDownloader+State, Wabbajack.Lib")]
    MediaFireDownloader {
        #[serde(rename = "Url")]
        url: String,
    },
    #[serde(alias = "ManualDownloader, Wabbajack.Lib")]
    ManualDownloader {
        #[serde(rename = "Url")]
        url: String,
        #[serde(default, rename = "Prompt")]
        prompt: String,
    },
    #[serde(alias = "HttpDownloader, Wabbajack.Lib")]
    HttpDownloader {
        #[serde(rename = "Url")]
        url: String,
        #[serde(default, rename = "Headers", deserialize_with = "deserialize_headers")]
        headers: HashMap<String, String>,
    },
    #[serde(alias = "ModDBDownloader, Wabbajack.Lib")]
    ModDBDownloader {
        #[serde(rename = "Url")]
        url: String,
        #[serde(flatten)]
        metadata: HashMap<String, serde_json::Value>,
    },
    #[serde(alias = "GameFileSourceDownloader, Wabbajack.Lib")]
    GameFileSourceDownloader {
        #[serde(flatten)]
        metadata: HashMap<String, serde_json::Value>,
    },
    #[serde(alias = "WabbajackCDNDownloader+State, Wabbajack.Lib")]
    WabbajackCDNDownloader {
        #[serde(flatten)]
        metadata: HashMap<String, serde_json::Value>,
    },
}

impl ArchiveState {
    /// Relative path inside the game install for a Wabbajack game-file source.
    ///
    /// Wabbajack has used a few field names for this state over time. Keep the
    /// parser strict about which string is treated as a path so game/version
    /// metadata is not accidentally interpreted as a filesystem path.
    #[must_use]
    pub fn game_file_path(&self) -> Option<&str> {
        let Self::GameFileSourceDownloader { metadata } = self else {
            return None;
        };

        [
            "File",
            "FilePath",
            "GameFile",
            "GameFilePath",
            "Path",
            "RelativePath",
        ]
        .into_iter()
        .find_map(|key| metadata.get(key).and_then(serde_json::Value::as_str))
    }
}

/// A raw directive from the manifest, before we convert to our typed enums.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "$type")]
pub enum RawDirective {
    #[serde(alias = "FromArchive, Wabbajack.Lib")]
    FromArchive {
        #[serde(rename = "ArchiveHashPath")]
        archive_hash_path: Vec<serde_json::Value>,
        #[serde(rename = "To")]
        to: String,
        #[serde(default, rename = "Size")]
        size: u64,
    },
    #[serde(alias = "InlineFile, Wabbajack.Lib")]
    InlineFile {
        #[serde(
            rename = "Hash",
            deserialize_with = "deserialize_b64_hash",
            serialize_with = "serialize_b64_hash"
        )]
        hash: u64,
        #[serde(rename = "Size")]
        size: u64,
        #[serde(rename = "SourceDataID")]
        source_data_id: String,
        #[serde(rename = "To")]
        to: String,
    },
    #[serde(alias = "RemappedInlineFile, Wabbajack.Lib")]
    RemappedInlineFile {
        #[serde(
            rename = "Hash",
            deserialize_with = "deserialize_b64_hash",
            serialize_with = "serialize_b64_hash"
        )]
        hash: u64,
        #[serde(rename = "Size")]
        size: u64,
        #[serde(rename = "SourceDataID")]
        source_data_id: String,
        #[serde(rename = "To")]
        to: String,
    },
    #[serde(alias = "PatchedFromArchive, Wabbajack.Lib")]
    PatchedFromArchive {
        #[serde(rename = "ArchiveHashPath")]
        archive_hash_path: Vec<serde_json::Value>,
        #[serde(rename = "To")]
        to: String,
        #[serde(
            rename = "Hash",
            deserialize_with = "deserialize_b64_hash",
            serialize_with = "serialize_b64_hash"
        )]
        hash: u64,
        #[serde(rename = "PatchID")]
        patch_id: String,
        #[serde(default, rename = "Size")]
        size: u64,
    },
    #[serde(alias = "CreateBSA, Wabbajack.Lib")]
    CreateBSA {
        #[serde(rename = "TempID")]
        temp_id: String,
        #[serde(rename = "To")]
        to: String,
        #[serde(default, rename = "FileStates")]
        file_states: Vec<BSAFileState>,
    },
    #[serde(other)]
    Unknown,
}

/// Our typed download directive enum for downstream consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DownloadDirective {
    Nexus {
        game_id: GameId,
        mod_id: u64,
        file_id: u64,
        hash: u64,
    },
    GitHub {
        user: String,
        repo: String,
        tag: String,
        asset: String,
        hash: u64,
    },
    GoogleDrive {
        id: String,
        hash: u64,
    },
    Mega {
        url: String,
        hash: u64,
    },
    MediaFire {
        url: String,
        hash: u64,
    },
    Manual {
        url: String,
        prompt: String,
        hash: u64,
        expected_name: String,
    },
    DirectURL {
        url: String,
        headers: HashMap<String, String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mirror_resolver: Option<HtmlMirrorResolver>,
        hash: u64,
    },
    WabbajackCdn {
        url: String,
        hash: u64,
    },
}

/// Optional generic HTML mirror resolver metadata for direct downloads that
/// point at an intermediate mirror-selection page instead of a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HtmlMirrorResolver {
    pub name: String,
    pub original_url: String,
    pub listing_url: String,
    pub link_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
}

/// Truncate `s` to at most `max` bytes, backing up to a char boundary so a
/// multibyte codepoint is never split (a byte slice mid-codepoint panics).
fn truncate_str(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

impl DownloadDirective {
    /// Extract the expected hash from any directive variant.
    #[must_use]
    pub fn hash(&self) -> u64 {
        match self {
            Self::Nexus { hash, .. }
            | Self::GitHub { hash, .. }
            | Self::GoogleDrive { hash, .. }
            | Self::Mega { hash, .. }
            | Self::MediaFire { hash, .. }
            | Self::Manual { hash, .. }
            | Self::DirectURL { hash, .. }
            | Self::WabbajackCdn { hash, .. } => *hash,
        }
    }

    /// Human-readable label for progress/error messages.
    ///
    /// Returns `Cow::Borrowed` for variants where the label can be
    /// computed without allocation (currently none, but future-proofed),
    /// and `Cow::Owned` when formatting is required.
    #[must_use]
    pub fn display_name(&self) -> Cow<'_, str> {
        match self {
            Self::Nexus { mod_id, .. } => format!("nexus:{mod_id}").into(),
            Self::GitHub { repo, .. } => format!("github:{repo}").into(),
            Self::GoogleDrive { id, .. } => format!("gdrive:{id}").into(),
            Self::Mega { url, .. } => format!("mega:{}", truncate_str(url, 30)).into(),
            Self::MediaFire { url, .. } => format!("mediafire:{}", truncate_str(url, 40)).into(),
            Self::Manual { expected_name, .. } => format!("manual:{expected_name}").into(),
            Self::DirectURL { url, .. } => format!("http:{}", truncate_str(url, 30)).into(),
            Self::WabbajackCdn { url, .. } => {
                format!("wabbajack-cdn:{}", truncate_str(url, 30)).into()
            }
        }
    }
}

/// Our typed install directive enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InstallDirective {
    FromArchive {
        archive_hash: u64,
        from: String,
        inner_path: Option<String>,
        to: String,
        size: u64,
    },
    InlineFile {
        source_data_id: String,
        to: String,
    },
    PatchedFromArchive {
        archive_hash: u64,
        from: String,
        inner_path: Option<String>,
        to: String,
        patch_id: String,
        size: u64,
    },
    CreateBSA {
        temp_id: String,
        to: String,
        file_states: Vec<BSAFileState>,
    },
}

/// An install directive paired with its original manifest directive index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedInstallDirective {
    pub directive_index: usize,
    pub directive: InstallDirective,
}

/// All install directives that read from a single source archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveInstallBatch {
    pub archive_hash: u64,
    pub archive_size_bytes: u64,
    pub directives: Vec<IndexedInstallDirective>,
}

impl InstallDirective {
    fn source_archive_hash(&self) -> Option<u64> {
        match self {
            Self::FromArchive { archive_hash, .. }
            | Self::PatchedFromArchive { archive_hash, .. } => Some(*archive_hash),
            Self::InlineFile { .. } | Self::CreateBSA { .. } => None,
        }
    }

    fn source_inner_path(&self) -> &str {
        match self {
            Self::FromArchive { from, .. } | Self::PatchedFromArchive { from, .. } => from,
            Self::InlineFile { source_data_id, .. } => source_data_id,
            Self::CreateBSA { temp_id, .. } => temp_id,
        }
    }
}

/// State for a file inside a BSA/BA2 archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BSAFileState {
    pub path: String,
    #[serde(
        default,
        deserialize_with = "deserialize_b64_hash",
        serialize_with = "serialize_b64_hash"
    )]
    pub hash: u64,
    #[serde(default)]
    pub size: u64,
}

/// Parse a hash from a `serde_json::Value` — tries base64 string first, then numeric.
fn parse_hash_value(val: Option<&serde_json::Value>) -> u64 {
    val.and_then(|v| v.as_str().and_then(parse_b64_hash).or_else(|| v.as_u64()))
        .unwrap_or(0)
}

impl WabbajackManifest {
    /// Extract typed download directives from archive entries.
    #[must_use]
    pub fn download_directives(&self) -> Vec<DownloadDirective> {
        self.archives
            .iter()
            .filter_map(|archive| {
                let state = archive.state.as_ref()?;
                Some(match state {
                    ArchiveState::NexusDownloader {
                        game_name,
                        mod_id,
                        file_id,
                    } => DownloadDirective::Nexus {
                        game_id: GameId::from(game_name.clone()),
                        mod_id: *mod_id,
                        file_id: *file_id,
                        hash: archive.hash,
                    },
                    ArchiveState::GitHubDownloader {
                        user,
                        repo,
                        tag,
                        asset,
                    } => DownloadDirective::GitHub {
                        user: user.clone(),
                        repo: repo.clone(),
                        tag: tag.clone(),
                        asset: asset.clone(),
                        hash: archive.hash,
                    },
                    ArchiveState::GoogleDriveDownloader { id } => DownloadDirective::GoogleDrive {
                        id: id.clone(),
                        hash: archive.hash,
                    },
                    ArchiveState::MegaDownloader { url } => DownloadDirective::Mega {
                        url: url.clone(),
                        hash: archive.hash,
                    },
                    ArchiveState::MediaFireDownloader { url } => DownloadDirective::MediaFire {
                        url: url.clone(),
                        hash: archive.hash,
                    },
                    ArchiveState::ManualDownloader { url, prompt } => DownloadDirective::Manual {
                        url: url.clone(),
                        prompt: prompt.clone(),
                        hash: archive.hash,
                        expected_name: archive.name.clone(),
                    },
                    ArchiveState::HttpDownloader { url, headers } => DownloadDirective::DirectURL {
                        url: url.clone(),
                        headers: headers.clone(),
                        mirror_resolver: None,
                        hash: archive.hash,
                    },
                    ArchiveState::ModDBDownloader { url, .. } => DownloadDirective::DirectURL {
                        url: url.clone(),
                        headers: HashMap::new(),
                        mirror_resolver: moddb_html_mirror_resolver(url),
                        hash: archive.hash,
                    },
                    ArchiveState::WabbajackCDNDownloader { metadata } => {
                        DownloadDirective::WabbajackCdn {
                            url: wabbajack_cdn_url(metadata)?,
                            hash: archive.hash,
                        }
                    }
                    ArchiveState::GameFileSourceDownloader { .. } => return None,
                })
            })
            .collect()
    }

    /// Extract typed install directives from raw directives.
    #[must_use]
    pub fn install_directives(&self) -> Vec<InstallDirective> {
        self.directives
            .iter()
            .filter_map(|d| match d {
                RawDirective::FromArchive {
                    archive_hash_path,
                    to,
                    size,
                } => {
                    let hash = parse_hash_value(archive_hash_path.first());
                    let from = archive_hash_path
                        .get(1)
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let inner_path = archive_hash_path
                        .get(2)
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string);
                    Some(InstallDirective::FromArchive {
                        archive_hash: hash,
                        from,
                        inner_path,
                        to: to.clone(),
                        size: *size,
                    })
                }
                RawDirective::InlineFile {
                    source_data_id, to, ..
                }
                | RawDirective::RemappedInlineFile {
                    source_data_id, to, ..
                } => Some(InstallDirective::InlineFile {
                    source_data_id: source_data_id.clone(),
                    to: to.clone(),
                }),
                RawDirective::PatchedFromArchive {
                    archive_hash_path,
                    to,
                    patch_id,
                    size,
                    ..
                } => {
                    let archive_hash = parse_hash_value(archive_hash_path.first());
                    let from = archive_hash_path
                        .get(1)
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let inner_path = archive_hash_path
                        .get(2)
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string);
                    Some(InstallDirective::PatchedFromArchive {
                        archive_hash,
                        from,
                        inner_path,
                        to: to.clone(),
                        patch_id: patch_id.clone(),
                        size: *size,
                    })
                }
                RawDirective::CreateBSA {
                    temp_id,
                    to,
                    file_states,
                } => Some(InstallDirective::CreateBSA {
                    temp_id: temp_id.clone(),
                    to: to.clone(),
                    file_states: file_states.clone(),
                }),
                RawDirective::Unknown => None,
            })
            .collect()
    }

    /// Group archive-backed install directives so each source archive's work is
    /// scheduled together and can be drained by a batch-scoped reader.
    #[must_use]
    pub fn install_directives_grouped_by_archive(&self) -> Vec<ArchiveInstallBatch> {
        let archive_size_by_hash: HashMap<u64, u64> =
            self.archives.iter().map(|a| (a.hash, a.size)).collect();
        let mut by_archive: HashMap<u64, Vec<IndexedInstallDirective>> = HashMap::new();

        for (directive_index, directive) in self.install_directives().into_iter().enumerate() {
            let Some(archive_hash) = directive.source_archive_hash() else {
                continue;
            };
            by_archive
                .entry(archive_hash)
                .or_default()
                .push(IndexedInstallDirective {
                    directive_index,
                    directive,
                });
        }

        let mut batches = by_archive
            .into_iter()
            .map(|(archive_hash, mut directives)| {
                directives.sort_by(|a, b| {
                    a.directive
                        .source_inner_path()
                        .cmp(b.directive.source_inner_path())
                });
                ArchiveInstallBatch {
                    archive_hash,
                    archive_size_bytes: archive_size_by_hash
                        .get(&archive_hash)
                        .copied()
                        .unwrap_or_default(),
                    directives,
                }
            })
            .collect::<Vec<_>>();

        batches.sort_by_key(|batch| {
            batch
                .directives
                .iter()
                .map(|directive| directive.directive_index)
                .min()
                .unwrap_or(usize::MAX)
        });
        batches
    }
}

fn moddb_html_mirror_resolver(url: &str) -> Option<HtmlMirrorResolver> {
    let id = moddb_download_id(url)?;
    Some(HtmlMirrorResolver {
        name: "moddb-html-mirror".to_string(),
        original_url: url.to_string(),
        listing_url: format!("https://www.moddb.com/downloads/start/{id}/all"),
        link_id: "downloadon".to_string(),
        user_agent: Some("Wabbajack/4.0 modde".to_string()),
    })
}

fn wabbajack_cdn_url(metadata: &HashMap<String, serde_json::Value>) -> Option<String> {
    metadata
        .get("Url")
        .and_then(serde_json::Value::as_str)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
}

fn moddb_download_id(url: &str) -> Option<&str> {
    let rest = url.split_once("/downloads/start/")?.1;
    let id_len = rest
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_digit())
        .map(|(idx, ch)| idx + ch.len_utf8())
        .last()
        .unwrap_or(0);
    if id_len == 0 {
        None
    } else {
        Some(&rest[..id_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_str_never_splits_a_codepoint() {
        // 1 ASCII byte + 3-byte chars => byte 30 lands mid-codepoint.
        let s = format!("x{}", "€".repeat(20));
        assert!(!s.is_char_boundary(30));
        let t = truncate_str(&s, 30);
        assert!(t.len() <= 30 && s.is_char_boundary(t.len()));
        assert_eq!(truncate_str("abc", 30), "abc");
    }

    #[test]
    fn display_name_does_not_panic_on_multibyte_url() {
        let url = format!("https://mega.nz/{}", "€".repeat(20));
        let _ = DownloadDirective::Mega { url, hash: 0 }.display_name();
    }

    #[test]
    fn game_file_source_downloader_parses_and_is_not_downloaded() {
        let json = r#"{
            "Name": "Legends of the Frost synthetic",
            "Author": "test",
            "Description": "test",
            "Game": "SkyrimSE",
            "Version": "1.0.0",
            "Archives": [
                {
                    "Hash": "AQAAAAAAAAA=",
                    "Name": "Data_Skyrim.esm",
                    "Size": 1024,
                    "State": {
                        "$type": "GameFileSourceDownloader, Wabbajack.Lib",
                        "File": "Data\\Skyrim.esm"
                    }
                },
                {
                    "Hash": "AgAAAAAAAAA=",
                    "Name": "mod.zip",
                    "Size": 2048,
                    "State": {
                        "$type": "HttpDownloader, Wabbajack.Lib",
                        "Url": "https://example.invalid/mod.zip",
                        "Headers": []
                    }
                }
            ],
            "Directives": [
                {
                    "$type": "FromArchive, Wabbajack.Lib",
                    "ArchiveHashPath": ["AQAAAAAAAAA="],
                    "To": "mods/Skyrim Base/Skyrim.esm"
                }
            ]
        }"#;

        let manifest: WabbajackManifest = serde_json::from_str(json).unwrap();
        let source = manifest.archives[0].state.as_ref().unwrap();
        assert_eq!(source.game_file_path(), Some("Data\\Skyrim.esm"));

        let downloads = manifest.download_directives();
        assert_eq!(downloads.len(), 1);
        assert_eq!(downloads[0].hash(), 2);

        let installs = manifest.install_directives();
        assert!(matches!(
            installs.as_slice(),
            [InstallDirective::FromArchive {
                archive_hash: 1,
                from,
                to,
                ..
            }] if from.is_empty() && to == "mods/Skyrim Base/Skyrim.esm"
        ));
    }

    #[test]
    fn wabbajack_cdn_downloader_parses_as_authored_download() {
        let json = r#"{
            "Name": "Legends of the Frost synthetic",
            "Author": "test",
            "Description": "test",
            "Game": "SkyrimSE",
            "Version": "1.0.0",
            "Archives": [
                {
                    "Hash": "AQAAAAAAAAA=",
                    "Name": "Legends of the Frost - Generated Output.7z",
                    "Size": 1024,
                    "State": {
                        "$type": "WabbajackCDNDownloader+State, Wabbajack.Lib",
                        "Url": "https://authored-files.wabbajack.org/Generated%20Output.7z_abc",
                        "MungedName": "Generated Output.7z_abc"
                    }
                },
                {
                    "Hash": "AgAAAAAAAAA=",
                    "Name": "mod.zip",
                    "Size": 2048,
                    "State": {
                        "$type": "HttpDownloader, Wabbajack.Lib",
                        "Url": "https://example.invalid/mod.zip",
                        "Headers": []
                    }
                }
            ],
            "Directives": []
        }"#;

        let manifest: WabbajackManifest = serde_json::from_str(json).unwrap();
        assert!(matches!(
            manifest.archives[0].state.as_ref(),
            Some(ArchiveState::WabbajackCDNDownloader { metadata })
                if metadata.get("MungedName").and_then(serde_json::Value::as_str)
                    == Some("Generated Output.7z_abc")
        ));

        let downloads = manifest.download_directives();
        assert_eq!(downloads.len(), 2);
        assert!(matches!(
            &downloads[0],
            DownloadDirective::WabbajackCdn { url, hash }
                if url == "https://authored-files.wabbajack.org/Generated%20Output.7z_abc"
                    && *hash == 1
        ));
        assert_eq!(downloads[1].hash(), 2);
    }

    #[test]
    fn moddb_downloader_parses_as_direct_url_download() {
        let json = r#"{
            "Name": "Legends of the Frost synthetic",
            "Author": "test",
            "Description": "test",
            "Game": "SkyrimSE",
            "Version": "1.0.0",
            "Archives": [
                {
                    "Hash": "AQAAAAAAAAA=",
                    "Name": "Skyrim_Realistic_Overhaul_Part_1.7z",
                    "Size": 1024,
                    "State": {
                        "$type": "ModDBDownloader, Wabbajack.Lib",
                        "Url": "https://www.moddb.com/downloads/start/116891",
                        "PrimaryKeyString": "ModDBDownloader+State|https://www.moddb.com/downloads/start/116891"
                    }
                }
            ],
            "Directives": []
        }"#;

        let manifest: WabbajackManifest = serde_json::from_str(json).unwrap();
        assert!(matches!(
            manifest.archives[0].state.as_ref(),
            Some(ArchiveState::ModDBDownloader { url, metadata })
                if url == "https://www.moddb.com/downloads/start/116891"
                    && metadata.contains_key("PrimaryKeyString")
        ));

        let downloads = manifest.download_directives();
        assert!(matches!(
            downloads.as_slice(),
            [DownloadDirective::DirectURL {
                url,
                headers,
                mirror_resolver,
                hash
            }]
                if url == "https://www.moddb.com/downloads/start/116891"
                    && headers.is_empty()
                    && mirror_resolver.as_ref().is_some_and(|resolver| {
                        resolver.name == "moddb-html-mirror"
                            && resolver.original_url == "https://www.moddb.com/downloads/start/116891"
                            && resolver.listing_url == "https://www.moddb.com/downloads/start/116891/all"
                            && resolver.link_id == "downloadon"
                            && resolver.user_agent.as_deref() == Some("Wabbajack/4.0 modde")
                    })
                    && *hash == 1
        ));
    }

    #[test]
    fn moddb_downloader_derives_mirror_listing_url_with_query_string() {
        assert_eq!(
            moddb_download_id("https://www.moddb.com/downloads/start/116927?referer=x"),
            Some("116927")
        );
        let resolver =
            moddb_html_mirror_resolver("https://www.moddb.com/downloads/start/116927?referer=x")
                .unwrap();
        assert_eq!(
            resolver.listing_url,
            "https://www.moddb.com/downloads/start/116927/all"
        );
    }

    #[test]
    fn create_bsa_file_state_hash_defaults_when_absent() {
        let json = r#"{
            "Name": "Legends of the Frost synthetic",
            "Author": "test",
            "Description": "test",
            "Game": "SkyrimSE",
            "Version": "1.0.0",
            "Archives": [],
            "Directives": [
                {
                    "$type": "CreateBSA, Wabbajack.Lib",
                    "TempID": "textures.bsa",
                    "To": "mods/Generated/textures.bsa",
                    "FileStates": [
                        {
                            "$type": "BSAFileState, Compression.BSA",
                            "FlipCompression": false,
                            "Index": 0,
                            "Path": "textures\\architecture\\riften\\riftenrope01.dds"
                        }
                    ]
                }
            ]
        }"#;

        let manifest: WabbajackManifest = serde_json::from_str(json).unwrap();
        let installs = manifest.install_directives();
        assert!(matches!(
            installs.as_slice(),
            [InstallDirective::CreateBSA { file_states, .. }]
                if file_states.len() == 1
                    && file_states[0].path == "textures\\architecture\\riften\\riftenrope01.dds"
                    && file_states[0].hash == 0
        ));
    }

    #[test]
    fn remapped_inline_file_parses_as_inline_install_directive() {
        let json = r#"{
            "Name": "Twisted synthetic",
            "Author": "test",
            "Description": "test",
            "Game": "SkyrimSpecialEdition",
            "Version": "1.0.0",
            "Archives": [],
            "Directives": [
                {
                    "$type": "RemappedInlineFile",
                    "Hash": "H6Wy/QKDBVE=",
                    "Size": 6022,
                    "SourceDataID": "db027c84-eb75-4852-ae01-72cc75abe3a1",
                    "To": "mods\\BodySlide and Outfit Studio\\CalienteTools\\BodySlide\\Config.xml"
                }
            ]
        }"#;

        let manifest: WabbajackManifest = serde_json::from_str(json).unwrap();
        let installs = manifest.install_directives();
        assert!(matches!(
            installs.as_slice(),
            [InstallDirective::InlineFile { source_data_id, to }]
                if source_data_id == "db027c84-eb75-4852-ae01-72cc75abe3a1"
                    && to == "mods\\BodySlide and Outfit Studio\\CalienteTools\\BodySlide\\Config.xml"
        ));
    }

    #[test]
    fn install_directives_grouped_by_archive_returns_one_batch_per_archive() {
        let manifest = WabbajackManifest {
            name: "batch synthetic".into(),
            author: "test".into(),
            description: "test".into(),
            game: "SkyrimSE".into(),
            version: "1.0.0".into(),
            archives: vec![
                ArchiveEntry {
                    hash: 10,
                    name: "a.7z".into(),
                    size: 1024,
                    state: None,
                },
                ArchiveEntry {
                    hash: 20,
                    name: "b.7z".into(),
                    size: 2048,
                    state: None,
                },
            ],
            directives: vec![
                RawDirective::FromArchive {
                    archive_hash_path: vec![
                        serde_json::Value::Number(10.into()),
                        serde_json::Value::String("z-last.txt".into()),
                    ],
                    to: "mods/a/z-last.txt".into(),
                    size: 0,
                },
                RawDirective::InlineFile {
                    hash: 0,
                    size: 1,
                    source_data_id: "inline".into(),
                    to: "mods/inline.txt".into(),
                },
                RawDirective::PatchedFromArchive {
                    archive_hash_path: vec![
                        serde_json::Value::Number(20.into()),
                        serde_json::Value::String("only.txt".into()),
                    ],
                    to: "mods/b/only.txt".into(),
                    hash: 0,
                    patch_id: "patch".into(),
                    size: 123,
                },
                RawDirective::FromArchive {
                    archive_hash_path: vec![
                        serde_json::Value::Number(10.into()),
                        serde_json::Value::String("a-first.txt".into()),
                    ],
                    to: "mods/a/a-first.txt".into(),
                    size: 0,
                },
                RawDirective::CreateBSA {
                    temp_id: "temp".into(),
                    to: "mods/out.bsa".into(),
                    file_states: vec![],
                },
            ],
        };

        let batches = manifest.install_directives_grouped_by_archive();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].archive_hash, 10);
        assert_eq!(batches[0].archive_size_bytes, 1024);
        assert_eq!(batches[0].directives.len(), 2);
        assert_eq!(batches[0].directives[0].directive_index, 3);
        assert!(matches!(
            &batches[0].directives[0].directive,
            InstallDirective::FromArchive { from, .. } if from == "a-first.txt"
        ));
        assert_eq!(batches[0].directives[1].directive_index, 0);
        assert!(matches!(
            &batches[1].directives[..],
            [IndexedInstallDirective {
                directive_index: 2,
                directive: InstallDirective::PatchedFromArchive {
                    archive_hash: 20,
                    size: 123,
                    ..
                }
            }]
        ));
    }
}
