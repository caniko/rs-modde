use modde_core::manifest::collection::*;
use modde_core::manifest::wabbajack::*;

// ── WabbajackManifest tests ──────────────────────────────────────────

fn minimal_manifest() -> WabbajackManifest {
    WabbajackManifest {
        name: "Test".to_string(),
        author: "Author".to_string(),
        description: "Desc".to_string(),
        game: "SkyrimSE".to_string(),
        version: "1.0".to_string(),
        archives: vec![],
        directives: vec![],
    }
}

#[test]
fn test_download_directives_no_archives() {
    let manifest = minimal_manifest();
    assert!(manifest.download_directives().is_empty());
}

#[test]
fn test_download_directives_archive_without_state() {
    let mut manifest = minimal_manifest();
    manifest.archives.push(ArchiveEntry {
        hash: 123,
        name: "test.7z".to_string(),
        size: 1000,
        state: None,
    });
    assert!(manifest.download_directives().is_empty());
}

#[test]
fn test_download_directives_all_types() {
    let mut manifest = minimal_manifest();
    manifest.archives = vec![
        ArchiveEntry {
            hash: 1,
            name: "nexus.7z".to_string(),
            size: 100,
            state: Some(ArchiveState::NexusDownloader {
                game_name: "SkyrimSE".to_string(),
                mod_id: 42.into(),
                file_id: 99.into(),
            }),
        },
        ArchiveEntry {
            hash: 2,
            name: "gh.zip".to_string(),
            size: 200,
            state: Some(ArchiveState::GitHubDownloader {
                user: "user".to_string(),
                repo: "repo".to_string(),
                tag: "v1.0".to_string(),
                asset: "release.zip".to_string(),
            }),
        },
        ArchiveEntry {
            hash: 3,
            name: "gdrive.zip".to_string(),
            size: 300,
            state: Some(ArchiveState::GoogleDriveDownloader {
                id: "abc123".to_string(),
            }),
        },
        ArchiveEntry {
            hash: 4,
            name: "mega.zip".to_string(),
            size: 400,
            state: Some(ArchiveState::MegaDownloader {
                url: "https://mega.nz/file/abc".to_string(),
            }),
        },
        ArchiveEntry {
            hash: 5,
            name: "direct.zip".to_string(),
            size: 500,
            state: Some(ArchiveState::HttpDownloader {
                url: "https://example.com/file.zip".to_string(),
                headers: Default::default(),
            }),
        },
    ];

    let directives = manifest.download_directives();
    assert_eq!(directives.len(), 5);

    // Verify each type via pattern matching
    assert!(matches!(
        &directives[0],
        DownloadDirective::Nexus { mod_id, .. } if mod_id.get() == 42
    ));
    assert!(matches!(&directives[1], DownloadDirective::GitHub { user, .. } if user == "user"));
    assert!(matches!(&directives[2], DownloadDirective::GoogleDrive { id, .. } if id == "abc123"));
    assert!(
        matches!(&directives[3], DownloadDirective::Mega { url, .. } if url.contains("mega.nz"))
    );
    assert!(matches!(
        &directives[4],
        DownloadDirective::DirectURL { .. }
    ));
}

#[test]
fn test_install_directives_unknown_filtered() {
    let mut manifest = minimal_manifest();
    // We need to deserialize an Unknown variant. Since it's `#[serde(other)]`,
    // we construct directives via JSON.
    let json = r#"[
        {"$type": "SomeRandomType, Wabbajack.Lib", "Foo": "bar"},
        {"$type": "FromArchive, Wabbajack.Lib", "ArchiveHashPath": [42, "data/test.esp"], "To": "output/test.esp"}
    ]"#;
    manifest.directives = serde_json::from_str(json).unwrap();

    let install = manifest.install_directives();
    assert_eq!(install.len(), 1);
    assert!(matches!(
        &install[0],
        InstallDirective::FromArchive {
            archive_hash: 42,
            ..
        }
    ));
}

#[test]
fn test_install_directives_from_archive_empty_hash_path() {
    let mut manifest = minimal_manifest();
    let json = r#"[
        {"$type": "FromArchive, Wabbajack.Lib", "ArchiveHashPath": [], "To": "output/test.esp"}
    ]"#;
    manifest.directives = serde_json::from_str(json).unwrap();

    let install = manifest.install_directives();
    assert_eq!(install.len(), 1);
    match &install[0] {
        InstallDirective::FromArchive {
            archive_hash, from, ..
        } => {
            assert_eq!(*archive_hash, 0);
            assert_eq!(from, "");
        }
        _ => panic!("expected FromArchive"),
    }
}

#[test]
fn test_install_directives_create_bsa() {
    let mut manifest = minimal_manifest();
    let json = r#"[
        {
            "$type": "CreateBSA, Wabbajack.Lib",
            "TempID": "tmp_001",
            "To": "output/test.bsa",
            "FileStates": [
                {"Path": "textures/sky.dds", "Hash": 111, "Size": 2048},
                {"Path": "meshes/tree.nif", "Hash": 222, "Size": 4096}
            ]
        }
    ]"#;
    manifest.directives = serde_json::from_str(json).unwrap();

    let install = manifest.install_directives();
    assert_eq!(install.len(), 1);
    match &install[0] {
        InstallDirective::CreateBSA {
            temp_id,
            to,
            file_states,
        } => {
            assert_eq!(temp_id, "tmp_001");
            assert_eq!(to, "output/test.bsa");
            assert_eq!(file_states.len(), 2);
            assert_eq!(file_states[0].path, "textures/sky.dds");
            assert_eq!(file_states[1].hash, 222);
        }
        _ => panic!("expected CreateBSA"),
    }
}

#[test]
fn test_roundtrip_serialization() {
    let mut manifest = minimal_manifest();
    manifest.archives.push(ArchiveEntry {
        hash: 42,
        name: "test.7z".to_string(),
        size: 1000,
        state: Some(ArchiveState::NexusDownloader {
            game_name: "SkyrimSE".to_string(),
            mod_id: 10.into(),
            file_id: 20.into(),
        }),
    });

    let json = serde_json::to_string(&manifest).unwrap();
    let deserialized: WabbajackManifest = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.name, manifest.name);
    assert_eq!(deserialized.archives.len(), 1);
    assert_eq!(deserialized.archives[0].hash, 42);
    assert!(deserialized.archives[0].state.is_some());
}

// ── CollectionManifest tests ─────────────────────────────────────────

fn minimal_collection_json() -> &'static str {
    r#"{
        "slug": "test-collection",
        "name": "Test Collection",
        "summary": null,
        "description": null,
        "author": {"name": "tester", "member_id": null},
        "game": {"id": 1704, "domain_name": "skyrimspecialedition", "name": "Skyrim SE"},
        "mods": [],
        "version": {"version": "1.0.0", "created_at": null},
        "endorsements": 0,
        "image_url": null
    }"#
}

#[test]
fn test_collection_optional_fields() {
    let collection: CollectionManifest = serde_json::from_str(minimal_collection_json()).unwrap();
    assert!(collection.summary.is_none());
    assert!(collection.description.is_none());
    assert!(collection.image_url.is_none());

    // Now with values set
    let json = r#"{
        "slug": "test",
        "name": "Test",
        "summary": "A summary",
        "description": "A description",
        "author": {"name": "tester", "member_id": 42},
        "game": {"id": 1704, "domain_name": "skyrimspecialedition", "name": "Skyrim SE"},
        "mods": [],
        "version": {"version": "1.0.0", "created_at": "2025-01-01"},
        "endorsements": 10,
        "image_url": "https://example.com/img.jpg"
    }"#;
    let collection: CollectionManifest = serde_json::from_str(json).unwrap();
    assert_eq!(collection.summary.as_deref(), Some("A summary"));
    assert_eq!(collection.description.as_deref(), Some("A description"));
    assert_eq!(
        collection.image_url.as_deref(),
        Some("https://example.com/img.jpg")
    );
}

#[test]
fn test_collection_empty_mods() {
    let collection: CollectionManifest = serde_json::from_str(minimal_collection_json()).unwrap();
    assert!(collection.mods.is_empty());
}

#[test]
fn test_collection_mod_optional_patch() {
    let json = r#"{
        "slug": "test",
        "name": "Test",
        "summary": null,
        "description": null,
        "author": {"name": "tester", "member_id": null},
        "game": {"id": 1704, "domain_name": "skyrimspecialedition", "name": "Skyrim SE"},
        "mods": [
            {
                "mod_id": 100,
                "file_id": 200,
                "name": "Cool Mod",
                "version": "1.0",
                "optional": false,
                "install_order": 0,
                "patch": null
            },
            {
                "mod_id": 101,
                "file_id": 201,
                "name": "Patched Mod",
                "version": "2.0",
                "optional": true,
                "install_order": 1,
                "patch": {"hash": "abc123", "url": "https://example.com/patch.bin"}
            }
        ],
        "version": {"version": "1.0.0", "created_at": null},
        "endorsements": 0,
        "image_url": null
    }"#;

    let collection: CollectionManifest = serde_json::from_str(json).unwrap();
    assert_eq!(collection.mods.len(), 2);
    assert!(collection.mods[0].patch.is_none());
    let patch = collection.mods[1].patch.as_ref().unwrap();
    assert_eq!(patch.hash, "abc123");
    assert_eq!(patch.url, "https://example.com/patch.bin");
}
