mod common;

use common::Fixture;
use modde_core::db::ModdeDb;
use modde_core::installer::{InstallMethod, InstallPlan, InstallStatus, StagedFile};
use modde_core::lockfile::ModdeLock;
use modde_core::profile::{EnabledMod, Profile, ProfileManager, ProfileSource};
use modde_core::resolver::{GameId, ModId};

#[test]
fn lock_export_sign_verify_round_trips_empty_profile() {
    let fx = Fixture::new();
    let lock = fx.root().join("modde.lock");
    let public_key = fx.root().join("modde-lock.pub.json");
    let secret_key = fx.root().join("modde-lock.secret.json");

    let create = fx
        .cmd()
        .args(["profile", "create", "portable", "--game", "skyrim-se"])
        .output()
        .expect("profile create");
    assert!(
        create.status.success(),
        "profile create failed: {}",
        String::from_utf8_lossy(&create.stderr)
    );

    let export = fx
        .cmd()
        .args([
            "lock",
            "export",
            "--profile",
            "portable",
            "--game",
            "skyrim-se",
            "--output",
            lock.to_str().unwrap(),
        ])
        .output()
        .expect("lock export");
    assert!(
        export.status.success(),
        "lock export failed: {}",
        String::from_utf8_lossy(&export.stderr)
    );
    let exported = std::fs::read_to_string(&lock).unwrap();
    assert!(exported.contains("\"kind\": \"modde.lock\""));
    assert!(exported.contains("\"format_version\": 1"));

    let keygen = fx
        .cmd()
        .args([
            "lock",
            "keygen",
            "--public",
            public_key.to_str().unwrap(),
            "--secret",
            secret_key.to_str().unwrap(),
        ])
        .output()
        .expect("lock keygen");
    assert!(
        keygen.status.success(),
        "lock keygen failed: {}",
        String::from_utf8_lossy(&keygen.stderr)
    );

    let sign = fx
        .cmd()
        .args([
            "lock",
            "sign",
            lock.to_str().unwrap(),
            "--secret-key",
            secret_key.to_str().unwrap(),
        ])
        .output()
        .expect("lock sign");
    assert!(
        sign.status.success(),
        "lock sign failed: {}",
        String::from_utf8_lossy(&sign.stderr)
    );

    let verify = fx
        .cmd()
        .args([
            "lock",
            "verify",
            lock.to_str().unwrap(),
            "--profile",
            "portable",
            "--game",
            "skyrim-se",
        ])
        .output()
        .expect("lock verify");
    assert!(
        verify.status.success(),
        "lock verify failed: {}",
        String::from_utf8_lossy(&verify.stderr)
    );
    let stdout = String::from_utf8_lossy(&verify.stdout);
    assert!(
        stdout.contains("Verified 1 signature"),
        "verify output missing signature count:\n{stdout}"
    );
}

const MOD_ID: &str = "test-mod";
const REL_PATH: &str = "textures/armor.dds";
const SOURCE_ARCHIVE_HASH: &str = "feedfacefeedface";
const FOMOD_CONFIG: &str = "[fomod]\nchoice = \"option-a\"\n";

/// Insert the installed-file row `export_profile_lock` requires so the mod is
/// not flagged as incomplete ("no tracked `installed_mod_files` rows").
async fn record_installed_file(db: &ModdeDb, profile_id: i64, size: u64) {
    let plan = InstallPlan {
        method: InstallMethod::BareExtract,
        strip_prefix: None,
        source_archive_hash: SOURCE_ARCHIVE_HASH.to_string(),
        staged_files: vec![StagedFile {
            rel_path: REL_PATH.to_string(),
            origin_rel_path: REL_PATH.to_string(),
            size,
            merge_group: None,
        }],
    };
    db.record_install(
        profile_id,
        &ModId::from(MOD_ID),
        &plan,
        InstallStatus::Installed,
    )
    .await
    .expect("record installed file");
}

fn read_lock(path: &std::path::Path) -> ModdeLock {
    let raw = std::fs::read_to_string(path).expect("read lock file");
    serde_json::from_str(&raw).expect("parse lock file")
}

#[tokio::test]
async fn lock_export_import_reexport_round_trips_populated_profile() {
    let fx = Fixture::new();
    let game = GameId::from("skyrim-se");
    let first_lock = fx.root().join("modde.lock");
    let second_lock = fx.root().join("modde-reexport.lock");
    let public_key = fx.root().join("modde-lock.pub.json");
    let secret_key = fx.root().join("modde-lock.secret.json");

    // A real store file must back the installed-file row so export can hash it.
    let store_file = fx.data_dir().join("store").join(MOD_ID).join(REL_PATH);
    std::fs::create_dir_all(store_file.parent().unwrap()).unwrap();
    std::fs::write(&store_file, b"armor texture payload").unwrap();
    let size = std::fs::metadata(&store_file).unwrap().len();

    // Seed a profile whose mod row carries fomod_config, install_method, and
    // source_archive_hash, plus Nexus provenance so the export is reproducible.
    let db = ModdeDb::open_at(&fx.data_dir().join("modde.db"))
        .await
        .expect("open fixture database");
    let pm = ProfileManager::with_db(db);
    let profile = Profile {
        id: None,
        name: "portable".to_string(),
        game_id: game.clone(),
        source: ProfileSource::Manual,
        mods: vec![EnabledMod {
            mod_id: MOD_ID.to_string(),
            display_name: Some("Test Mod".to_string()),
            enabled: true,
            version: Some("1.2.3".to_string()),
            fomod_config: Some(FOMOD_CONFIG.to_string()),
            nexus_mod_id: Some(42u64.into()),
            nexus_file_id: Some(7u64.into()),
            nexus_game_domain: Some("skyrimspecialedition".to_string()),
            install_method: Some(InstallMethod::BareExtract),
            source_archive_hash: Some(SOURCE_ARCHIVE_HASH.to_string()),
            ..Default::default()
        }],
        overrides: fx.data_dir().join("profiles/portable/overrides"),
        load_order_rules: Default::default(),
        load_order_lock: None,
    };
    let profile_id = pm.create(&profile).await.expect("create profile");
    record_installed_file(pm.db(), profile_id, size).await;

    let export = fx
        .cmd()
        .args([
            "lock",
            "export",
            "--profile",
            "portable",
            "--game",
            "skyrim-se",
            "--output",
            first_lock.to_str().unwrap(),
        ])
        .output()
        .expect("lock export");
    assert!(
        export.status.success(),
        "lock export failed: {}",
        String::from_utf8_lossy(&export.stderr)
    );

    let first = read_lock(&first_lock);
    assert!(
        first.payload.reproducible,
        "seeded export should be reproducible"
    );
    assert_eq!(first.payload.mods.len(), 1);
    let locked_mod = &first.payload.mods[0];
    assert_eq!(locked_mod.fomod_config.as_deref(), Some(FOMOD_CONFIG));
    assert_eq!(locked_mod.install_method, Some(InstallMethod::BareExtract));
    assert_eq!(
        locked_mod.source_archive_hash.as_deref(),
        Some(SOURCE_ARCHIVE_HASH)
    );
    assert_eq!(locked_mod.files.len(), 1);
    assert_eq!(locked_mod.files[0].rel_path, REL_PATH);

    // Import only accepts signed locks, so keygen + sign before importing.
    let keygen = fx
        .cmd()
        .args([
            "lock",
            "keygen",
            "--public",
            public_key.to_str().unwrap(),
            "--secret",
            secret_key.to_str().unwrap(),
        ])
        .output()
        .expect("lock keygen");
    assert!(
        keygen.status.success(),
        "lock keygen failed: {}",
        String::from_utf8_lossy(&keygen.stderr)
    );
    let sign = fx
        .cmd()
        .args([
            "lock",
            "sign",
            first_lock.to_str().unwrap(),
            "--secret-key",
            secret_key.to_str().unwrap(),
        ])
        .output()
        .expect("lock sign");
    assert!(
        sign.status.success(),
        "lock sign failed: {}",
        String::from_utf8_lossy(&sign.stderr)
    );

    // Drop the profile, then restore it from the lock.
    pm.delete("portable", Some(&game))
        .await
        .expect("delete profile");
    let import = fx
        .cmd()
        .args([
            "lock",
            "import",
            first_lock.to_str().unwrap(),
            "--profile",
            "portable",
            "--game",
            "skyrim-se",
            "--apply",
        ])
        .output()
        .expect("lock import");
    assert!(
        import.status.success(),
        "lock import failed: {}",
        String::from_utf8_lossy(&import.stderr)
    );

    // profile_from_lock must have carried the provenance fields across.
    let imported = pm
        .load("portable", Some(&game))
        .await
        .expect("load imported profile");
    assert_eq!(imported.mods.len(), 1);
    let imported_mod = &imported.mods[0];
    assert_eq!(imported_mod.fomod_config.as_deref(), Some(FOMOD_CONFIG));
    assert_eq!(
        imported_mod.install_method,
        Some(InstallMethod::BareExtract)
    );
    assert_eq!(
        imported_mod.source_archive_hash.as_deref(),
        Some(SOURCE_ARCHIVE_HASH)
    );

    // Import restores metadata only; re-seed the installed-file row so the
    // re-export passes the same completeness checks as the original.
    let imported_id = imported.id.expect("imported profile id");
    record_installed_file(pm.db(), imported_id, size).await;

    let reexport = fx
        .cmd()
        .args([
            "lock",
            "export",
            "--profile",
            "portable",
            "--game",
            "skyrim-se",
            "--output",
            second_lock.to_str().unwrap(),
        ])
        .output()
        .expect("lock re-export");
    assert!(
        reexport.status.success(),
        "lock re-export failed: {}",
        String::from_utf8_lossy(&reexport.stderr)
    );

    let mut second = read_lock(&second_lock);
    // generated_at is the only field expected to differ between the exports.
    second.payload.generated_at = first.payload.generated_at.clone();
    assert_eq!(
        first.payload, second.payload,
        "re-exported lock payload should match the original export"
    );
}
