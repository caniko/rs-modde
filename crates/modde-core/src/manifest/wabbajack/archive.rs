use std::borrow::Cow;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{BSAFileState, deserialize_b64_hash, deserialize_headers, serialize_b64_hash};
use crate::nexus_id::{NexusFileId, NexusModId};
use crate::resolver::GameId;

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

impl ArchiveEntry {
    /// Build this archive's download directive (`None` if it carries no downloadable state).
    #[must_use]
    pub fn download_directive(&self) -> Option<DownloadDirective> {
        let state = self.state.as_ref()?;
        Some(match state {
            ArchiveState::NexusDownloader {
                game_name,
                mod_id,
                file_id,
            } => DownloadDirective::Nexus {
                game_id: GameId::from(game_name.clone()),
                mod_id: *mod_id,
                file_id: *file_id,
                hash: self.hash,
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
                hash: self.hash,
            },
            ArchiveState::GoogleDriveDownloader { id } => DownloadDirective::GoogleDrive {
                id: id.clone(),
                hash: self.hash,
            },
            ArchiveState::MegaDownloader { url } => DownloadDirective::Mega {
                url: url.clone(),
                hash: self.hash,
            },
            ArchiveState::MediaFireDownloader { url } => DownloadDirective::MediaFire {
                url: url.clone(),
                hash: self.hash,
            },
            ArchiveState::ManualDownloader { url, prompt } => DownloadDirective::Manual {
                url: url.clone(),
                prompt: prompt.clone(),
                hash: self.hash,
                expected_name: self.name.clone(),
            },
            ArchiveState::HttpDownloader { url, headers } => DownloadDirective::DirectURL {
                url: url.clone(),
                headers: headers.clone(),
                mirror_resolver: None,
                hash: self.hash,
            },
            ArchiveState::ModDBDownloader { url, .. } => DownloadDirective::DirectURL {
                url: url.clone(),
                headers: HashMap::new(),
                mirror_resolver: moddb_html_mirror_resolver(url),
                hash: self.hash,
            },
            ArchiveState::WabbajackCDNDownloader { metadata } => DownloadDirective::WabbajackCdn {
                url: wabbajack_cdn_url(metadata)?,
                hash: self.hash,
            },
            ArchiveState::GameFileSourceDownloader { .. } => return None,
        })
    }
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
        mod_id: NexusModId,
        #[serde(rename = "FileID")]
        file_id: NexusFileId,
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
        mod_id: NexusModId,
        file_id: NexusFileId,
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
pub(in crate::manifest::wabbajack) fn truncate_str(s: &str, max: usize) -> &str {
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

pub(in crate::manifest::wabbajack) fn moddb_html_mirror_resolver(
    url: &str,
) -> Option<HtmlMirrorResolver> {
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

pub(in crate::manifest::wabbajack) fn moddb_download_id(url: &str) -> Option<&str> {
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
