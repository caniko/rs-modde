use std::fmt;

use serde::{Deserialize, Serialize};

use crate::nexus_id::{NexusFileId, NexusModId};

/// Ecosystem-neutral artifact identity used by exact install transactions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransactionPackage {
    NexusMod {
        game_domain: String,
        mod_id: NexusModId,
    },
    WabbajackArchive {
        hash: u64,
    },
    WabbajackPatchOutput {
        patch_id: String,
        output_hash: u64,
    },
}

impl fmt::Display for TransactionPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NexusMod {
                game_domain,
                mod_id,
            } => write!(f, "nexus:{game_domain}:{mod_id}"),
            Self::WabbajackArchive { hash } => write!(f, "wabbajack-archive:{hash:016x}"),
            Self::WabbajackPatchOutput {
                patch_id,
                output_hash,
            } => write!(f, "wabbajack-patch:{patch_id}:{output_hash:016x}"),
        }
    }
}

/// Exact version identity for a transaction artifact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransactionVersion {
    NexusFile { file_id: NexusFileId },
    WabbajackHash { hash: u64 },
}

impl fmt::Display for TransactionVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NexusFile { file_id } => write!(f, "file:{file_id}"),
            Self::WabbajackHash { hash } => write!(f, "hash:{hash:016x}"),
        }
    }
}

/// A single resolved artifact selected by the SAT transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionArtifact {
    pub package: TransactionPackage,
    pub version: TransactionVersion,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_version: Option<String>,
    #[serde(default)]
    pub enabled_by_default: bool,
    pub provenance: TransactionProvenance,
}

impl fmt::Display for TransactionArtifact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(version) = &self.display_version {
            write!(f, "{} {version} ({})", self.display_name, self.version)
        } else {
            write!(f, "{} ({})", self.display_name, self.version)
        }
    }
}

/// Source that introduced an artifact into the transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionProvenance {
    NexusCollection { slug: String, version: String },
    WabbajackManifest { manifest_hash: String },
    WabbajackPatch { patch_id: String },
    NexusLazyMetadata,
}

/// Resolved, provably consistent install transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionPlan {
    pub artifacts: Vec<TransactionArtifact>,
}

/// Explainable transaction failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransactionError {
    #[error("transaction is unsatisfiable: {reason}")]
    Unsatisfiable { reason: String },
    #[error("transaction metadata is invalid: {reason}")]
    InvalidMetadata { reason: String },
    #[error("transaction metadata could not be fetched: {reason}")]
    MetadataFetch { reason: String },
}
