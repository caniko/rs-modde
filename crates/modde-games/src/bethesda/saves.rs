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

use anyhow::Result;
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

        for entry in std::fs::read_dir(save_dir)? {
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
    let mut file = std::fs::File::open(path)?;

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
        match slot_part.parse() {
            Ok(slot) => return (name_part.to_string(), slot),
            Err(_) => {
                tracing::warn!(
                    raw_slot = slot_part,
                    label,
                    "bethesda saves: failed to parse save slot number; treating as 0"
                );
                return (format!("{name_part} — Save {slot_part}"), 0);
            }
        }
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
mod tests {
    use super::*;

    // ── classify_slot_name ───────────────────────────────────────────

    #[test]
    fn classify_autosave() {
        assert_eq!(classify_slot_name("Autosave1"), "auto");
        assert_eq!(classify_slot_name("autosave"), "auto");
    }

    #[test]
    fn classify_quicksave() {
        assert_eq!(classify_slot_name("Quicksave"), "quick");
        assert_eq!(classify_slot_name("Quicksave5"), "quick");
        assert_eq!(classify_slot_name("quicksave1"), "quick");
    }

    #[test]
    fn classify_manual_save() {
        assert_eq!(
            classify_slot_name("Save1_DEADBEEF_Lydia_WhiterunWorld"),
            "manual"
        );
        assert_eq!(classify_slot_name("Save42"), "manual");
    }

    // ── parse_label ──────────────────────────────────────────────────

    #[test]
    fn parse_label_valid() {
        let (name, slot) = parse_label(Some("Lydia — Save 14"));
        assert_eq!(name, "Lydia");
        assert_eq!(slot, 14);
    }

    #[test]
    fn parse_label_no_label() {
        let (name, slot) = parse_label(None);
        assert_eq!(name, "Unknown");
        assert_eq!(slot, 0);
    }

    #[test]
    fn parse_label_no_separator() {
        let (name, slot) = parse_label(Some("Just a name"));
        assert_eq!(name, "Just a name");
        assert_eq!(slot, 0);
    }

    #[test]
    fn parse_label_unparseable_slot_preserves_raw_text() {
        let (name, slot) = parse_label(Some("Lydia — Save abc"));
        assert_eq!(name, "Lydia — Save abc");
        assert_eq!(slot, 0);
    }

    #[test]
    fn parse_label_empty_slot_preserves_raw_text() {
        let (name, slot) = parse_label(Some("Lydia — Save "));
        assert_eq!(name, "Lydia — Save ");
        assert_eq!(slot, 0);
    }

    // ── read_save_header ─────────────────────────────────────────────

    /// Build a minimal .ess file in memory.
    fn make_ess(magic: &[u8], save_number: u32, player_name: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(magic); // magic
        buf.extend_from_slice(&0u32.to_le_bytes()); // headerSize (ignored)
        buf.extend_from_slice(&save_number.to_le_bytes()); // saveNumber
        let name_bytes = player_name.as_bytes();
        buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes()); // nameLength
        buf.extend_from_slice(name_bytes); // playerName
        buf
    }

    #[test]
    fn read_header_skyrim_se() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Save1.ess");
        std::fs::write(&path, make_ess(MAGIC_SKYRIM_SE, 42, "Lydia")).unwrap();

        let hdr = read_save_header(&path, MAGIC_SKYRIM_SE).unwrap();
        assert_eq!(hdr.save_number, 42);
        assert_eq!(hdr.player_name, "Lydia");
    }

    #[test]
    fn read_header_fallout4() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Save1.ess");
        std::fs::write(&path, make_ess(MAGIC_FALLOUT4, 7, "Sole Survivor")).unwrap();

        let hdr = read_save_header(&path, MAGIC_FALLOUT4).unwrap();
        assert_eq!(hdr.save_number, 7);
        assert_eq!(hdr.player_name, "Sole Survivor");
    }

    #[test]
    fn read_header_wrong_magic_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Save1.ess");
        std::fs::write(&path, make_ess(MAGIC_FALLOUT4, 1, "X")).unwrap();

        // Trying to read it as Skyrim should fail
        assert!(read_save_header(&path, MAGIC_SKYRIM_SE).is_err());
    }

    #[test]
    fn read_header_empty_player_name() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Save1.ess");
        std::fs::write(&path, make_ess(MAGIC_SKYRIM_SE, 1, "")).unwrap();

        let hdr = read_save_header(&path, MAGIC_SKYRIM_SE).unwrap();
        assert_eq!(hdr.player_name, "");
    }

    #[test]
    fn read_header_nonexistent_file() {
        let result = read_save_header(
            std::path::Path::new("/nonexistent/save.ess"),
            MAGIC_SKYRIM_SE,
        );
        assert!(result.is_err());
    }

    // ── detect_saves ─────────────────────────────────────────────────

    #[test]
    fn detect_saves_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let saves = SKYRIM_SAVE_TRACKER.detect_saves(tmp.path()).unwrap();
        assert!(saves.is_empty());
    }

    #[test]
    fn detect_saves_nonexistent_dir() {
        let saves = SKYRIM_SAVE_TRACKER
            .detect_saves(std::path::Path::new("/nonexistent/saves"))
            .unwrap();
        assert!(saves.is_empty());
    }

    #[test]
    fn detect_saves_finds_ess_files() {
        let tmp = tempfile::tempdir().unwrap();
        let ess = tmp.path().join("Save1.ess");
        std::fs::write(&ess, make_ess(MAGIC_SKYRIM_SE, 1, "Dragonborn")).unwrap();

        let saves = SKYRIM_SAVE_TRACKER.detect_saves(tmp.path()).unwrap();
        assert_eq!(saves.len(), 1);
        assert!(
            saves[0]
                .label
                .as_deref()
                .unwrap_or("")
                .contains("Dragonborn")
        );
    }

    #[test]
    fn detect_saves_skips_bak_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Save1.bak"), b"ignored").unwrap();
        std::fs::write(
            tmp.path().join("Save1.ess"),
            make_ess(MAGIC_SKYRIM_SE, 1, "Hero"),
        )
        .unwrap();

        let saves = SKYRIM_SAVE_TRACKER.detect_saves(tmp.path()).unwrap();
        // .bak must be skipped; only .ess counts
        assert_eq!(saves.len(), 1);
    }

    #[test]
    fn detect_saves_wrong_game_magic_still_captured() {
        let tmp = tempfile::tempdir().unwrap();
        // FO4 save read by Skyrim tracker — header parse will fail, but file is still captured
        let ess = tmp.path().join("Save1.ess");
        std::fs::write(&ess, make_ess(MAGIC_FALLOUT4, 5, "Sole")).unwrap();

        let saves = SKYRIM_SAVE_TRACKER.detect_saves(tmp.path()).unwrap();
        assert_eq!(
            saves.len(),
            1,
            "file should be captured even if magic mismatches"
        );
        assert!(
            saves[0].label.is_none(),
            "label should be None when header fails"
        );
    }

    #[test]
    fn detect_saves_sorted_newest_first() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 1..=3 {
            let ess = tmp.path().join(format!("Save{i}.ess"));
            std::fs::write(&ess, make_ess(MAGIC_SKYRIM_SE, i, "X")).unwrap();
            // Touch files with slightly different mtimes
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        let saves = SKYRIM_SAVE_TRACKER.detect_saves(tmp.path()).unwrap();
        assert_eq!(saves.len(), 3);
        // Newest first
        for i in 0..saves.len() - 1 {
            assert!(saves[i].modified >= saves[i + 1].modified);
        }
    }

    // ── describe_capture ─────────────────────────────────────────────

    #[test]
    fn describe_capture_no_saves() {
        let msg = SKYRIM_SAVE_TRACKER.describe_capture(&[]);
        assert!(msg.contains("no new saves"));
    }

    #[test]
    fn describe_capture_single_save() {
        let save = DetectedSave {
            rel_path: "Save1.ess".into(),
            category: Cow::Borrowed("manual"),
            label: Some("Lydia — Save 14".to_string()),
            modified: SystemTime::UNIX_EPOCH,
        };
        let msg = SKYRIM_SAVE_TRACKER.describe_capture(std::slice::from_ref(&save));
        assert!(msg.contains("Lydia — Save 14"));
        assert!(msg.contains("manual"));
    }

    #[test]
    fn describe_capture_multiple_saves_groups_by_character() {
        let saves: Vec<DetectedSave> = vec![
            DetectedSave {
                rel_path: "Save1.ess".into(),
                category: Cow::Borrowed("manual"),
                label: Some("Lydia — Save 14".to_string()),
                modified: SystemTime::UNIX_EPOCH,
            },
            DetectedSave {
                rel_path: "Save2.ess".into(),
                category: Cow::Borrowed("manual"),
                label: Some("Lydia — Save 15".to_string()),
                modified: SystemTime::UNIX_EPOCH,
            },
            DetectedSave {
                rel_path: "Save3.ess".into(),
                category: Cow::Borrowed("manual"),
                label: Some("Orc Mage — Save 7".to_string()),
                modified: SystemTime::UNIX_EPOCH,
            },
        ];

        let msg = SKYRIM_SAVE_TRACKER.describe_capture(&saves);
        assert!(msg.contains("3 saves"));
        assert!(msg.contains("Lydia"));
        assert!(msg.contains("Orc Mage"));
    }

    #[test]
    fn describe_capture_fo76_includes_warning() {
        let msg = FALLOUT76_SAVE_TRACKER.describe_capture(&[]);
        assert!(msg.contains("FO76"));
    }
}
