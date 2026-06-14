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
