use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Top-level manifest from a `.wabbajack` archive (which is a zip containing JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WabbajackManifest {
    pub name: String,
    pub author: String,
    pub description: String,
    pub game: String,
    pub version: String,
    #[serde(default)]
    pub archives: Vec<ArchiveEntry>,
    #[serde(default)]
    pub directives: Vec<RawDirective>,
}

/// An archive entry referenced by hash in download/install directives.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ArchiveEntry {
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
        #[serde(default, rename = "Headers")]
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
    #[serde(alias = "PatchedFromArchive, Wabbajack.Lib")]
    PatchedFromArchive {
        #[serde(rename = "ArchiveHashPath")]
        archive_hash_path: Vec<serde_json::Value>,
        #[serde(rename = "To")]
        to: String,
        #[serde(rename = "Hash")]
        hash: u64,
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
        game_id: String,
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

/// Our typed install directive enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InstallDirective {
    FromArchive {
        archive_hash: u64,
        from: String,
        to: String,
    },
    PatchedFromArchive {
        archive_hash: u64,
        from: String,
        to: String,
        patch_hash: u64,
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
    pub hash: u64,
    #[serde(default)]
    pub size: u64,
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
                        game_id: game_name.clone(),
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
                    let hash = archive_hash_path
                        .first()
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
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
                RawDirective::PatchedFromArchive {
                    archive_hash_path,
                    to,
                    hash,
                } => {
                    let archive_hash = archive_hash_path
                        .first()
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let from = archive_hash_path
                        .get(1)
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    Some(InstallDirective::PatchedFromArchive {
                        archive_hash,
                        from,
                        to: to.clone(),
                        patch_hash: *hash,
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
