//! Tests for SaveFingerprint, FingerprintCheck, and SaveSnapshot compatibility.

use modde_core::profile::EnabledMod;
use modde_core::save::{FingerprintCheck, SaveFingerprint, SaveSnapshot};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn mod_entry(id: &str, enabled: bool) -> EnabledMod {
    EnabledMod {
        mod_id: id.to_string(),
        enabled,
        version: Some("1.0".to_string()),
        fomod_config: None, ..Default::default()
    }
}

/// Classifier that treats mod IDs starting with "script_" as save-breaking.
fn script_classifier(id: &str) -> bool {
    id.starts_with("script_")
}

fn make_snapshot(fp: Option<SaveFingerprint>) -> SaveSnapshot {
    SaveSnapshot {
        id: "abc123def456".to_string(),
        message: "test commit".to_string(),
        timestamp: 0,
        file_count: 1,
        fingerprint: fp,
    }
}

// ── SaveFingerprint::compute ──────────────────────────────────────────────────

#[test]
fn fingerprint_compute_empty_mods() {
    let fp = SaveFingerprint::compute(&[], script_classifier);
    assert!(fp.is_empty());
    assert!(fp.mod_ids.is_empty());
}

#[test]
fn fingerprint_compute_no_breaking_mods() {
    let mods = vec![
        mod_entry("texture_hd", true),
        mod_entry("ui_overhaul", true),
    ];
    let fp = SaveFingerprint::compute(&mods, script_classifier);
    assert!(fp.is_empty());
}

#[test]
fn fingerprint_compute_single_breaking_mod() {
    let mods = vec![
        mod_entry("script_main_quest", true),
        mod_entry("texture_hd", true),
    ];
    let fp = SaveFingerprint::compute(&mods, script_classifier);
    assert!(!fp.is_empty());
    assert_eq!(fp.mod_ids.len(), 1);
    assert!(fp.mod_ids.contains(&"script_main_quest".to_string()));
}

#[test]
fn fingerprint_compute_multiple_breaking_mods() {
    let mods = vec![
        mod_entry("script_a", true),
        mod_entry("script_b", true),
        mod_entry("texture_x", true),
    ];
    let fp = SaveFingerprint::compute(&mods, script_classifier);
    assert_eq!(fp.mod_ids.len(), 2);
    assert!(fp.mod_ids.contains(&"script_a".to_string()));
    assert!(fp.mod_ids.contains(&"script_b".to_string()));
}

#[test]
fn fingerprint_compute_disabled_mods_excluded() {
    let mods = vec![
        mod_entry("script_heavy_quest", false), // disabled — must be excluded
        mod_entry("script_minor_tweak", true),
    ];
    let fp = SaveFingerprint::compute(&mods, script_classifier);
    assert_eq!(fp.mod_ids.len(), 1);
    assert!(fp.mod_ids.contains(&"script_minor_tweak".to_string()));
    assert!(!fp.mod_ids.contains(&"script_heavy_quest".to_string()));
}

#[test]
fn fingerprint_compute_is_deterministic() {
    let mods = vec![
        mod_entry("script_z", true),
        mod_entry("script_a", true),
        mod_entry("script_m", true),
    ];
    let fp1 = SaveFingerprint::compute(&mods, script_classifier);
    let fp2 = SaveFingerprint::compute(&mods, script_classifier);
    assert_eq!(fp1.hash, fp2.hash);
}

#[test]
fn fingerprint_compute_order_independent() {
    // mod IDs are sorted before hashing — insertion order must not matter
    let mods_a = vec![
        mod_entry("script_alpha", true),
        mod_entry("script_beta", true),
    ];
    let mods_b = vec![
        mod_entry("script_beta", true),
        mod_entry("script_alpha", true),
    ];
    let fp_a = SaveFingerprint::compute(&mods_a, script_classifier);
    let fp_b = SaveFingerprint::compute(&mods_b, script_classifier);
    assert_eq!(fp_a.hash, fp_b.hash);
}

#[test]
fn fingerprint_compute_different_mods_different_hash() {
    let mods_a = vec![mod_entry("script_quest_main", true)];
    let mods_b = vec![mod_entry("script_quest_side", true)];
    let fp_a = SaveFingerprint::compute(&mods_a, script_classifier);
    let fp_b = SaveFingerprint::compute(&mods_b, script_classifier);
    assert_ne!(fp_a.hash, fp_b.hash);
}

// ── SaveFingerprint::empty ────────────────────────────────────────────────────

#[test]
fn fingerprint_empty_is_empty() {
    let fp = SaveFingerprint::empty();
    assert!(fp.is_empty());
    assert_eq!(fp.hash.len(), 64);
    assert!(fp.hash.chars().all(|c| c == '0'));
}

// ── SaveFingerprint::short_hash ───────────────────────────────────────────────

#[test]
fn fingerprint_short_hash_is_12_chars() {
    let mods = vec![mod_entry("script_x", true)];
    let fp = SaveFingerprint::compute(&mods, script_classifier);
    assert_eq!(fp.short_hash().len(), 12);
}

#[test]
fn fingerprint_short_hash_is_prefix_of_full_hash() {
    let mods = vec![mod_entry("script_x", true)];
    let fp = SaveFingerprint::compute(&mods, script_classifier);
    assert!(fp.hash.starts_with(fp.short_hash()));
}

// ── FingerprintCheck ──────────────────────────────────────────────────────────

#[test]
fn fingerprint_check_no_snapshot_fingerprint() {
    let snapshot = make_snapshot(None);
    let current = SaveFingerprint::compute(&[mod_entry("script_a", true)], script_classifier);
    let check = snapshot.check_compatibility(&current);
    assert!(matches!(check, FingerprintCheck::NoFingerprint));
    // NoFingerprint is treated as compatible (pre-fingerprint saves are allowed)
    assert!(check.is_compatible());
}

#[test]
fn fingerprint_check_identical_fingerprints() {
    let mods = vec![mod_entry("script_a", true), mod_entry("script_b", true)];
    let fp = SaveFingerprint::compute(&mods, script_classifier);
    let snapshot = make_snapshot(Some(fp.clone()));
    let check = snapshot.check_compatibility(&fp);
    assert!(matches!(check, FingerprintCheck::Compatible));
    assert!(check.is_compatible());
}

#[test]
fn fingerprint_check_mismatch_mod_removed() {
    let mods_then = vec![
        mod_entry("script_heavy_quest", true),
        mod_entry("script_tweak", true),
    ];
    let mods_now = vec![mod_entry("script_tweak", true)];

    let fp_then = SaveFingerprint::compute(&mods_then, script_classifier);
    let fp_now = SaveFingerprint::compute(&mods_now, script_classifier);

    let snapshot = make_snapshot(Some(fp_then));
    let check = snapshot.check_compatibility(&fp_now);

    match check {
        FingerprintCheck::Mismatch { removed, added } => {
            assert!(removed.contains(&"script_heavy_quest".to_string()));
            assert!(added.is_empty());
        }
        other => panic!("expected Mismatch, got {:?}", other),
    }
}

#[test]
fn fingerprint_check_mismatch_mod_added() {
    let mods_then = vec![mod_entry("script_tweak", true)];
    let mods_now = vec![
        mod_entry("script_tweak", true),
        mod_entry("script_new_quest", true),
    ];

    let fp_then = SaveFingerprint::compute(&mods_then, script_classifier);
    let fp_now = SaveFingerprint::compute(&mods_now, script_classifier);

    let snapshot = make_snapshot(Some(fp_then));
    let check = snapshot.check_compatibility(&fp_now);

    match check {
        FingerprintCheck::Mismatch { removed, added } => {
            assert!(removed.is_empty());
            assert!(added.contains(&"script_new_quest".to_string()));
        }
        other => panic!("expected Mismatch, got {:?}", other),
    }
}

#[test]
fn fingerprint_check_mismatch_both_removed_and_added() {
    let mods_then = vec![
        mod_entry("script_old_quest", true),
        mod_entry("script_shared", true),
    ];
    let mods_now = vec![
        mod_entry("script_shared", true),
        mod_entry("script_new_quest", true),
    ];

    let fp_then = SaveFingerprint::compute(&mods_then, script_classifier);
    let fp_now = SaveFingerprint::compute(&mods_now, script_classifier);

    let snapshot = make_snapshot(Some(fp_then));
    let check = snapshot.check_compatibility(&fp_now);

    match check {
        FingerprintCheck::Mismatch { removed, added } => {
            assert!(removed.contains(&"script_old_quest".to_string()));
            assert!(added.contains(&"script_new_quest".to_string()));
        }
        other => panic!("expected Mismatch, got {:?}", other),
    }
}

#[test]
fn fingerprint_check_is_compatible_semantics() {
    assert!(FingerprintCheck::Compatible.is_compatible());
    assert!(FingerprintCheck::NoFingerprint.is_compatible());

    // Generate a real Mismatch via check_compatibility
    let fp_a = SaveFingerprint::compute(&[mod_entry("script_x", true)], script_classifier);
    let fp_b = SaveFingerprint::compute(&[mod_entry("script_y", true)], script_classifier);
    let snapshot = make_snapshot(Some(fp_a));
    let mismatch = snapshot.check_compatibility(&fp_b);
    assert!(!mismatch.is_compatible());
}

// ── Cosmetic-only profile produces empty fingerprint ─────────────────────────

#[test]
fn cosmetic_only_profile_empty_fingerprint() {
    let mods = vec![
        mod_entry("texture_hd_terrain", true),
        mod_entry("ui_font_overhaul", true),
        mod_entry("lore_friendly_armor", true),
    ];
    let fp = SaveFingerprint::compute(&mods, |_| false);
    assert!(fp.is_empty());
}

// ── SaveSnapshot::short_id ────────────────────────────────────────────────────

#[test]
fn snapshot_short_id_is_8_chars() {
    let snapshot = make_snapshot(None);
    assert_eq!(snapshot.short_id().len(), 8);
    assert!(snapshot.id.starts_with(snapshot.short_id()));
}
