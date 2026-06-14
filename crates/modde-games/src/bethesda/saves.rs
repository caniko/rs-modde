//! Bethesda game save detection and classification.
//!
//! Covers Skyrim SE/AE, Fallout 4, Fallout 76 (partially), and Starfield.
//!
//! # Binary header format
//!
//! All Bethesda save files share this header layout:
//! ```text
//! Offset   Size  Field
//! 0        var   Magic string (see MAGIC_* constants below)
//! +0       4     headerSize  (u32 LE)
//! +4       4     saveNumber  (u32 LE)  — unique slot ID, increments each save
//! +8       2     nameLength  (u16 LE)
//! +10      var   playerName  (UTF-8, `nameLength` bytes)
//! ```
//!
//! Fallout 76 saves are server-side; the local `.sav` files are partial cache
//! entries. We capture them but label the commit with a clear warning.

use std::borrow::Cow;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::Path;
use std::time::SystemTime;

use anyhow::{Context, Result};
use smallvec::SmallVec;

use crate::save_patterns::{CaptureSummary, PatternSaveTracker, PrefixSaveRule};
use crate::traits::{DetectedSave, SaveTracker};

// ── Magic constants ───────────────────────────────────────────────────────────

const MAGIC_SKYRIM_SE: &[u8] = b"TESV_SAVEGAME"; // 13 bytes
const MAGIC_FALLOUT4: &[u8] = b"FO4_SAVEGAME"; // 12 bytes
const MAGIC_FALLOUT76: &[u8] = b"FO76_SAVEGAME"; // 13 bytes

// ── Public singletons ─────────────────────────────────────────────────────────

pub struct BethesdaSaveTracker {
    /// Magic bytes prefix that identifies save files for this game.
    magic: &'static [u8],
    /// Fallout 76 warning (server-side saves).
    is_fo76: bool,
}

pub static SKYRIM_SAVE_TRACKER: BethesdaSaveTracker = BethesdaSaveTracker {
    magic: MAGIC_SKYRIM_SE,
    is_fo76: false,
};

pub static FALLOUT4_SAVE_TRACKER: BethesdaSaveTracker = BethesdaSaveTracker {
    magic: MAGIC_FALLOUT4,
    is_fo76: false,
};

pub static FALLOUT76_SAVE_TRACKER: BethesdaSaveTracker = BethesdaSaveTracker {
    magic: MAGIC_FALLOUT76,
    is_fo76: true,
};

const STARFIELD_SAVE_PREFIXES: &[PrefixSaveRule] = &[
    PrefixSaveRule {
        prefix: "Autosave",
        category: "auto",
    },
    PrefixSaveRule {
        prefix: "Quicksave",
        category: "quick",
    },
    PrefixSaveRule {
        prefix: "Exitsave",
        category: "exit",
    },
    PrefixSaveRule {
        prefix: "Save",
        category: "manual",
    },
];

pub static STARFIELD_SAVE_TRACKER: PatternSaveTracker = PatternSaveTracker {
    prefix_rules: STARFIELD_SAVE_PREFIXES,
    file_extensions: &["sfs"],
    default_category: "manual",
    recursive: false,
    exclude_patterns: &[],
    label_extractor: starfield_save_label,
    summary: CaptureSummary::ByCategory,
};

// ── SaveTracker impl ──────────────────────────────────────────────────────────

impl SaveTracker for BethesdaSaveTracker {
    fn save_patterns(&self) -> SmallVec<[String; 2]> {
        smallvec::smallvec!["*.ess".into(), "*.bak".into()]
    }

    fn exclude_patterns(&self) -> SmallVec<[String; 2]> {
        // Skyrim stores a global "Skse" co-save alongside .ess; we don't track it as a save
        smallvec::smallvec!["*.skse".into()]
    }

    fn detect_saves(&self, save_dir: &Path) -> Result<Vec<DetectedSave>> {
        let mut saves = Vec::new();

        if !save_dir.exists() {
            return Ok(saves);
        }

        for entry in std::fs::read_dir(save_dir)
            .with_context(|| format!("failed to read directory: {}", save_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();

            // Only process .ess files (skip .bak — they're backup copies)
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext.eq_ignore_ascii_case("bak") {
                continue;
            }
            if !ext.eq_ignore_ascii_case("ess") {
                continue;
            }

            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            let category = classify_slot_name(stem);

            // Parse header to extract save number and character name
            let header = if let Ok(h) = read_save_header(&path, self.magic) {
                h
            } else {
                // Unreadable or wrong-game save — include without metadata
                let rel = path
                    .file_name()
                    .map_or_else(|| path.clone(), std::path::PathBuf::from);
                saves.push(DetectedSave {
                    rel_path: rel,
                    category,
                    label: None,
                    modified,
                });
                continue;
            };

            let label = Some(format!(
                "{} — Save {}",
                header.player_name, header.save_number
            ));
            let rel = path
                .file_name()
                .map_or_else(|| path.clone(), std::path::PathBuf::from);

            saves.push(DetectedSave {
                rel_path: rel,
                category,
                label,
                modified,
            });
        }

        // Newest first
        saves.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(saves)
    }

    fn describe_capture(&self, saves: &[DetectedSave]) -> String {
        let prefix = if self.is_fo76 {
            "capture (FO76 cache — server saves not tracked)"
        } else {
            "capture"
        };

        match saves.len() {
            0 => format!("{prefix}: no new saves"),
            1 => {
                let s = &saves[0];
                let name = s
                    .label
                    .as_deref()
                    .unwrap_or_else(|| s.rel_path.to_str().unwrap_or("unknown"));
                format!("{prefix}: {} [{}]", name, s.category)
            }
            _ => {
                // Group by character name (extracted from label before " — Save N")
                let mut chars: std::collections::BTreeMap<String, Vec<u32>> =
                    std::collections::BTreeMap::new();
                for s in saves {
                    let (char_name, slot_num) = parse_label(s.label.as_deref());
                    chars.entry(char_name).or_default().push(slot_num);
                }

                let parts: Vec<String> = chars
                    .iter()
                    .map(|(name, slots)| {
                        if slots.len() == 1 {
                            format!("{name} (slot {})", slots[0])
                        } else {
                            let mut sorted = slots.clone();
                            sorted.sort_unstable();
                            let slot_list: Vec<_> = sorted
                                .iter()
                                .map(std::string::ToString::to_string)
                                .collect();
                            format!("{name} (slots {})", slot_list.join(", "))
                        }
                    })
                    .collect();

                format!("{prefix}: {} saves — {}", saves.len(), parts.join("; "))
            }
        }
    }
}

// ── Binary header parser ──────────────────────────────────────────────────────

struct SaveHeader {
    save_number: u32,
    player_name: String,
}

/// Read the binary save header to extract save number and player name.
///
/// Returns `Err` if the file is unreadable, too short, or doesn't start
/// with the expected magic bytes (wrong game).
fn read_save_header(path: &Path, expected_magic: &[u8]) -> anyhow::Result<SaveHeader> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;

    // Read and verify magic
    let mut magic_buf = vec![0u8; expected_magic.len()];
    file.read_exact(&mut magic_buf)?;
    if magic_buf != expected_magic {
        anyhow::bail!("magic mismatch");
    }

    // Skip headerSize (4 bytes)
    file.seek(SeekFrom::Current(4))?;

    // Read saveNumber (4 bytes, LE)
    let mut num_buf = [0u8; 4];
    file.read_exact(&mut num_buf)?;
    let save_number = u32::from_le_bytes(num_buf);

    // Read playerName: u16 length prefix + data
    let mut len_buf = [0u8; 2];
    file.read_exact(&mut len_buf)?;
    let name_len = u16::from_le_bytes(len_buf) as usize;

    if name_len > 256 {
        anyhow::bail!("implausibly long player name ({name_len} bytes)");
    }

    let mut name_buf = vec![0u8; name_len];
    file.read_exact(&mut name_buf)?;
    let player_name = String::from_utf8_lossy(&name_buf).into_owned();

    Ok(SaveHeader {
        save_number,
        player_name,
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Classify the save slot type from the filename stem.
///
/// Skyrim SE save filenames follow patterns like:
/// - `Save1_XXXXXXXX_PlayerName_cell_hhmm_dd.dd.ddd.ess`  → "manual"
/// - `Autosave1.ess` → "auto"
/// - `Quicksave.ess` or `Quicksave1.ess` → "quick"
fn classify_slot_name(stem: &str) -> Cow<'static, str> {
    let lower = stem.to_lowercase();
    if lower.starts_with("autosave") {
        Cow::Borrowed("auto")
    } else if lower.starts_with("quicksave") {
        Cow::Borrowed("quick")
    } else {
        Cow::Borrowed("manual")
    }
}

/// Parse a save label like `"Lydia — Save 14"` into `("Lydia", 14)`.
/// Falls back to `("Unknown", 0)` if the format doesn't match.
fn parse_label(label: Option<&str>) -> (String, u32) {
    let Some(label) = label else {
        return ("Unknown".to_string(), 0);
    };
    if let Some((name_part, slot_part)) = label.split_once(" — Save ") {
        if let Ok(slot) = slot_part.parse() {
            return (name_part.to_string(), slot);
        }
        tracing::warn!(
            raw_slot = slot_part,
            label,
            "bethesda saves: failed to parse save slot number; treating as 0"
        );
        return (format!("{name_part} — Save {slot_part}"), 0);
    }
    (label.to_string(), 0)
}

fn starfield_save_label(path: &Path, rel_name: &str) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(std::string::ToString::to_string)
        .or_else(|| Some(rel_name.to_string()))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
