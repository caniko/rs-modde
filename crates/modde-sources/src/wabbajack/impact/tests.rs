use super::*;
use modde_core::manifest::wabbajack::{
    ArchiveEntry, ArchiveState, RawDirective, WabbajackManifest,
};

fn manifest() -> WabbajackManifest {
    WabbajackManifest {
        name: "test".into(),
        author: "test".into(),
        description: "test".into(),
        game: "SkyrimSE".into(),
        version: "1".into(),
        archives: vec![
            ArchiveEntry {
                hash: 1,
                name: "missing.7z".into(),
                size: 10,
                state: Some(ArchiveState::ManualDownloader {
                    url: "https://example.test/file".into(),
                    prompt: String::new(),
                }),
            },
            ArchiveEntry {
                hash: 2,
                name: "present.7z".into(),
                size: 20,
                state: Some(ArchiveState::ManualDownloader {
                    url: "https://example.test/present".into(),
                    prompt: String::new(),
                }),
            },
        ],
        directives: vec![
            RawDirective::FromArchive {
                archive_hash_path: vec![
                    serde_json::Value::Number(1.into()),
                    serde_json::Value::String("a.txt".into()),
                ],
                to: "mods\\Missing Mod\\a.txt".into(),
                size: 5,
            },
            RawDirective::FromArchive {
                archive_hash_path: vec![
                    serde_json::Value::Number(1.into()),
                    serde_json::Value::String("b.txt".into()),
                ],
                to: "TEMP_BSA_FILES\\temp-a\\b.txt".into(),
                size: 6,
            },
            RawDirective::CreateBSA {
                temp_id: "temp-a".into(),
                to: "mods\\BSA Mod\\out.bsa".into(),
                file_states: vec![],
            },
            RawDirective::InlineFile {
                source_data_id: "inline".into(),
                hash: 9,
                size: 1,
                to: "mods\\Missing Mod\\inline.txt".into(),
            },
        ],
    }
}

#[test]
fn missing_impact_reports_manual_archives_and_direct_dependents() {
    let temp = tempfile::tempdir().unwrap();
    let impact = MissingArchiveImpact::analyze(&manifest(), temp.path());

    assert_eq!(impact.missing_archives.len(), 2);
    assert_eq!(impact.missing_archive_bytes, 30);
    assert_eq!(impact.blocked_archive_directives, 2);
    assert_eq!(impact.blocked_output_bytes, 11);
    assert!(
        impact
            .affected_mod_roots
            .iter()
            .any(|g| g.name == "Missing Mod")
    );
    assert_eq!(impact.affected_create_bsa.len(), 1);
    assert!(impact.omit_mod_roots.iter().any(|g| g.name == "BSA Mod"));
}

#[test]
fn omit_files_skips_missing_sources_and_downstream_bsa_only() {
    let temp = tempfile::tempdir().unwrap();
    let manifest = manifest();
    let impact = MissingArchiveImpact::analyze(&manifest, temp.path());
    let plan = impact.skip_plan(&manifest, MissingArchivePolicy::OmitFiles);

    assert!(plan.skipped_directives.contains(&0));
    assert!(plan.skipped_directives.contains(&1));
    assert!(plan.skipped_directives.contains(&2));
    assert!(!plan.skipped_directives.contains(&3));
}

#[test]
fn omit_mods_skips_whole_affected_mod_roots() {
    let temp = tempfile::tempdir().unwrap();
    let manifest = manifest();
    let impact = MissingArchiveImpact::analyze(&manifest, temp.path());
    let plan = impact.skip_plan(&manifest, MissingArchivePolicy::OmitMods);

    assert!(plan.skipped_directives.contains(&0));
    assert!(plan.skipped_directives.contains(&2));
    assert!(plan.skipped_directives.contains(&3));
    assert!(plan.skipped_mod_roots.contains("Missing Mod"));
    assert!(plan.skipped_mod_roots.contains("BSA Mod"));
}
