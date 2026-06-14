use serde::{Deserialize, Serialize};

use crate::installer::InstallMethod;
use crate::patcher::{PatcherStageKind, PatcherStageSettings};
use crate::profile::{LoadOrderLock, ProfileSource};

pub const LOCK_FORMAT_VERSION: u32 = 1;
pub(in crate::lockfile) const LOCK_KIND: &str = "modde.lock";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModdeLock {
    pub kind: String,
    pub format_version: u32,
    pub payload: LockPayload,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signatures: Vec<LockSignature>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockPayload {
    pub modde_version: String,
    pub generated_at: String,
    pub reproducible: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incomplete_reasons: Vec<IncompleteReason>,
    pub profile: LockProfile,
    pub mods: Vec<LockedMod>,
    pub plugin_order: Vec<PluginEntryLock>,
    pub hidden_files: Vec<HiddenFileLock>,
    pub patchers: Vec<PatcherStageLock>,
    pub tool_outputs: Vec<ToolOutputLock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wabbajack_manifest: Option<WabbajackManifestLock>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockProfile {
    pub name: String,
    pub game_id: String,
    pub source: ProfileSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_order_lock: Option<LoadOrderLock>,
    pub mod_order: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedMod {
    pub order: usize,
    pub mod_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nexus: Option<NexusProvenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_archive_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_method: Option<InstallMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fomod_config: Option<String>,
    pub files: Vec<LockedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NexusProvenance {
    pub game_domain: String,
    pub mod_id: u64,
    pub file_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedFile {
    pub rel_path: String,
    pub origin_rel_path: String,
    pub size: u64,
    pub sha256: String,
    pub xxh64: String,
    pub xxh3: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginEntryLock {
    pub plugin_name: String,
    pub sort_index: i64,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HiddenFileLock {
    pub mod_id: String,
    pub rel_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatcherStageLock {
    pub name: String,
    pub stage_kind: PatcherStageKind,
    pub enabled: bool,
    pub sort_index: i64,
    pub settings: PatcherStageSettings,
    pub output_mod: String,
    pub outputs: Vec<LockedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolOutputLock {
    pub tool_id: String,
    pub rel_path: String,
    pub file: LockedFile,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WabbajackManifestLock {
    pub manifest_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IncompleteReason {
    pub subject: String,
    pub reason: String,
    pub required_source: String,
    pub regenerate: String,
    pub validate: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockSignature {
    pub key_id: String,
    pub public_key: String,
    pub signed_at: String,
    pub payload_sha256: String,
    pub signature: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportOptions {
    pub allow_incomplete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    pub signature_count: usize,
    pub checked_files: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicKeyFile {
    pub kind: String,
    pub key_id: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretKeyFile {
    pub kind: String,
    pub key_id: String,
    pub secret_key: String,
    pub public_key: String,
}
