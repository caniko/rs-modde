//! Save snapshot detail panel — rendered at the bottom of the left sidebar
//! when a save snapshot is selected in the saves view.

use modde_core::save::{FingerprintCheck, SaveFingerprint, SaveSnapshot};

/// State for the currently-selected save snapshot's detail panel.
#[derive(Debug, Clone)]
pub struct SaveDetailsState {
    /// Full commit hash — used to reject stale async results.
    pub commit_id: String,
    /// Short commit hash for display.
    pub short_id: String,
    /// Unix timestamp of the snapshot.
    pub timestamp: i64,
    /// Profile name extracted from the commit message.
    pub profile_name: Option<String>,
    /// Character/player name extracted from the save label.
    pub character_name: Option<String>,
    /// Save label (e.g. "Save 14").
    pub save_label: Option<String>,
    /// Save category (e.g. "manual", "auto", "quick").
    pub category: Option<String>,
    /// Number of files in this snapshot.
    pub file_count: usize,
    /// File paths in the snapshot tree. `None` until loaded.
    pub file_paths: Option<Vec<String>>,
    /// Mod fingerprint from the commit.
    pub fingerprint: Option<SaveFingerprint>,
    /// Compatibility check result against the current profile.
    pub compatibility: Option<FingerprintCheck>,
}

impl SaveDetailsState {
    /// Construct from a `SaveSnapshot` and an optional compatibility check result.
    #[must_use]
    pub fn from_snapshot(snap: &SaveSnapshot, compat: Option<FingerprintCheck>) -> Self {
        Self {
            commit_id: snap.id.clone(),
            short_id: snap.short_id().to_string(),
            timestamp: snap.timestamp,
            profile_name: snap.profile_name.clone(),
            character_name: snap.character_name.clone(),
            save_label: snap.save_label.clone(),
            category: snap.category.clone(),
            file_count: snap.file_count,
            file_paths: None,
            fingerprint: snap.fingerprint.clone(),
            compatibility: compat,
        }
    }

    /// Formatted date string for display.
    #[must_use]
    pub fn formatted_date(&self) -> String {
        modde_core::save::format_timestamp(self.timestamp)
    }

    /// Human-readable title: character + save label, or fallback.
    #[must_use]
    pub fn display_title(&self) -> String {
        if let (Some(char_name), Some(label)) = (&self.character_name, &self.save_label) {
            format!("{char_name} — {label}")
        } else if let Some(char_name) = &self.character_name {
            char_name.clone()
        } else if let Some(label) = &self.save_label {
            label.clone()
        } else {
            "Save snapshot".to_string()
        }
    }
}
