//! Advanced end-to-end tests for modde-core.
//!
//! These tests exercise multi-mod pipelines, conflict chains, rollback,
//! profile management, and hash verification flows.

use smallvec::{smallvec, SmallVec};
use std::collections::HashMap;
use std::path::PathBuf;


use modde_core::GameId;
use modde_core::error::CoreError;
use modde_core::hash::{hash_file_sha256, hash_file_xxhash, verify_sha256, verify_xxhash};
use modde_core::manifest::collection::CollectionManifest;
use modde_core::manifest::wabbajack::WabbajackManifest;
use modde_core::profile::{EnabledMod, Profile, ProfileManager, ProfileSource};
use modde_core::resolver::{resolve, ConflictMap, LoadOrderRule, ModId, ResolvedLoadOrder};
use modde_core::vfs::{Built, SymlinkFarm};
use modde_core::ModdeDb;
use tempfile::TempDir;

// ── Helpers ──────────────────────────────────────────────────────────

fn simple_mod(id: &str, enabled: bool) -> EnabledMod {
    EnabledMod {
        mod_id: id.to_string(),
        enabled,
        version: Some("1.0".to_string()),
        fomod_config: None, ..Default::default()
    }
}

fn make_mod_files(
    tmp: &std::path::Path,
    mod_id: &str,
    files: &[(&str, &str)],
) -> Vec<(String, PathBuf)> {
    let mod_dir = tmp.join("store").join(mod_id);
    std::fs::create_dir_all(&mod_dir).unwrap();
    files
        .iter()
        .map(|(rel_path, content)| {
            let file_path = mod_dir.join(rel_path);
            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&file_path, content).unwrap();
            (rel_path.to_string(), file_path)
        })
        .collect()
}

fn build_farm_from_resolved(
    resolved: &ResolvedLoadOrder,
    mod_files: &HashMap<ModId, Vec<(String, PathBuf)>>,
    staging_dir: PathBuf,
) -> SymlinkFarm<Built> {
    let mut links = HashMap::new();
    for mod_id in &resolved.order {
        if let Some(files) = mod_files.get(mod_id) {
            for (rel_path, source) in files {
                links.insert(rel_path.clone(), source.clone());
            }
        }
    }
    SymlinkFarm::from_links(staging_dir, links)
}

// ── 1. Full pipeline with 20+ mods ──────────────────────────────────

#[tokio::test]
async fn e2e_full_pipeline_20_mods_complex_order() {
    let tmp = TempDir::new().unwrap();

    // Generate 22 mods with a chain of LoadAfter rules
    let mod_ids: Vec<String> = (0..22).map(|i| format!("mod_{i:02}")).collect();
    let mods: Vec<EnabledMod> = mod_ids.iter().map(|id| simple_mod(id, true)).collect();

    // Chain: mod_00 < mod_01 < mod_02 < ... < mod_21
    let rules: SmallVec<[LoadOrderRule; 4]> = (1..22)
        .map(|i| LoadOrderRule::LoadAfter {
            mod_id: ModId::from(format!("mod_{i:02}").as_str()),
            after: ModId::from(format!("mod_{:02}", i - 1).as_str()),
        })
        .collect();

    let profile = Profile {
        id: None,
        name: "big_pipeline".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods,
        overrides: PathBuf::from("/tmp"),
        load_order_rules: rules,
    };

    // Resolve
    let resolved = resolve(&profile).unwrap();
    assert_eq!(resolved.order.len(), 22);

    // Verify the chain ordering
    for i in 1..22 {
        let pos_prev = resolved
            .order
            .iter()
            .position(|m| m == format!("mod_{:02}", i - 1).as_str())
            .unwrap();
        let pos_curr = resolved
            .order
            .iter()
            .position(|m| m == format!("mod_{i:02}").as_str())
            .unwrap();
        assert!(
            pos_prev < pos_curr,
            "mod_{:02} should come before mod_{i:02}",
            i - 1
        );
    }

    // Create mod files: each mod provides a unique file + some shared conflicts
    let mut all_mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
    for (i, mod_id) in mod_ids.iter().enumerate() {
        let unique_file = format!("unique_{i:02}.esp");
        let mut files_spec: Vec<(&str, &str)> = Vec::new();
        // We need owned strings for the unique file path
        let unique_content = format!("content_{i:02}");
        // Use a leaked string to satisfy lifetime - acceptable in tests
        let unique_ref: &'static str = Box::leak(unique_file.into_boxed_str());
        let content_ref: &'static str = Box::leak(unique_content.into_boxed_str());
        files_spec.push((unique_ref, content_ref));

        // Every mod provides "shared_texture.dds" (conflict chain)
        files_spec.push(("textures/shared_texture.dds", content_ref));

        all_mod_files.insert(
            ModId::from(mod_id.as_str()),
            make_mod_files(tmp.path(), mod_id, &files_spec),
        );
    }

    // Build conflict map
    let mut conflict_map = ConflictMap::default();
    for (mod_id, files) in &all_mod_files {
        for (rel_path, _) in files {
            conflict_map.register(rel_path.clone(), mod_id.clone());
        }
    }
    let conflicts = conflict_map.conflicts();
    // shared_texture.dds should have 22 providers
    let shared_conflict = conflicts
        .iter()
        .find(|(p, _)| *p == "textures/shared_texture.dds")
        .unwrap();
    assert_eq!(shared_conflict.1.len(), 22);

    // Build farm - last mod wins for shared file
    let farm = build_farm_from_resolved(
        &resolved,
        &all_mod_files,
        tmp.path().join("staging"),
    );

    // 22 unique files + 1 shared file = 23 links
    assert_eq!(farm.links.len(), 23);

    // The shared texture should come from the last mod in the chain
    let last_mod_id = resolved.order.last().unwrap();
    let shared_source = farm.links.get("textures/shared_texture.dds").unwrap();
    assert!(
        shared_source.to_string_lossy().contains(last_mod_id.as_str()),
        "shared texture should be from {last_mod_id}, got {shared_source:?}"
    );

    // Materialize and deploy
    let farm = farm.materialize().await.unwrap();
    let game_dir = tmp.path().join("game/Data");
    farm.deploy_to(&game_dir).await.unwrap();

    // Verify all unique files deployed
    for i in 0..22 {
        let deployed = game_dir.join(format!("unique_{i:02}.esp"));
        assert!(
            deployed.symlink_metadata().unwrap().file_type().is_symlink(),
            "unique_{i:02}.esp should be a symlink"
        );
    }
}

// ── 2. Wabbajack source profile pipeline ─────────────────────────────

#[tokio::test]
async fn e2e_wabbajack_source_resolve_deploy_verify() {
    let tmp = TempDir::new().unwrap();

    // Simulate a Wabbajack-sourced profile
    let profile = Profile {
        id: None,
        name: "wj_profile".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Wabbajack {
            manifest_hash: "abc123".to_string(),
        },
        mods: vec![
            simple_mod("skse", true),
            simple_mod("unofficial_patch", true),
            simple_mod("enb_helper", true),
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("unofficial_patch"),
                after: ModId::from("skse"),
            },
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("enb_helper"),
                after: ModId::from("unofficial_patch"),
            },
        ],
    };

    // Save and reload via DB
    let pm = ProfileManager::with_db(ModdeDb::open_memory().unwrap());
    pm.create(&profile).unwrap();
    let loaded = pm.load("wj_profile", None).unwrap();
    match &loaded.source {
        ProfileSource::Wabbajack { manifest_hash } => {
            assert_eq!(manifest_hash, "abc123");
        }
        _ => panic!("expected Wabbajack source"),
    }

    // Resolve
    let resolved = resolve(&loaded).unwrap();
    assert_eq!(resolved.order.len(), 3);

    let pos_skse = resolved.order.iter().position(|m| m == "skse").unwrap();
    let pos_up = resolved
        .order
        .iter()
        .position(|m| m == "unofficial_patch")
        .unwrap();
    let pos_enb = resolved.order.iter().position(|m| m == "enb_helper").unwrap();
    assert!(pos_skse < pos_up);
    assert!(pos_up < pos_enb);

    // Create mod files
    let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
    mod_files.insert(
        ModId::from("skse"),
        make_mod_files(tmp.path(), "skse", &[("skse64_loader.exe", "exe_data")]),
    );
    mod_files.insert(
        ModId::from("unofficial_patch"),
        make_mod_files(
            tmp.path(),
            "unofficial_patch",
            &[("Unofficial Skyrim SE Patch.esp", "patch_data")],
        ),
    );
    mod_files.insert(
        ModId::from("enb_helper"),
        make_mod_files(
            tmp.path(),
            "enb_helper",
            &[("Data/SKSE/Plugins/ENBHelperSE.dll", "dll_data")],
        ),
    );

    let farm = build_farm_from_resolved(&resolved, &mod_files, tmp.path().join("staging"));
    let farm = farm.materialize().await.unwrap();

    let game_dir = tmp.path().join("game");
    farm.deploy_to(&game_dir).await.unwrap();

    // Verify symlinks
    assert!(game_dir.join("skse64_loader.exe").symlink_metadata().unwrap().file_type().is_symlink());
    assert!(game_dir
        .join("Unofficial Skyrim SE Patch.esp")
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(game_dir
        .join("Data/SKSE/Plugins/ENBHelperSE.dll")
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());
}

// ── 3. NexusCollection source profile pipeline ──────────────────────

#[tokio::test]
async fn e2e_nexus_collection_source_resolve_deploy_verify() {
    let tmp = TempDir::new().unwrap();

    let profile = Profile {
        id: None,
        name: "nexus_profile".to_string(),
        game_id: GameId::from("fallout4"),
        source: ProfileSource::NexusCollection {
            slug: "ultimate-fallout".to_string(),
            version: "4.0".to_string(),
        },
        mods: vec![
            simple_mod("f4se", true),
            simple_mod("body_mod", true),
            simple_mod("weather_mod", true),
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![LoadOrderRule::LoadBefore {
            mod_id: ModId::from("f4se"),
            before: ModId::from("body_mod"),
        }],
    };

    let resolved = resolve(&profile).unwrap();
    assert_eq!(resolved.order.len(), 3);

    let pos_f4se = resolved.order.iter().position(|m| m == "f4se").unwrap();
    let pos_body = resolved.order.iter().position(|m| m == "body_mod").unwrap();
    assert!(pos_f4se < pos_body);

    let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
    mod_files.insert(
        ModId::from("f4se"),
        make_mod_files(tmp.path(), "f4se", &[("f4se_loader.exe", "exe")]),
    );
    mod_files.insert(
        ModId::from("body_mod"),
        make_mod_files(
            tmp.path(),
            "body_mod",
            &[("meshes/body.nif", "mesh_data")],
        ),
    );
    mod_files.insert(
        ModId::from("weather_mod"),
        make_mod_files(
            tmp.path(),
            "weather_mod",
            &[("weather.esp", "plugin_data")],
        ),
    );

    let farm = build_farm_from_resolved(&resolved, &mod_files, tmp.path().join("staging"));
    let farm = farm.materialize().await.unwrap();

    let game_dir = tmp.path().join("game");
    farm.deploy_to(&game_dir).await.unwrap();

    assert_eq!(
        std::fs::read_to_string(game_dir.join("f4se_loader.exe")).unwrap(),
        "exe"
    );
    assert_eq!(
        std::fs::read_to_string(game_dir.join("meshes/body.nif")).unwrap(),
        "mesh_data"
    );
    assert_eq!(
        std::fs::read_to_string(game_dir.join("weather.esp")).unwrap(),
        "plugin_data"
    );
}

// ── 4. Deploy then rollback ──────────────────────────────────────────

#[tokio::test]
async fn e2e_deploy_then_rollback_verify() {
    // We simulate rollback by creating staging and staging.bak manually,
    // since the real rollback function reads from XDG_DATA_HOME.
    let tmp = TempDir::new().unwrap();

    // Version 1 files
    let mut mod_files_v1: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
    mod_files_v1.insert(
        ModId::from("mod_a"),
        make_mod_files(tmp.path(), "mod_a_v1", &[("data.esp", "version_1")]),
    );

    let resolved = ResolvedLoadOrder {
        order: vec![ModId::from("mod_a")],
    };

    let farm_v1 = build_farm_from_resolved(
        &resolved,
        &mod_files_v1,
        tmp.path().join("staging_v1"),
    );
    let farm_v1 = farm_v1.materialize().await.unwrap();

    let game_dir = tmp.path().join("game");
    farm_v1.deploy_to(&game_dir).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(game_dir.join("data.esp")).unwrap(),
        "version_1"
    );

    // Version 2 files
    let mut mod_files_v2: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
    mod_files_v2.insert(
        ModId::from("mod_a"),
        make_mod_files(tmp.path(), "mod_a_v2", &[("data.esp", "version_2")]),
    );

    let farm_v2 = build_farm_from_resolved(
        &resolved,
        &mod_files_v2,
        tmp.path().join("staging_v2"),
    );
    let farm_v2 = farm_v2.materialize().await.unwrap();

    farm_v2.deploy_to(&game_dir).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(game_dir.join("data.esp")).unwrap(),
        "version_2"
    );

    // Simulate rollback: re-deploy v1
    farm_v1.deploy_to(&game_dir).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(game_dir.join("data.esp")).unwrap(),
        "version_1"
    );
}

// ── 5. Multiple profiles for same game ───────────────────────────────

#[test]
fn e2e_multiple_profiles_same_game() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().unwrap());

    let p1 = Profile {
        id: None,
        name: "vanilla_plus".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            simple_mod("unofficial_patch", true),
            simple_mod("skyui", true),
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
    };

    let p2 = Profile {
        id: None,
        name: "heavy_modded".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            simple_mod("unofficial_patch", true),
            simple_mod("skyui", true),
            simple_mod("enb_series", true),
            simple_mod("texture_pack", true),
            simple_mod("weather_mod", true),
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
    };

    let p3 = Profile {
        id: None,
        name: "minimalist".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![simple_mod("skyui", true)],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
    };

    mgr.create(&p1).unwrap();
    mgr.create(&p2).unwrap();
    mgr.create(&p3).unwrap();

    let mut names: Vec<String> = mgr.list().unwrap().into_iter().map(|s| s.name).collect();
    names.sort();
    assert_eq!(names, vec!["heavy_modded", "minimalist", "vanilla_plus"]);

    // Each profile resolves independently
    let r1 = resolve(&mgr.load("vanilla_plus", None).unwrap()).unwrap();
    let r2 = resolve(&mgr.load("heavy_modded", None).unwrap()).unwrap();
    let r3 = resolve(&mgr.load("minimalist", None).unwrap()).unwrap();
    assert_eq!(r1.order.len(), 2);
    assert_eq!(r2.order.len(), 5);
    assert_eq!(r3.order.len(), 1);
}

// ── 6. Profile with disabled mods mixed in ───────────────────────────

#[test]
fn e2e_disabled_mods_excluded_from_resolution() {
    let profile = Profile {
        id: None,
        name: "mixed".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            simple_mod("always_on", true),
            EnabledMod {
                mod_id: "disabled_1".to_string(),
                enabled: false,
                version: Some("1.0".to_string()),
                fomod_config: None, ..Default::default()
            },
            simple_mod("also_on", true),
            EnabledMod {
                mod_id: "disabled_2".to_string(),
                enabled: false,
                version: Some("2.0".to_string()),
                fomod_config: None, ..Default::default()
            },
            EnabledMod {
                mod_id: "disabled_3".to_string(),
                enabled: false,
                version: None,
                fomod_config: None, ..Default::default()
            },
            simple_mod("last_enabled", true),
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
    };

    let resolved = resolve(&profile).unwrap();
    assert_eq!(resolved.order.len(), 3);
    assert!(resolved.order.contains(&ModId::from("always_on")));
    assert!(resolved.order.contains(&ModId::from("also_on")));
    assert!(resolved.order.contains(&ModId::from("last_enabled")));
    assert!(!resolved.order.contains(&ModId::from("disabled_1")));
    assert!(!resolved.order.contains(&ModId::from("disabled_2")));
    assert!(!resolved.order.contains(&ModId::from("disabled_3")));
}

// ── 7. Conflict detection with 3+ mods ──────────────────────────────

#[test]
fn e2e_conflict_detection_three_plus_mods() {
    let mut cm = ConflictMap::default();

    // 5 mods all providing the same texture
    for i in 0..5 {
        cm.register("textures/body/skin.dds".to_string(), ModId::from(format!("mod_{i}").as_str()));
    }

    // 3 mods providing another shared file
    for i in 0..3 {
        cm.register("meshes/body/body.nif".to_string(), ModId::from(format!("mod_{i}").as_str()));
    }

    // 1 mod with a unique file (no conflict)
    cm.register("scripts/custom.pex".to_string(), ModId::from("mod_0"));

    let conflicts = cm.conflicts();
    assert_eq!(conflicts.len(), 2);

    let skin_conflict = conflicts
        .iter()
        .find(|(p, _)| *p == "textures/body/skin.dds")
        .unwrap();
    assert_eq!(skin_conflict.1.len(), 5);

    let body_conflict = conflicts
        .iter()
        .find(|(p, _)| *p == "meshes/body/body.nif")
        .unwrap();
    assert_eq!(body_conflict.1.len(), 3);
}

// ── 8. Load order with circular dependency ───────────────────────────

#[test]
fn e2e_circular_dependency_detected() {
    // Simple cycle: A -> B -> A
    let profile = Profile {
        id: None,
        name: "cycle2".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![simple_mod("mod_a", true), simple_mod("mod_b", true)],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_b"),
                after: ModId::from("mod_a"),
            },
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_a"),
                after: ModId::from("mod_b"),
            },
        ],
    };

    let result = resolve(&profile);
    assert!(result.is_err());
    let err = result.unwrap_err();
    match &err {
        CoreError::DependencyCycle(mod_id) => {
            assert!(
                mod_id == "mod_a" || mod_id == "mod_b",
                "cycle should mention one of the involved mods, got: {mod_id}"
            );
        }
        _ => panic!("expected DependencyCycle, got: {err}"),
    }
}

#[test]
fn e2e_circular_dependency_three_way() {
    // A -> B -> C -> A
    let profile = Profile {
        id: None,
        name: "cycle3".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            simple_mod("mod_a", true),
            simple_mod("mod_b", true),
            simple_mod("mod_c", true),
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_b"),
                after: ModId::from("mod_a"),
            },
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_c"),
                after: ModId::from("mod_b"),
            },
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_a"),
                after: ModId::from("mod_c"),
            },
        ],
    };

    let result = resolve(&profile);
    assert!(result.is_err());
    match result.unwrap_err() {
        CoreError::DependencyCycle(_) => {} // expected
        other => panic!("expected DependencyCycle, got: {other}"),
    }
}

// ── 9. Profile save/modify/save/load roundtrip ──────────────────────

#[test]
fn e2e_profile_save_modify_save_load() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().unwrap());

    // Initial profile
    let mut profile = Profile {
        id: None,
        name: "evolving".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![simple_mod("mod_a", true), simple_mod("mod_b", true)],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
    };
    let id = pm.create(&profile).unwrap();
    profile.id = Some(id);

    // Modify: add a mod and a rule
    profile
        .mods
        .push(simple_mod("mod_c", true));
    profile
        .load_order_rules
        .push(LoadOrderRule::LoadAfter {
            mod_id: ModId::from("mod_c"),
            after: ModId::from("mod_a"),
        });
    pm.update(&profile).unwrap();

    // Modify again: disable mod_b
    profile.mods[1].enabled = false;
    pm.update(&profile).unwrap();

    // Load and verify we get the latest state
    let loaded = pm.load("evolving", None).unwrap();
    assert_eq!(loaded.mods.len(), 3);
    assert!(loaded.mods[0].enabled); // mod_a
    assert!(!loaded.mods[1].enabled); // mod_b disabled
    assert!(loaded.mods[2].enabled); // mod_c
    assert_eq!(loaded.load_order_rules.len(), 1);

    // Resolve with the final state
    let resolved = resolve(&loaded).unwrap();
    assert_eq!(resolved.order.len(), 2); // mod_a and mod_c (mod_b disabled)
    assert!(!resolved.order.contains(&ModId::from("mod_b")));

    let pos_a = resolved.order.iter().position(|m| m == "mod_a").unwrap();
    let pos_c = resolved.order.iter().position(|m| m == "mod_c").unwrap();
    assert!(pos_a < pos_c);
}

// ── 10. Hash verification pipeline ──────────────────────────────────

#[tokio::test]
async fn e2e_hash_verification_pipeline() {
    let tmp = TempDir::new().unwrap();

    // Create files
    let file_a = tmp.path().join("file_a.esp");
    let file_b = tmp.path().join("file_b.dds");
    let file_c = tmp.path().join("file_c.nif");

    std::fs::write(&file_a, "plugin data alpha").unwrap();
    std::fs::write(&file_b, "texture data bravo").unwrap();
    std::fs::write(&file_c, "mesh data charlie").unwrap();

    // Hash all files with both algorithms
    let hash_a_xx = hash_file_xxhash(&file_a).await.unwrap();
    let hash_b_xx = hash_file_xxhash(&file_b).await.unwrap();
    let hash_c_xx = hash_file_xxhash(&file_c).await.unwrap();

    let hash_a_sha = hash_file_sha256(&file_a).await.unwrap();
    let hash_b_sha = hash_file_sha256(&file_b).await.unwrap();
    let hash_c_sha = hash_file_sha256(&file_c).await.unwrap();

    // All hashes should be unique
    assert_ne!(hash_a_xx, hash_b_xx);
    assert_ne!(hash_b_xx, hash_c_xx);
    assert_ne!(hash_a_sha, hash_b_sha);
    assert_ne!(hash_b_sha, hash_c_sha);

    // Verify all pass
    verify_xxhash(&file_a, hash_a_xx).await.unwrap();
    verify_xxhash(&file_b, hash_b_xx).await.unwrap();
    verify_xxhash(&file_c, hash_c_xx).await.unwrap();
    verify_sha256(&file_a, &hash_a_sha).await.unwrap();
    verify_sha256(&file_b, &hash_b_sha).await.unwrap();
    verify_sha256(&file_c, &hash_c_sha).await.unwrap();

    // Modify file_b
    std::fs::write(&file_b, "MODIFIED texture data").unwrap();

    // file_b should now fail verification
    let result_xx = verify_xxhash(&file_b, hash_b_xx).await;
    assert!(result_xx.is_err());
    match result_xx.unwrap_err() {
        CoreError::HashMismatch {
            path,
            expected,
            actual,
        } => {
            assert_eq!(path, file_b);
            assert_ne!(expected, actual);
        }
        other => panic!("expected HashMismatch, got: {other}"),
    }

    let result_sha = verify_sha256(&file_b, &hash_b_sha).await;
    assert!(result_sha.is_err());

    // Unmodified files should still verify fine
    verify_xxhash(&file_a, hash_a_xx).await.unwrap();
    verify_xxhash(&file_c, hash_c_xx).await.unwrap();
    verify_sha256(&file_a, &hash_a_sha).await.unwrap();
    verify_sha256(&file_c, &hash_c_sha).await.unwrap();
}

// ── Extra: WabbajackManifest with archives and directives ────────────

#[test]
fn e2e_wabbajack_manifest_download_directives() {
    let json = r#"{
        "Name": "TestList",
        "Author": "tester",
        "Description": "desc",
        "Game": "SkyrimSE",
        "Version": "1.0",
        "Archives": [
            {
                "Hash": 111,
                "Name": "nexus_mod.7z",
                "Size": 1000,
                "State": {
                    "$type": "NexusDownloader, Wabbajack.Lib",
                    "GameName": "SkyrimSpecialEdition",
                    "ModID": 42,
                    "FileID": 99
                }
            },
            {
                "Hash": 222,
                "Name": "github_mod.zip",
                "Size": 2000,
                "State": {
                    "$type": "GitHubDownloader, Wabbajack.Lib",
                    "User": "test_user",
                    "Repo": "test_repo",
                    "Tag": "v1.0",
                    "Asset": "release.zip"
                }
            },
            {
                "Hash": 333,
                "Name": "direct.zip",
                "Size": 500
            }
        ],
        "Directives": [
            {
                "$type": "FromArchive, Wabbajack.Lib",
                "ArchiveHashPath": [111, "inner/file.esp"],
                "To": "Data/file.esp"
            }
        ]
    }"#;

    let manifest: WabbajackManifest = serde_json::from_str(json).unwrap();
    assert_eq!(manifest.archives.len(), 3);

    let downloads = manifest.download_directives();
    // Only archives with State produce download directives
    assert_eq!(downloads.len(), 2);

    let installs = manifest.install_directives();
    assert_eq!(installs.len(), 1);
}

// ── Extra: CollectionManifest with full mod list ─────────────────────

#[test]
fn e2e_collection_manifest_with_mods_and_patches() {
    let json = r#"{
        "slug": "test-collection",
        "name": "Test Collection",
        "author": { "name": "Author", "member_id": 1 },
        "game": { "id": 1704, "domain_name": "skyrimse", "name": "Skyrim SE" },
        "mods": [
            {
                "mod_id": 100,
                "file_id": 200,
                "name": "Mod A",
                "version": "1.0",
                "optional": false,
                "install_order": 1,
                "patch": {
                    "hash": "abc123",
                    "url": "https://example.com/patch.bin"
                }
            },
            {
                "mod_id": 101,
                "file_id": 201,
                "name": "Mod B",
                "version": "2.0",
                "optional": true,
                "install_order": 2
            }
        ],
        "version": { "version": "1.0", "created_at": "2025-06-15" },
        "endorsements": 42,
        "image_url": "https://example.com/image.png"
    }"#;

    let manifest: CollectionManifest = serde_json::from_str(json).unwrap();
    assert_eq!(manifest.mods.len(), 2);
    assert!(manifest.mods[0].patch.is_some());
    assert!(manifest.mods[1].patch.is_none());
    assert_eq!(manifest.mods[0].patch.as_ref().unwrap().hash, "abc123");
    assert_eq!(manifest.endorsements, 42);
    assert_eq!(manifest.image_url, Some("https://example.com/image.png".to_string()));
}

// ── Extra: Incompatible mods error ───────────────────────────────────

#[test]
fn e2e_incompatible_mods_both_enabled() {
    let profile = Profile {
        id: None,
        name: "incompat".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            simple_mod("enb", true),
            simple_mod("reshade", true),
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![LoadOrderRule::Incompatible {
            mod_a: ModId::from("enb"),
            mod_b: ModId::from("reshade"),
        }],
    };

    let result = resolve(&profile);
    assert!(result.is_err());
}

#[test]
fn e2e_incompatible_mods_one_disabled_is_ok() {
    let profile = Profile {
        id: None,
        name: "incompat_ok".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            simple_mod("enb", true),
            EnabledMod {
                mod_id: "reshade".to_string(),
                enabled: false,
                version: None,
                fomod_config: None, ..Default::default()
            },
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![LoadOrderRule::Incompatible {
            mod_a: ModId::from("enb"),
            mod_b: ModId::from("reshade"),
        }],
    };

    let resolved = resolve(&profile).unwrap();
    assert_eq!(resolved.order.len(), 1);
    assert_eq!(resolved.order[0], "enb");
}
