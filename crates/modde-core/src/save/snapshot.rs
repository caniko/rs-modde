use smallvec::SmallVec;

use super::{FingerprintCheck, SaveFingerprint};

pub struct SaveSnapshot {
    /// Full commit hash.
    pub id: String,
    /// Commit message.
    pub message: String,
    /// Unix timestamp.
    pub timestamp: i64,
    /// Number of files in this snapshot.
    pub file_count: usize,
    /// Mod fingerprint extracted from the commit, if present.
    pub fingerprint: Option<SaveFingerprint>,
    /// Profile name extracted from the commit message.
    pub profile_name: Option<String>,
    /// Character/player name extracted from the save label.
    pub character_name: Option<String>,
    /// Save label (e.g. "Save 14").
    pub save_label: Option<String>,
    /// Save category (e.g. "manual", "auto", "quick").
    pub category: Option<String>,
}

impl SaveSnapshot {
    /// First 8 characters of the commit hash — computed on demand
    /// instead of storing a redundant heap allocation.
    #[must_use]
    pub fn short_id(&self) -> &str {
        &self.id[..self.id.len().min(8)]
    }

    /// Human-readable title for display: character + save label, or first message line.
    #[must_use]
    pub fn display_title(&self) -> String {
        if let (Some(char_name), Some(label)) = (&self.character_name, &self.save_label) {
            format!("{char_name} — {label}")
        } else if let Some(char_name) = &self.character_name {
            char_name.clone()
        } else if let Some(label) = &self.save_label {
            label.clone()
        } else {
            self.message.lines().next().unwrap_or("").trim().to_string()
        }
    }

    /// Parse structured metadata from the commit message.
    ///
    /// Handles these formats produced by `describe_capture()`:
    /// - `"capture: Lydia — Save 14 [manual]"`
    /// - `"capture: 3 saves — Lydia (slots 1, 2); Orc Mage (slot 5)"`
    /// - `"capture saves for profile 'Default'"`
    /// - `"capture (FO76 cache — server saves not tracked): ..."`
    pub(super) fn parse_metadata_from_message(&mut self) {
        let first_line = self.message.lines().next().unwrap_or("").trim();

        // Extract profile name from "capture saves for profile 'Name'"
        if let Some(rest) = first_line.strip_prefix("capture saves for profile '") {
            if let Some(name) = rest.strip_suffix('\'') {
                self.profile_name = Some(name.to_string());
            }
            return;
        }

        // Strip capture prefix: "capture: " or "capture (FO76 ...): "
        let body = if let Some(rest) = first_line.strip_prefix("capture: ") {
            rest
        } else if first_line.starts_with("capture (") {
            // "capture (FO76 cache — server saves not tracked): Lydia — Save 14 [manual]"
            if let Some(idx) = first_line.find("): ") {
                &first_line[idx + 3..]
            } else {
                return;
            }
        } else {
            return;
        };

        // Extract category from trailing "[manual]", "[auto]", "[quick]", etc.
        let (body, category) = if let Some(bracket_start) = body.rfind(" [") {
            if body.ends_with(']') {
                let cat = &body[bracket_start + 2..body.len() - 1];
                self.category = Some(cat.to_string());
                (&body[..bracket_start], Some(cat.to_string()))
            } else {
                (body, None)
            }
        } else {
            (body, None)
        };
        let _ = category; // used above via self.category

        // Extract character name and save label from "Lydia — Save 14"
        if let Some((char_part, save_part)) = body.split_once(" — ") {
            // Check if it's a multi-save summary like "3 saves — Lydia (slots 1, 2); ..."
            if char_part.ends_with("saves")
                && char_part.chars().next().is_some_and(|c| c.is_ascii_digit())
            {
                // Multi-save: use the whole body as the label
                self.save_label = Some(first_line.to_string());
            } else {
                self.character_name = Some(char_part.to_string());
                self.save_label = Some(save_part.to_string());
            }
        } else if body != "no new saves" {
            // Single item without separator — use as label
            self.save_label = Some(body.to_string());
        }
    }

    /// Check whether this snapshot's fingerprint is compatible with the given fingerprint.
    #[must_use]
    pub fn check_compatibility(&self, current: &SaveFingerprint) -> FingerprintCheck {
        let stored = match &self.fingerprint {
            Some(fp) => fp,
            None => return FingerprintCheck::NoFingerprint,
        };

        if stored.hash == current.hash || stored.short_hash() == current.short_hash() {
            return FingerprintCheck::Compatible;
        }

        let stored_set: std::collections::HashSet<&str> = stored
            .mod_ids
            .iter()
            .map(std::string::String::as_str)
            .collect();
        let current_set: std::collections::HashSet<&str> = current
            .mod_ids
            .iter()
            .map(std::string::String::as_str)
            .collect();

        let removed: SmallVec<[String; 4]> = stored_set
            .difference(&current_set)
            .map(std::string::ToString::to_string)
            .collect();
        let added: SmallVec<[String; 4]> = current_set
            .difference(&stored_set)
            .map(std::string::ToString::to_string)
            .collect();

        FingerprintCheck::Mismatch { removed, added }
    }
}
