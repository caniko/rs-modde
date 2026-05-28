use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::collision::FileOrigin;
use crate::resolver::ModId;

/// Stable identifier for a merge driver implementation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct MergeDriverId(pub String);

impl MergeDriverId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for MergeDriverId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for MergeDriverId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// High-level kind of merge needed for a collided file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergeKind {
    /// Textual file that can be merged with syntax-aware tooling.
    Text { syntax: String },
    /// Bethesda plugin files (`.esp`, `.esm`, `.esl`) for future record-level handling.
    BethesdaPlugin,
    /// Cyberpunk redscript override files (`.reds`) for future handling.
    RedscriptOverride,
    /// Escape hatch for game-specific merge kinds.
    Custom(String),
}

/// Provenance for the merge base used by a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BaseSource {
    /// A vanilla game file on disk.
    Vanilla {
        abs_path: PathBuf,
        content_hash: String,
    },
    /// A generated base with a recorded reason.
    Synthetic { reason: String },
    /// No base was available.
    Missing,
}

/// Lifecycle status of a merge session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeStatus {
    Pending,
    Resolved,
    Conflicted,
    Stale,
}

impl MergeStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Resolved => "resolved",
            Self::Conflicted => "conflicted",
            Self::Stale => "stale",
        }
    }
}

impl std::str::FromStr for MergeStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "resolved" => Ok(Self::Resolved),
            "conflicted" => Ok(Self::Conflicted),
            "stale" => Ok(Self::Stale),
            other => Err(format!("unknown merge status: {other}")),
        }
    }
}

/// Tool or workflow used to resolve a merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergedWith {
    VSCode,
    Meld,
    KDiff3,
    Inline,
    ClaudeCode,
    Manual,
}

impl MergedWith {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VSCode => "vscode",
            Self::Meld => "meld",
            Self::KDiff3 => "kdiff3",
            Self::Inline => "inline",
            Self::ClaudeCode => "claude_code",
            Self::Manual => "manual",
        }
    }
}

impl std::str::FromStr for MergedWith {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "vscode" => Ok(Self::VSCode),
            "meld" => Ok(Self::Meld),
            "kdiff3" => Ok(Self::KDiff3),
            "inline" => Ok(Self::Inline),
            "claude_code" => Ok(Self::ClaudeCode),
            "manual" => Ok(Self::Manual),
            other => Err(format!("unknown merge tool: {other}")),
        }
    }
}

/// Optional path hints for future merge drivers and UI surfaces.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergePathsHint {
    pub base: Option<PathBuf>,
    pub result: Option<PathBuf>,
}

/// One mod participating in a merge session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeParticipant {
    pub mod_id: ModId,
    pub origin: FileOrigin,
    /// Hash of the participating file content when the upstream producer has it.
    ///
    /// Collision reports currently do not carry file hashes, so candidates
    /// produced from those reports leave this unset instead of inventing data.
    pub content_hash: Option<String>,
}

/// Persistent merge session metadata stored in `SQLite`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeSession {
    pub merge_group: String,
    pub rel_path: String,
    pub participants: Vec<MergeParticipant>,
    pub base: BaseSource,
    pub kind: MergeKind,
    pub status: MergeStatus,
    pub result_path: Option<PathBuf>,
    pub merged_with: Option<MergedWith>,
    pub resolved_at: Option<i64>,
}
