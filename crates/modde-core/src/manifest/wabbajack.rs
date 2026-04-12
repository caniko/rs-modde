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
            Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
        }
        serde_json::Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("hash number not a valid u64")),
        _ => Err(serde::de::Error::custom("expected string or number for hash")),
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
        _ => Err(serde::de::Error::custom("expected object or array for Headers")),
    }
}

/// Serialize a u64 hash back to Wabbajack base64 format.
fn serialize_b64_hash<S: Serializer>(val: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    let encoded = BASE64.encode(val.to_le_bytes());
    serializer.serialize_str(&encoded)
}

/// Parse a base64 hash string into u64 (for use outside serde).
pub fn parse_b64_hash(s: &str) -> Option<u64> {
    let bytes = BASE64.decode(s).ok()?;
    if bytes.len() != 8 {
        return None;
    }
    Some(u64::from_le_bytes(bytes.try_into().unwrap()))
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
    #[serde(alias = "HttpDownloader, Wabbajack.Lib")]
    HttpDownloader {
        #[serde(rename = "Url")]
        url: String,
        #[serde(
            default,
            rename = "Headers",
            deserialize_with = "deserialize_headers"
        )]
        headers: HashMap<String, String>,
    },
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
    DirectURL {
        url: String,
        headers: HashMap<String, String>,
        hash: u64,
    },
}

impl DownloadDirective {
    /// Extract the expected hash from any directive variant.
    pub fn hash(&self) -> u64 {
        match self {
            Self::Nexus { hash, .. }
            | Self::GitHub { hash, .. }
            | Self::GoogleDrive { hash, .. }
            | Self::Mega { hash, .. }
            | Self::DirectURL { hash, .. } => *hash,
        }
    }

    /// Human-readable label for progress/error messages.
    ///
    /// Returns `Cow::Borrowed` for variants where the label can be
    /// computed without allocation (currently none, but future-proofed),
    /// and `Cow::Owned` when formatting is required.
    pub fn display_name(&self) -> Cow<'_, str> {
        match self {
            Self::Nexus { mod_id, .. } => format!("nexus:{mod_id}").into(),
            Self::GitHub { repo, .. } => format!("github:{repo}").into(),
            Self::GoogleDrive { id, .. } => format!("gdrive:{id}").into(),
            Self::Mega { url, .. } => {
                format!("mega:{}", &url[..url.len().min(30)]).into()
            }
            Self::DirectURL { url, .. } => {
                format!("http:{}", &url[..url.len().min(30)]).into()
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
        to: String,
    },
    InlineFile {
        source_data_id: String,
        to: String,
    },
    PatchedFromArchive {
        archive_hash: u64,
        from: String,
        to: String,
        patch_id: String,
    },
    CreateBSA {
        temp_id: String,
        to: String,
        file_states: Vec<BSAFileState>,
    },
}

/// State for a file inside a BSA/BA2 archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BSAFileState {
    pub path: String,
    #[serde(
        deserialize_with = "deserialize_b64_hash",
        serialize_with = "serialize_b64_hash"
    )]
    pub hash: u64,
    #[serde(default)]
    pub size: u64,
}

/// Parse a hash from a serde_json::Value — tries base64 string first, then numeric.
fn parse_hash_value(val: Option<&serde_json::Value>) -> u64 {
    val.and_then(|v| {
        v.as_str()
            .and_then(parse_b64_hash)
            .or_else(|| v.as_u64())
    })
    .unwrap_or(0)
}

impl WabbajackManifest {
    /// Extract typed download directives from archive entries.
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
                    ArchiveState::HttpDownloader { url, headers } => DownloadDirective::DirectURL {
                        url: url.clone(),
                        headers: headers.clone(),
                        hash: archive.hash,
                    },
                })
            })
            .collect()
    }

    /// Extract typed install directives from raw directives.
    pub fn install_directives(&self) -> Vec<InstallDirective> {
        self.directives
            .iter()
            .filter_map(|d| match d {
                RawDirective::FromArchive {
                    archive_hash_path,
                    to,
                } => {
                    let hash = parse_hash_value(archive_hash_path.first());
                    let from = archive_hash_path
                        .get(1)
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    Some(InstallDirective::FromArchive {
                        archive_hash: hash,
                        from,
                        to: to.clone(),
                    })
                }
                RawDirective::InlineFile {
                    source_data_id,
                    to,
                    ..
                } => Some(InstallDirective::InlineFile {
                    source_data_id: source_data_id.clone(),
                    to: to.clone(),
                }),
                RawDirective::PatchedFromArchive {
                    archive_hash_path,
                    to,
                    patch_id,
                    ..
                } => {
                    let archive_hash = parse_hash_value(archive_hash_path.first());
                    let from = archive_hash_path
                        .get(1)
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    Some(InstallDirective::PatchedFromArchive {
                        archive_hash,
                        from,
                        to: to.clone(),
                        patch_id: patch_id.clone(),
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
}
