use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

mod archive;
pub use archive::*;

#[cfg(test)]
use archive::{moddb_download_id, moddb_html_mirror_resolver, truncate_str};

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
            .filter_map(ArchiveEntry::download_directive)
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

#[cfg(test)]
mod tests;
