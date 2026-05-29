use serde::{Deserialize, Serialize};

use crate::nexus_id::{NexusFileId, NexusModId};

/// Nexus Collections API response for `/v1/collections`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionManifest {
    pub slug: String,
    pub name: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub author: CollectionAuthor,
    pub game: CollectionGame,
    #[serde(default)]
    pub mods: Vec<CollectionMod>,
    pub version: CollectionVersion,
    #[serde(default)]
    pub endorsements: u64,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionAuthor {
    pub name: String,
    pub member_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionGame {
    pub id: u64,
    pub domain_name: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionMod {
    pub mod_id: NexusModId,
    pub file_id: NexusFileId,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub install_order: i32,
    pub patch: Option<CollectionPatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionPatch {
    pub hash: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionVersion {
    pub version: String,
    pub created_at: Option<String>,
}

/// Search results wrapper from Nexus Collections API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSearchResults {
    #[serde(default)]
    pub collections: Vec<CollectionManifest>,
    pub total: Option<u64>,
}
