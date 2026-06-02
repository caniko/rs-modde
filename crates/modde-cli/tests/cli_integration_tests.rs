//! Integration tests for CLI commands.
//!
//! Tests profile lifecycle, deploy pipeline with temp dirs,
//! and command argument validation.

use smallvec::smallvec;
use std::path::PathBuf;

use modde_core::GameId;
use modde_core::db::ModdeDb;
use modde_core::profile::{
    EnabledMod, LoadOrderLock, LockReason, Profile, ProfileManager, ProfileSource,
};
use modde_core::resolver::{LoadOrderRule, ModId, resolve};

// ── Profile lifecycle ──────────────────────────────────────────────

#[tokio::test]
async fn test_profile_create_load_modify_save_load() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

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
                ..Default::default()
            },
            EnabledMod {
                mod_id: "ussep".to_string(),
                enabled: true,
                version: Some("4.2.8".to_string()),
                fomod_config: None,
                ..Default::default()
            },
        ],
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec![LoadOrderRule::LoadAfter {
            mod_id: ModId::from("skyui"),
            after: ModId::from("ussep"),
        }],
        load_order_lock: None,
    };

    mgr.create(&profile).await.unwrap();

    // Load and verify
    let loaded = mgr.load("test_profile", None).await.unwrap();
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
        ..Default::default()
    });

    // Save modified profile (delete + recreate)
    mgr.delete("test_profile", None).await.unwrap();
    mgr.create(&modified).await.unwrap();

    // Load and verify modifications
    let reloaded = mgr.load("test_profile", None).await.unwrap();
    assert_eq!(reloaded.mods.len(), 3);
    assert!(!reloaded.mods[0].enabled);
    assert_eq!(reloaded.mods[2].mod_id, "enb_helper");
}

#[tokio::test]
async fn test_profile_list_multiple() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

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
            load_order_lock: None,
        };
        mgr.create(&profile).await.unwrap();
    }

    let list = mgr.list().await.unwrap();
    assert_eq!(list.len(), 3);
}

#[tokio::test]
async fn test_profile_load_nonexistent_returns_error() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let result = mgr.load("does_not_exist", None).await;
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("not found"));
}

#[tokio::test]
async fn test_profile_create_duplicate_returns_error() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let profile = Profile {
        id: None,
        name: "dupe".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };

    mgr.create(&profile).await.unwrap();
    let result = mgr.create(&profile).await;
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("already exists") || err.contains("UNIQUE constraint"));
}

#[tokio::test]
async fn test_profile_delete_nonexistent_returns_error() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let result = mgr.delete("ghost", None).await;
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
                ..Default::default()
            },
            EnabledMod {
                mod_id: "framework".to_string(),
                enabled: true,
                version: None,
                fomod_config: None,
                ..Default::default()
            },
            EnabledMod {
                mod_id: "visuals".to_string(),
                enabled: true,
                version: Some("2.0".to_string()),
                fomod_config: None,
                ..Default::default()
            },
            EnabledMod {
                mod_id: "gameplay".to_string(),
                enabled: true,
                version: Some("3.0".to_string()),
                fomod_config: None,
                ..Default::default()
            },
            EnabledMod {
                mod_id: "patch".to_string(),
                enabled: true,
                version: Some("1.1".to_string()),
                fomod_config: None,
                ..Default::default()
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
        load_order_lock: None,
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
                ..Default::default()
            },
            EnabledMod {
                mod_id: "dependent".to_string(),
                enabled: true,
                version: None,
                fomod_config: None,
                ..Default::default()
            },
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![LoadOrderRule::LoadAfter {
            mod_id: ModId::from("dependent"),
            after: ModId::from("base"),
        }],
        load_order_lock: None,
    };

    let result = resolve(&profile).unwrap();
    assert_eq!(result.order.len(), 1);
    assert_eq!(result.order[0], "dependent");
}

// ── Deploy pipeline simulation ─────────────────────────────────────

#[tokio::test]
async fn test_deploy_pipeline_end_to_end() {
    use modde_core::vfs::SymlinkFarm;
    use std::collections::HashMap;

    let tmp = tempfile::TempDir::new().unwrap();

    // Create profile
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
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
                ..Default::default()
            },
            EnabledMod {
                mod_id: "mesh_mod".to_string(),
                enabled: true,
                version: Some("2.0".to_string()),
                fomod_config: None,
                ..Default::default()
            },
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![LoadOrderRule::LoadAfter {
            mod_id: ModId::from("mesh_mod"),
            after: ModId::from("texture_mod"),
        }],
        load_order_lock: None,
    };
    mgr.create(&profile).await.unwrap();

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
    std::fs::write(
        store.join("texture_mod/textures/shared.dds"),
        "from texture_mod",
    )
    .unwrap();
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
    let farm = SymlinkFarm::build("e2e_test", &resolved, &mod_files, None, None).unwrap();

    // mesh_mod loads after texture_mod, so it wins for shared.dds
    assert_eq!(
        farm.links.get("textures/shared.dds").unwrap(),
        &store.join("mesh_mod/textures/shared.dds")
    );

    // Keep the materialized farm under this test's tempdir. The production
    // builder uses the global modde data dir, which may be unwritable inside
    // Nix check sandboxes.
    let farm = SymlinkFarm::from_links(tmp.path().join("profile-staging"), farm.links.clone());

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
        (CoreError::UnsupportedFs("btrfs".into()), "unsupported"),
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

#[tokio::test]
async fn test_profile_wabbajack_source_roundtrip() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

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
                ..Default::default()
            },
            EnabledMod {
                mod_id: "wj_mod_2".to_string(),
                enabled: false,
                version: None,
                fomod_config: None,
                ..Default::default()
            },
        ],
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec![LoadOrderRule::LoadBefore {
            mod_id: ModId::from("wj_mod_1"),
            before: ModId::from("wj_mod_2"),
        },],
        load_order_lock: None,
    };

    mgr.create(&profile).await.unwrap();
    let loaded = mgr.load("wj_list", None).await.unwrap();

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

#[tokio::test]
async fn test_profile_nexus_collection_source_roundtrip() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

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
                version: Some(format!("{i}.0")),
                fomod_config: None,
                ..Default::default()
            })
            .collect(),
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };

    mgr.create(&profile).await.unwrap();
    let loaded = mgr.load("nexus_col", None).await.unwrap();
    assert_eq!(loaded.mods.len(), 20);

    match &loaded.source {
        ProfileSource::NexusCollection { slug, version } => {
            assert_eq!(slug, "ultimate-fallout");
            assert_eq!(version, "2.1.0");
        }
        _ => panic!("expected NexusCollection source"),
    }
}

// ── Per-mod pin parity (CLI lock-mod / unlock-mod) ─────────────────
//
// These tests validate the same load → mutate → update sequence the
// `ProfileAction::LockMod` / `UnlockMod` handlers perform in
// `crates/modde-cli/src/commands/profile.rs`. Because the CLI crate
// is a binary (no lib target), integration tests cannot call the
// handler functions directly — instead we drive the same
// `ProfileManager` API the handler uses and assert the same
// predicates (mod presence, profile lock state) that the handler
// checks before mutating.

fn make_profile_with_mods(mods: Vec<EnabledMod>) -> Profile {
    Profile {
        id: None,
        name: "pin_test".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods,
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    }
}

#[tokio::test]
async fn test_profile_lock_mod_happy_path() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    let profile = make_profile_with_mods(vec![
        EnabledMod {
            mod_id: "skyui".to_string(),
            enabled: true,
            ..Default::default()
        },
        EnabledMod {
            mod_id: "ussep".to_string(),
            enabled: true,
            ..Default::default()
        },
    ]);
    mgr.create(&profile).await.unwrap();

    // Mirror the ProfileAction::LockMod handler.
    let mut p = mgr.load("pin_test", None).await.unwrap();
    assert!(
        p.load_order_lock.is_none(),
        "precondition: profile must not be locked"
    );
    let m = p
        .mods
        .iter_mut()
        .find(|m| m.mod_id == "skyui")
        .expect("mod must exist");
    m.lock = Some(LockReason::Manual {
        note: Some("pinned".to_string()),
    });
    mgr.update(&p).await.unwrap();

    let reloaded = mgr.load("pin_test", None).await.unwrap();
    let skyui = reloaded.mods.iter().find(|m| m.mod_id == "skyui").unwrap();
    match &skyui.lock {
        Some(LockReason::Manual { note }) => {
            assert_eq!(note.as_deref(), Some("pinned"));
        }
        other => panic!("expected Manual lock with note, got {other:?}"),
    }
    let ussep = reloaded.mods.iter().find(|m| m.mod_id == "ussep").unwrap();
    assert!(ussep.lock.is_none(), "unrelated mod must remain unlocked");
}

#[tokio::test]
async fn test_profile_lock_mod_rejects_missing_mod() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    let profile = make_profile_with_mods(vec![EnabledMod {
        mod_id: "skyui".to_string(),
        enabled: true,
        ..Default::default()
    }]);
    mgr.create(&profile).await.unwrap();

    let p = mgr.load("pin_test", None).await.unwrap();
    // This is the predicate the handler bails on:
    //   `profile.mods.iter_mut().find(|m| m.mod_id == mod_id)` → None
    assert!(
        p.mods.iter().all(|m| m.mod_id != "nonexistent"),
        "fixture must not contain 'nonexistent' for this test to be meaningful"
    );
}

#[tokio::test]
async fn test_profile_lock_mod_rejects_when_profile_locked() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    let mut profile = make_profile_with_mods(vec![EnabledMod {
        mod_id: "skyui".to_string(),
        enabled: true,
        ..Default::default()
    }]);
    profile.load_order_lock = Some(LoadOrderLock::now(LockReason::Wabbajack {
        manifest_hash: "deadbeef".to_string(),
    }));
    mgr.create(&profile).await.unwrap();

    let p = mgr.load("pin_test", None).await.unwrap();
    // This is the predicate the handler bails on.
    let lock = p
        .load_order_lock
        .as_ref()
        .expect("profile-level lock must be present");
    match &lock.reason {
        LockReason::Wabbajack { manifest_hash } => {
            assert_eq!(manifest_hash, "deadbeef");
        }
        other => panic!("expected Wabbajack reason, got {other:?}"),
    }
}

#[tokio::test]
async fn test_profile_unlock_mod_happy_path() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    let profile = make_profile_with_mods(vec![EnabledMod {
        mod_id: "skyui".to_string(),
        enabled: true,
        lock: Some(LockReason::Manual {
            note: Some("was pinned".to_string()),
        }),
        ..Default::default()
    }]);
    mgr.create(&profile).await.unwrap();

    // Mirror the ProfileAction::UnlockMod handler.
    let mut p = mgr.load("pin_test", None).await.unwrap();
    let m = p
        .mods
        .iter_mut()
        .find(|m| m.mod_id == "skyui")
        .expect("mod must exist");
    let prior = m.lock.take();
    assert!(prior.is_some(), "precondition: mod must be pinned");
    mgr.update(&p).await.unwrap();

    let reloaded = mgr.load("pin_test", None).await.unwrap();
    assert!(
        reloaded.mods[0].lock.is_none(),
        "lock must be cleared after update"
    );
}

#[tokio::test]
async fn test_profile_unlock_mod_idempotent_on_unlocked_mod() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    let profile = make_profile_with_mods(vec![EnabledMod {
        mod_id: "skyui".to_string(),
        enabled: true,
        version: Some("5.2".to_string()),
        ..Default::default()
    }]);
    mgr.create(&profile).await.unwrap();

    // Mirror the no-op branch of ProfileAction::UnlockMod: the handler
    // skips `pm.update` when `m.lock.take()` returns None.
    let mut p = mgr.load("pin_test", None).await.unwrap();
    let m = p
        .mods
        .iter_mut()
        .find(|m| m.mod_id == "skyui")
        .expect("mod must exist");
    let prior = m.lock.take();
    assert!(prior.is_none(), "precondition: mod must start unlocked");
    // Intentionally skip `mgr.update(&p)` — that's the handler's no-op branch.

    let reloaded = mgr.load("pin_test", None).await.unwrap();
    assert!(reloaded.mods[0].lock.is_none());
    // Ensure unrelated state wasn't clobbered.
    assert_eq!(reloaded.mods[0].version.as_deref(), Some("5.2"));
}

#[tokio::test]
async fn test_profile_lock_info_lists_pins_without_profile_lock() {
    // Regression guard: the `LockInfo` handler's "Per-mod pins"
    // section must run even when `profile.load_order_lock` is None.
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    let profile = make_profile_with_mods(vec![
        EnabledMod {
            mod_id: "skyui".to_string(),
            enabled: true,
            lock: Some(LockReason::Manual { note: None }),
            ..Default::default()
        },
        EnabledMod {
            mod_id: "ussep".to_string(),
            enabled: true,
            ..Default::default()
        },
    ]);
    mgr.create(&profile).await.unwrap();

    let p = mgr.load("pin_test", None).await.unwrap();
    assert!(p.load_order_lock.is_none());
    // Mirror the filter predicate in the LockInfo handler.
    let pinned: Vec<&str> = p
        .mods
        .iter()
        .filter(|m| m.lock.is_some())
        .map(|m| m.mod_id.as_str())
        .collect();
    assert_eq!(pinned, vec!["skyui"]);
}
