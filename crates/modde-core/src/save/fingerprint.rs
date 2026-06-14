use std::fmt::Write;

use sha2::{Digest, Sha256};
use smallvec::SmallVec;

use crate::profile::EnabledMod;

/// A fingerprint of the save-breaking mods active when a save was captured.
///
/// Computed as SHA-256 over the sorted list of enabled, save-breaking mod IDs
/// (and their versions). Two profiles with the same save-breaking mods produce
/// the same fingerprint, regardless of cosmetic mod differences.
///
/// Stored as a `Mod-Fingerprint:` trailer in save vault commit messages.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SaveFingerprint {
    /// Hex-encoded SHA-256 hash (first 16 characters for display).
    pub hash: String,
    /// The save-breaking mod IDs that contributed to this fingerprint.
    /// Typically 5–15 mods; `SmallVec<[_; 8]>` keeps ≤8 inline (no heap allocation).
    pub mod_ids: SmallVec<[String; 8]>,
}

/// The git commit trailer key used to store the fingerprint.
const FINGERPRINT_TRAILER: &str = "Mod-Fingerprint";

/// The git commit trailer key for the human-readable mod list.
const MODS_TRAILER: &str = "Save-Breaking-Mods";

impl SaveFingerprint {
    /// Compute a fingerprint from a list of mods and a classification function.
    ///
    /// `classify` takes a `mod_id` and returns whether it's save-breaking.
    /// This is intentionally a callback so the caller can resolve staging
    /// paths and call `GamePlugin::classify_mod` — keeping modde-core
    /// independent of modde-games.
    pub fn compute(mods: &[EnabledMod], classify: impl Fn(&str) -> bool) -> Self {
        let mut breaking_ids: Vec<&str> = mods
            .iter()
            .filter(|m| m.enabled && classify(&m.mod_id))
            .map(|m| m.mod_id.as_str())
            .collect();
        breaking_ids.sort_unstable();
        breaking_ids.dedup();

        let mut hasher = Sha256::new();
        for id in &breaking_ids {
            hasher.update(id.as_bytes());
            hasher.update(b"\0");
        }
        let mut hash = String::with_capacity(64);
        for byte in hasher.finalize() {
            write!(&mut hash, "{byte:02x}").expect("writing to String cannot fail");
        }

        Self {
            hash,
            mod_ids: breaking_ids.into_iter().map(String::from).collect(),
        }
    }

    /// Short hash for display (first 12 hex chars).
    #[must_use]
    pub fn short_hash(&self) -> &str {
        &self.hash[..self.hash.len().min(12)]
    }

    /// Empty fingerprint (no save-breaking mods).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            hash: "0".repeat(64),
            mod_ids: SmallVec::new(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mod_ids.is_empty()
    }

    /// Format as git commit trailer lines.
    pub(super) fn to_trailers(&self) -> String {
        let mut s = format!("{FINGERPRINT_TRAILER}: {}", self.short_hash());
        if !self.mod_ids.is_empty() {
            s.push_str(&format!("\n{MODS_TRAILER}: {}", self.mod_ids.join(", ")));
        }
        s
    }

    /// Parse from a git commit message (looks for trailer lines).
    pub(super) fn from_commit_message(message: &str) -> Option<Self> {
        let mut hash = None;
        let mut mod_ids = SmallVec::new();

        for line in message.lines() {
            if let Some(value) = line.strip_prefix(&format!("{FINGERPRINT_TRAILER}: ")) {
                hash = Some(value.trim().to_string());
            } else if let Some(value) = line.strip_prefix(&format!("{MODS_TRAILER}: ")) {
                mod_ids = value
                    .split(", ")
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }

        hash.map(|h| Self { hash: h, mod_ids })
    }
}

/// Result of comparing two fingerprints.
#[derive(Debug, Clone)]
pub enum FingerprintCheck {
    /// Fingerprints match — saves are compatible.
    Compatible,
    /// No fingerprint stored in the snapshot (pre-fingerprint era).
    NoFingerprint,
    /// Fingerprints differ — saves may be incompatible.
    Mismatch {
        /// Mods present in the snapshot but not the current profile.
        /// Typically 1–5 mods; `SmallVec<[_; 4]>` avoids heap for common diffs.
        removed: SmallVec<[String; 4]>,
        /// Mods present in the current profile but not the snapshot.
        added: SmallVec<[String; 4]>,
    },
}

impl FingerprintCheck {
    #[must_use]
    pub fn is_compatible(&self) -> bool {
        matches!(self, Self::Compatible | Self::NoFingerprint)
    }
}
