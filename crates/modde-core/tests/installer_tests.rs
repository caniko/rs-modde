use std::path::PathBuf;

use modde_core::installer::{InstallMethod, InstallPlan};

fn touch(path: &std::path::Path, body: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

fn plan(method: InstallMethod) -> InstallPlan {
    InstallPlan {
        method,
        strip_prefix: None,
        source_archive_hash: "fixture".into(),
        staged_files: Vec::new(),
    }
}

#[test]
fn strip_content_root_stages_data_contents_without_data_prefix() {
    let tmp = tempfile::tempdir().unwrap();
    let extracted = tmp.path().join("extracted");
    let store = tmp.path().join("store");
    touch(&extracted.join("Data/Foo.esp"), b"plugin");

    let mut plan = plan(InstallMethod::StripContentRoot {
        root: "Data".into(),
    });
    let files = modde_core::installer::execute(&mut plan, &extracted, &store).unwrap();

    assert!(store.join("Foo.esp").exists());
    assert_eq!(files[0].rel_path, "Foo.esp");
    assert_eq!(files[0].origin_rel_path, "Data/Foo.esp");
}

#[test]
fn directory_mod_stages_root_under_store_mod_name() {
    let tmp = tempfile::tempdir().unwrap();
    let extracted = tmp.path().join("extracted");
    let store = tmp.path().join("store/BetterRanching");
    touch(&extracted.join("manifest.json"), b"{}");

    let mut plan = plan(InstallMethod::DirectoryMod {
        directory_name: None,
    });
    let files = modde_core::installer::execute(&mut plan, &extracted, &store).unwrap();

    assert!(store.join("BetterRanching/manifest.json").exists());
    assert_eq!(files[0].rel_path, "BetterRanching/manifest.json");
}

#[test]
fn directory_mod_from_xml_uses_marker_id() {
    let tmp = tempfile::tempdir().unwrap();
    let extracted = tmp.path().join("extracted");
    let store = tmp.path().join("store/bannerlord_mod");
    touch(
        &extracted.join("SubModule.xml"),
        br#"<Module><Name value="Visible"/><Id value="StableId"/></Module>"#,
    );

    let mut plan = plan(InstallMethod::DirectoryModFromXml {
        marker: PathBuf::from("SubModule.xml"),
        id_attr: "Id.value".into(),
        fallback_name: None,
    });
    let files = modde_core::installer::execute(&mut plan, &extracted, &store).unwrap();

    assert!(store.join("StableId/SubModule.xml").exists());
    assert_eq!(files[0].rel_path, "StableId/SubModule.xml");
}

#[test]
fn multi_root_overlay_preserves_selected_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let extracted = tmp.path().join("extracted");
    let store = tmp.path().join("store/witcher_mod");
    touch(
        &extracted.join("mods/modFoo/content/scripts/a.ws"),
        b"script",
    );
    touch(&extracted.join("dlc/dlcFoo/content/blob.bundle"), b"bundle");
    touch(&extracted.join("bin/config.xml"), b"xml");

    let mut plan = plan(InstallMethod::MultiRootOverlay {
        roots: vec!["mods".into(), "dlc".into(), "bin".into()],
    });
    let files = modde_core::installer::execute(&mut plan, &extracted, &store).unwrap();
    let rels = files
        .iter()
        .map(|file| file.rel_path.as_str())
        .collect::<Vec<_>>();

    assert!(store.join("mods/modFoo/content/scripts/a.ws").exists());
    assert!(rels.contains(&"mods/modFoo/content/scripts/a.ws"));
    assert!(rels.contains(&"dlc/dlcFoo/content/blob.bundle"));
    assert!(rels.contains(&"bin/config.xml"));
}

#[test]
fn single_file_set_stages_only_root_files() {
    let tmp = tempfile::tempdir().unwrap();
    let extracted = tmp.path().join("extracted");
    let store = tmp.path().join("store/bg3_mod");
    touch(&extracted.join("Cool.pak"), b"pak");
    touch(&extracted.join("nested/Ignored.pak"), b"pak");

    let mut plan = plan(InstallMethod::SingleFileSet);
    let files = modde_core::installer::execute(&mut plan, &extracted, &store).unwrap();

    assert_eq!(files.len(), 1);
    assert!(store.join("Cool.pak").exists());
    assert!(!store.join("Ignored.pak").exists());
}
