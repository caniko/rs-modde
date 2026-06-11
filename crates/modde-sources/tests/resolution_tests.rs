use modde_core::manifest::collection::{
    CollectionAuthor, CollectionGame, CollectionManifest, CollectionMod, CollectionVersion,
};
use modde_core::manifest::wabbajack::{ArchiveEntry, RawDirective, WabbajackManifest};
use modde_core::{NexusFileId, NexusModId, TransactionError};
use modde_sources::nexus::api::NexusApi;
use modde_sources::resolution::{
    preflight_collection_transaction, preflight_lazy_nexus_file_transaction,
    preflight_wabbajack_transaction,
};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn collection_with_mods(mods: Vec<CollectionMod>) -> CollectionManifest {
    CollectionManifest {
        slug: "test-collection".to_string(),
        name: "Test Collection".to_string(),
        summary: None,
        description: None,
        author: CollectionAuthor {
            name: "Author".to_string(),
            member_id: None,
        },
        game: CollectionGame {
            id: 1,
            domain_name: "skyrimspecialedition".to_string(),
            name: "Skyrim Special Edition".to_string(),
        },
        mods,
        version: CollectionVersion {
            version: "7".to_string(),
            created_at: None,
        },
        endorsements: 0,
        image_url: None,
    }
}

#[test]
fn collection_exact_files_solve_in_install_order() {
    let manifest = collection_with_mods(vec![
        CollectionMod {
            mod_id: NexusModId::from(2),
            file_id: NexusFileId::from(20),
            name: "Second".to_string(),
            version: "2.0".to_string(),
            optional: false,
            install_order: 2,
            patch: None,
        },
        CollectionMod {
            mod_id: NexusModId::from(1),
            file_id: NexusFileId::from(10),
            name: "First".to_string(),
            version: "1.0".to_string(),
            optional: true,
            install_order: 1,
            patch: None,
        },
    ]);

    let plan = preflight_collection_transaction(&manifest).unwrap();
    assert_eq!(plan.artifacts.len(), 2);
    assert_eq!(plan.artifacts[0].display_name, "First");
    assert!(!plan.artifacts[0].enabled_by_default);
    assert_eq!(plan.artifacts[1].display_name, "Second");
}

#[test]
fn collection_duplicate_mod_with_different_exact_files_is_unsat() {
    let manifest = collection_with_mods(vec![
        CollectionMod {
            mod_id: NexusModId::from(1),
            file_id: NexusFileId::from(10),
            name: "A".to_string(),
            version: "1.0".to_string(),
            optional: false,
            install_order: 1,
            patch: None,
        },
        CollectionMod {
            mod_id: NexusModId::from(1),
            file_id: NexusFileId::from(11),
            name: "A newer".to_string(),
            version: "2.0".to_string(),
            optional: false,
            install_order: 2,
            patch: None,
        },
    ]);

    let err = preflight_collection_transaction(&manifest).unwrap_err();
    assert!(matches!(err, TransactionError::Unsatisfiable { .. }));
}

#[test]
fn collection_optional_duplicate_mod_is_dropped_instead_of_unsat() {
    let manifest = collection_with_mods(vec![
        CollectionMod {
            mod_id: NexusModId::from(1),
            file_id: NexusFileId::from(10),
            name: "Required".to_string(),
            version: "1.0".to_string(),
            optional: false,
            install_order: 1,
            patch: None,
        },
        CollectionMod {
            mod_id: NexusModId::from(1),
            file_id: NexusFileId::from(11),
            name: "Optional alternate".to_string(),
            version: "2.0".to_string(),
            optional: true,
            install_order: 2,
            patch: None,
        },
    ]);

    let plan = preflight_collection_transaction(&manifest).unwrap();
    assert_eq!(plan.artifacts.len(), 1);
    assert_eq!(plan.artifacts[0].display_name, "Required");
}

#[test]
fn wabbajack_patch_output_requires_source_archive() {
    let manifest = WabbajackManifest {
        name: "Patch List".to_string(),
        author: "Author".to_string(),
        description: String::new(),
        game: "Skyrim Special Edition".to_string(),
        version: "1".to_string(),
        archives: vec![ArchiveEntry {
            hash: 1,
            name: "source.7z".to_string(),
            size: 10,
            state: None,
        }],
        directives: vec![
            RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::json!(1), serde_json::json!("file.txt")],
                to: "patched.txt".to_string(),
                hash: 2,
                patch_id: "patch-a".to_string(),
                size: 12,
            },
            RawDirective::CreateBSA {
                temp_id: "temp".to_string(),
                to: "archive.bsa".to_string(),
                file_states: Vec::new(),
            },
        ],
    };

    let plan = preflight_wabbajack_transaction(&manifest, "manifest-hash").unwrap();
    assert_eq!(plan.artifacts.len(), 3);
    assert!(
        plan.artifacts
            .iter()
            .any(|artifact| artifact.display_name == "patched.txt")
    );
}

#[test]
fn wabbajack_patch_missing_source_archive_is_unsat() {
    let manifest = WabbajackManifest {
        name: "Broken Patch List".to_string(),
        author: "Author".to_string(),
        description: String::new(),
        game: "Skyrim Special Edition".to_string(),
        version: "1".to_string(),
        archives: Vec::new(),
        directives: vec![RawDirective::PatchedFromArchive {
            archive_hash_path: vec![serde_json::json!(999_u64), serde_json::json!("file.txt")],
            to: "patched.txt".to_string(),
            hash: 2,
            patch_id: "patch-a".to_string(),
            size: 12,
        }],
    };

    let err = preflight_wabbajack_transaction(&manifest, "manifest-hash").unwrap_err();
    assert!(matches!(err, TransactionError::Unsatisfiable { .. }));
}

#[tokio::test(flavor = "multi_thread")]
async fn lazy_nexus_file_candidates_are_fetched_mid_solve() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/games/skyrimspecialedition/mods/42/files.json"))
        .and(header("apikey", "test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "files": [
                {
                    "file_id": 100,
                    "name": "Old file",
                    "version": "1.0",
                    "size_kb": 1,
                    "file_name": "old.zip",
                    "category_name": "OLD_VERSION",
                    "uploaded_timestamp": 1
                },
                {
                    "file_id": 101,
                    "name": "Main file",
                    "version": "2.0",
                    "size_kb": 1,
                    "file_name": "main.zip",
                    "category_name": "MAIN",
                    "uploaded_timestamp": 2
                }
            ]
        })))
        .mount(&server)
        .await;

    let api = NexusApi::new(reqwest::Client::new(), "test-key".to_string());
    // SAFETY: this test serializes access to the process environment with
    // ENV_LOCK and removes the variable before returning.
    unsafe {
        std::env::set_var("MODDE_NEXUS_BASE_URL", server.uri());
    }
    let plan = preflight_lazy_nexus_file_transaction(
        api,
        "skyrimspecialedition",
        NexusModId::from(42),
        NexusFileId::from(101),
    )
    .await
    .unwrap();
    // SAFETY: guarded by ENV_LOCK, matching the set above.
    unsafe {
        std::env::remove_var("MODDE_NEXUS_BASE_URL");
    }

    assert_eq!(plan.artifacts.len(), 1);
    assert_eq!(plan.artifacts[0].display_name, "Main file");
}

#[tokio::test(flavor = "multi_thread")]
async fn lazy_nexus_metadata_failure_is_explicit() {
    let _env_guard = ENV_LOCK.lock().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/games/skyrimspecialedition/mods/42/files.json"))
        .and(header("apikey", "test-key"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let api = NexusApi::new(reqwest::Client::new(), "test-key".to_string());
    unsafe {
        std::env::set_var("MODDE_NEXUS_BASE_URL", server.uri());
    }
    let err = preflight_lazy_nexus_file_transaction(
        api,
        "skyrimspecialedition",
        NexusModId::from(42),
        NexusFileId::from(101),
    )
    .await
    .unwrap_err();
    unsafe {
        std::env::remove_var("MODDE_NEXUS_BASE_URL");
    }

    assert!(matches!(err, TransactionError::MetadataFetch { .. }));
    assert!(err.to_string().contains("Nexus metadata unavailable"));
}
