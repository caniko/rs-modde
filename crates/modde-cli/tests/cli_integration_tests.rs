//! Integration tests for CLI commands.
//!
//! Tests profile lifecycle, deploy pipeline with temp dirs,
//! and command argument validation.

use smallvec::smallvec;
use std::path::PathBuf;

use modde_core::GameId;
use modde_core::db::ModdeDb;
use modde_core::profile::{EnabledMod, Profile, ProfileManager, ProfileSource};
use modde_core::resolver::{resolve, LoadOrderRule, ModId};

// ── Profile lifecycle ──────────────────────────────────────────────

#[test]
fn test_profile_create_load_modify_save_load() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().unwrap());

    // Create
    let profile = Profile {
        id: None,
        name: "test_profile".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            EnabledMod {
                mod_id: "skyui".to_string(),
                enabled: true,
                version: Some("5.2".to_string()),
                fomod_config: None,
            },
            EnabledMod {
                mod_id: "ussep".to_string(),
                enabled: true,
                version: Some("4.2.8".to_string()),
                fomod_config: None,
            },
        ],
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec![LoadOrderRule::LoadAfter {
            mod_id: ModId::from("skyui"),
            after: ModId::from("ussep"),
        }],
    };

    mgr.create(&profile).unwrap();

    // Load and verify
    let loaded = mgr.load("test_profile", None).unwrap();
    assert_eq!(loaded.game_id, "skyrim-se");
    assert_eq!(loaded.mods.len(), 2);
    assert_eq!(loaded.load_order_rules.len(), 1);

    // Modify: disable a mod and add a new one
    let mut modified = loaded;
    modified.mods[0].enabled = false;
    modified.mods.push(EnabledMod {
        mod_id: "enb_helper".to_string(),
        enabled: true,
        version: Some("1.0".to_string()),
        fomod_config: None,
    });

    // Save modified profile (delete + recreate)
    mgr.delete("test_profile", None).unwrap();
    mgr.create(&modified).unwrap();

    // Load and verify modifications
    let reloaded = mgr.load("test_profile", None).unwrap();
    assert_eq!(reloaded.mods.len(), 3);
    assert!(!reloaded.mods[0].enabled);
    assert_eq!(reloaded.mods[2].mod_id, "enb_helper");
}

#[test]
fn test_profile_list_multiple() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().unwrap());

    let game_ids = ["skyrim-se", "fallout4", "cyberpunk2077"];
    for (i, game_id) in game_ids.iter().enumerate() {
        let profile = Profile {
            id: None,
            name: format!("profile_{i}"),
            game_id: GameId::from(*game_id),
            source: ProfileSource::Manual,
            mods: vec![],
            overrides: PathBuf::from("/tmp"),
            load_order_rules: smallvec![],
        };
        mgr.create(&profile).unwrap();
    }

    let list = mgr.list().unwrap();
    assert_eq!(list.len(), 3);
}

#[test]
fn test_profile_load_nonexistent_returns_error() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().unwrap());

    let result = mgr.load("does_not_exist", None);
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("not found"));
}

#[test]
fn test_profile_create_duplicate_returns_error() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().unwrap());

    let profile = Profile {
        id: None,
        name: "dupe".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
    };

    mgr.create(&profile).unwrap();
    let result = mgr.create(&profile);
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("already exists") || err.contains("UNIQUE constraint"));
}

#[test]
fn test_profile_delete_nonexistent_returns_error() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().unwrap());

    let result = mgr.delete("ghost", None);
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("not found"));
}

// ── Resolver integration ───────────────────────────────────────────

#[test]
fn test_resolve_complex_mod_dependency_chain() {
    let profile = Profile {
        id: None,
        name: "complex".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            EnabledMod {
                mod_id: "base".to_string(),
                enabled: true,
                version: Some("1.0".to_string()),
                fomod_config: None,
            },
            EnabledMod {
                mod_id: "framework".to_string(),
                enabled: true,
                version: None,
                fomod_config: None,
            },
            EnabledMod {
                mod_id: "visuals".to_string(),
                enabled: true,
                version: Some("2.0".to_string()),
                fomod_config: None,
            },
            EnabledMod {
                mod_id: "gameplay".to_string(),
                enabled: true,
                version: Some("3.0".to_string()),
                fomod_config: None,
            },
            EnabledMod {
                mod_id: "patch".to_string(),
                enabled: true,
                version: Some("1.1".to_string()),
                fomod_config: None,
            },
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![
            // base → framework → visuals
            // base → framework → gameplay
            // visuals + gameplay → patch
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("framework"),
                after: ModId::from("base"),
            },
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("visuals"),
                after: ModId::from("framework"),
            },
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("gameplay"),
                after: ModId::from("framework"),
            },
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("patch"),
                after: ModId::from("visuals"),
            },
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("patch"),
                after: ModId::from("gameplay"),
            },
        ],
    };

    let result = resolve(&profile).unwrap();
    assert_eq!(result.order.len(), 5);

    let pos = |name: &str| result.order.iter().position(|m| m == name).unwrap();
    assert!(pos("base") < pos("framework"));
    assert!(pos("framework") < pos("visuals"));
    assert!(pos("framework") < pos("gameplay"));
    assert!(pos("visuals") < pos("patch"));
    assert!(pos("gameplay") < pos("patch"));
}

#[test]
fn test_resolve_with_disabled_dependency() {
    // If a mod's dependency is disabled, the rule is ignored
    let profile = Profile {
        id: None,
        name: "disabled_dep".to_string(),
        game_id: GameId::from("fallout4"),
        source: ProfileSource::Manual,
        mods: vec![
            EnabledMod {
                mod_id: "base".to_string(),
                enabled: false, // disabled
                version: None,
                fomod_config: None,
            },
            EnabledMod {
                mod_id: "dependent".to_string(),
                enabled: true,
                version: None,
                fomod_config: None,
            },
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![LoadOrderRule::LoadAfter {
            mod_id: ModId::from("dependent"),
            after: ModId::from("base"),
        }],
    };

    let result = resolve(&profile).unwrap();
    assert_eq!(result.order.len(), 1);
    assert_eq!(result.order[0], "dependent");
}

// ── Deploy pipeline simulation ─────────────────────────────────────

#[tokio::test]
async fn test_deploy_pipeline_end_to_end() {
    use std::collections::HashMap;
    use modde_core::vfs::SymlinkFarm;

    let tmp = tempfile::TempDir::new().unwrap();

    // Create profile
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().unwrap());
    let profile = Profile {
        id: None,
        name: "e2e_test".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            EnabledMod {
                mod_id: "texture_mod".to_string(),
                enabled: true,
                version: Some("1.0".to_string()),
                fomod_config: None,
            },
            EnabledMod {
                mod_id: "mesh_mod".to_string(),
                enabled: true,
                version: Some("2.0".to_string()),
                fomod_config: None,
            },
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![LoadOrderRule::LoadAfter {
            mod_id: ModId::from("mesh_mod"),
            after: ModId::from("texture_mod"),
        }],
    };
    mgr.create(&profile).unwrap();

    // Resolve
    let resolved = resolve(&profile).unwrap();
    assert_eq!(resolved.order.len(), 2);

    // Create mod files
    let store = tmp.path().join("store");
    std::fs::create_dir_all(store.join("texture_mod/textures")).unwrap();
    std::fs::write(store.join("texture_mod/textures/sky.dds"), "sky texture").unwrap();
    std::fs::create_dir_all(store.join("mesh_mod/meshes")).unwrap();
    std::fs::write(store.join("mesh_mod/meshes/tree.nif"), "tree mesh").unwrap();
    // Both provide a shared file
    std::fs::write(store.join("texture_mod/textures/shared.dds"), "from texture_mod").unwrap();
    std::fs::create_dir_all(store.join("mesh_mod/textures")).unwrap();
    std::fs::write(store.join("mesh_mod/textures/shared.dds"), "from mesh_mod").unwrap();

    // Build mod_files map
    let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
    mod_files.insert(
        ModId::from("texture_mod"),
        vec![
            (
                "textures/sky.dds".to_string(),
                store.join("texture_mod/textures/sky.dds"),
            ),
            (
                "textures/shared.dds".to_string(),
                store.join("texture_mod/textures/shared.dds"),
            ),
        ],
    );
    mod_files.insert(
        ModId::from("mesh_mod"),
        vec![
            (
                "meshes/tree.nif".to_string(),
                store.join("mesh_mod/meshes/tree.nif"),
            ),
            (
                "textures/shared.dds".to_string(),
                store.join("mesh_mod/textures/shared.dds"),
            ),
        ],
    );

    // Build symlink farm
    let farm = SymlinkFarm::build("e2e_test", &resolved, &mod_files, None).unwrap();

    // mesh_mod loads after texture_mod, so it wins for shared.dds
    assert_eq!(
        farm.links.get("textures/shared.dds").unwrap(),
        &store.join("mesh_mod/textures/shared.dds")
    );

    // Materialize
    let farm = farm.materialize().await.unwrap();

    // Deploy to target
    let target = tmp.path().join("game/Data");
    farm.deploy_to(&target).await.unwrap();

    // Verify symlinks
    assert!(target.join("textures/sky.dds").is_symlink());
    assert!(target.join("meshes/tree.nif").is_symlink());
    assert!(target.join("textures/shared.dds").is_symlink());

    // Content through symlink chain should be readable
    assert_eq!(
        std::fs::read_to_string(target.join("textures/sky.dds")).unwrap(),
        "sky texture"
    );
    assert_eq!(
        std::fs::read_to_string(target.join("textures/shared.dds")).unwrap(),
        "from mesh_mod"
    );
}

// ── Error handling ─────────────────────────────────────────────────

#[test]
fn test_core_error_display_messages() {
    use modde_core::error::CoreError;

    let errors = vec![
        (
            CoreError::HashMismatch {
                path: PathBuf::from("/file.txt"),
                expected: "aaa".to_string(),
                actual: "bbb".to_string(),
            },
            "hash mismatch",
        ),
        (
            CoreError::DependencyCycle("mod_a".to_string()),
            "dependency cycle",
        ),
        (
            CoreError::FileConflict {
                path: "shared.dds".to_string(),
                mods: Box::new(smallvec::smallvec!["a".to_string(), "b".to_string()]),
            },
            "conflict",
        ),
        (
            CoreError::ProfileNotFound("missing".to_string()),
            "not found",
        ),
        (
            CoreError::ProfileAlreadyExists("dupe".to_string()),
            "already exists",
        ),
        (
            CoreError::GameNotDetected("unknown".to_string()),
            "not detected",
        ),
        (
            CoreError::UnsupportedFs("btrfs".into()),
            "unsupported",
        ),
        (CoreError::Other("custom error".into()), "custom error"),
    ];

    for (error, expected_substring) in errors {
        let msg = format!("{error}");
        assert!(
            msg.contains(expected_substring),
            "Error '{msg}' should contain '{expected_substring}'"
        );
    }
}

// ── Profile with Wabbajack source ──────────────────────────────────

#[test]
fn test_profile_wabbajack_source_roundtrip() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().unwrap());

    let profile = Profile {
        id: None,
        name: "wj_list".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Wabbajack {
            manifest_hash: "abcdef0123456789".to_string(),
        },
        mods: vec![
            EnabledMod {
                mod_id: "wj_mod_1".to_string(),
                enabled: true,
                version: Some("1.0".to_string()),
                fomod_config: Some(r#"{"steps":[]}"#.to_string()),
            },
            EnabledMod {
                mod_id: "wj_mod_2".to_string(),
                enabled: false,
                version: None,
                fomod_config: None,
            },
        ],
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec![
            LoadOrderRule::LoadBefore {
                mod_id: ModId::from("wj_mod_1"),
                before: ModId::from("wj_mod_2"),
            },
        ],
    };

    mgr.create(&profile).unwrap();
    let loaded = mgr.load("wj_list", None).unwrap();

    match &loaded.source {
        ProfileSource::Wabbajack { manifest_hash } => {
            assert_eq!(manifest_hash, "abcdef0123456789");
        }
        _ => panic!("expected Wabbajack source"),
    }

    assert_eq!(loaded.mods.len(), 2);
    assert_eq!(
        loaded.mods[0].fomod_config.as_deref(),
        Some(r#"{"steps":[]}"#)
    );
    assert!(loaded.mods[1].fomod_config.is_none());
}

// ── Profile with NexusCollection source ────────────────────────────

#[test]
fn test_profile_nexus_collection_source_roundtrip() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().unwrap());

    let profile = Profile {
        id: None,
        name: "nexus_col".to_string(),
        game_id: GameId::from("fallout4"),
        source: ProfileSource::NexusCollection {
            slug: "ultimate-fallout".to_string(),
            version: "2.1.0".to_string(),
        },
        mods: (0..20)
            .map(|i| EnabledMod {
                mod_id: format!("mod_{i}"),
                enabled: i % 2 == 0,
                version: Some(format!("{}.0", i)),
                fomod_config: None,
            })
            .collect(),
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
    };

    mgr.create(&profile).unwrap();
    let loaded = mgr.load("nexus_col", None).unwrap();
    assert_eq!(loaded.mods.len(), 20);

    match &loaded.source {
        ProfileSource::NexusCollection { slug, version } => {
            assert_eq!(slug, "ultimate-fallout");
            assert_eq!(version, "2.1.0");
        }
        _ => panic!("expected NexusCollection source"),
    }
}
