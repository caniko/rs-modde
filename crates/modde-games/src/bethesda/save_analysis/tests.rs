use super::*;

fn save_bytes(magic: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(magic);
    bytes.extend_from_slice(&32u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(b"Test");
    bytes.push(0);
    bytes.extend_from_slice(payload);
    bytes
}

#[test]
fn skyrim_save_symbols_include_plugins_and_scripts() {
    let tmp = tempfile::tempdir().unwrap();
    let save = tmp.path().join("Save1.ess");
    std::fs::write(
        &save,
        save_bytes(
            MAGIC_SKYRIM_SE,
            b"SomeMod.esp scripts/MyQuestScript.pex ActiveScript MyQuestScript",
        ),
    )
    .unwrap();

    let parsed = SKYRIM_SAVE_ANALYZER.parse_save_symbols(&save).unwrap();
    assert!(parsed.plugins.contains("somemod.esp"));
    assert!(parsed.scripts.contains("myquestscript"));
    assert!(parsed.active_scripts.contains("myquestscript"));
}

#[test]
fn skyrim_save_symbols_include_zlib_payload() {
    let tmp = tempfile::tempdir().unwrap();
    let save = tmp.path().join("Save1.ess");
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(
        &mut encoder,
        b"CompressedMod.esp scripts/CompressedScript.pex ActiveScript CompressedScript",
    )
    .unwrap();
    let compressed = encoder.finish().unwrap();
    std::fs::write(&save, save_bytes(MAGIC_SKYRIM_SE, &compressed)).unwrap();

    let parsed = SKYRIM_SAVE_ANALYZER.parse_save_symbols(&save).unwrap();
    assert!(parsed.plugins.contains("compressedmod.esp"));
    assert!(parsed.scripts.contains("compressedscript"));
    assert!(parsed.active_scripts.contains("compressedscript"));
}

#[test]
fn invalid_save_header_fails_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let save = tmp.path().join("Save1.ess");
    std::fs::write(&save, b"not a save").unwrap();
    assert!(SKYRIM_SAVE_ANALYZER.parse_save_symbols(&save).is_err());
}

#[test]
fn analyzer_blocks_loose_plugin_dependency() {
    let tmp = tempfile::tempdir().unwrap();
    let mod_dir = tmp.path().join("mod");
    let saves = tmp.path().join("saves");
    std::fs::create_dir_all(&mod_dir).unwrap();
    std::fs::create_dir_all(&saves).unwrap();
    std::fs::write(mod_dir.join("SomeMod.esp"), b"TES4").unwrap();
    std::fs::write(
        saves.join("Save1.ess"),
        save_bytes(MAGIC_SKYRIM_SE, b"plugins SomeMod.esp"),
    )
    .unwrap();

    let report = SKYRIM_SAVE_ANALYZER
        .analyze_mod_removal("mod", &mod_dir, &[saves])
        .unwrap();
    assert_eq!(report.safety, ModSafety::SaveBreaking);
    assert!(report.is_blocked());
    assert_eq!(
        report.blocking_findings[0].dependency_kind,
        SaveDependencyKind::PluginRecord
    );
}

#[test]
fn analyzer_allows_asset_only_mod() {
    let tmp = tempfile::tempdir().unwrap();
    let mod_dir = tmp.path().join("mod");
    let saves = tmp.path().join("saves");
    std::fs::create_dir_all(mod_dir.join("textures")).unwrap();
    std::fs::create_dir_all(&saves).unwrap();
    std::fs::write(mod_dir.join("textures/sky.dds"), b"DDS").unwrap();
    std::fs::write(
        saves.join("Save1.ess"),
        save_bytes(MAGIC_SKYRIM_SE, b"plugins SomeMod.esp"),
    )
    .unwrap();

    let report = SKYRIM_SAVE_ANALYZER
        .analyze_mod_removal("mod", &mod_dir, &[saves])
        .unwrap();
    assert_eq!(report.safety, ModSafety::SaveSafe);
    assert!(!report.is_blocked());
}

#[test]
fn analyzer_blocks_loose_pex_dependency() {
    let tmp = tempfile::tempdir().unwrap();
    let mod_dir = tmp.path().join("mod");
    let saves = tmp.path().join("saves");
    std::fs::create_dir_all(mod_dir.join("scripts")).unwrap();
    std::fs::create_dir_all(&saves).unwrap();
    std::fs::write(mod_dir.join("scripts/MyQuestScript.pex"), b"pex").unwrap();
    std::fs::write(
        saves.join("Save1.ess"),
        save_bytes(MAGIC_SKYRIM_SE, b"ActiveScript MyQuestScript"),
    )
    .unwrap();

    let report = SKYRIM_SAVE_ANALYZER
        .analyze_mod_removal("mod", &mod_dir, &[saves])
        .unwrap();
    assert_eq!(report.safety, ModSafety::SaveBreaking);
    assert!(report.is_blocked());
    assert!(
        report
            .blocking_findings
            .iter()
            .any(|f| f.dependency_kind == SaveDependencyKind::ActiveScript)
    );
}

#[test]
fn analyzer_fails_closed_on_invalid_archive() {
    let tmp = tempfile::tempdir().unwrap();
    let mod_dir = tmp.path().join("mod");
    let saves = tmp.path().join("saves");
    std::fs::create_dir_all(&mod_dir).unwrap();
    std::fs::create_dir_all(&saves).unwrap();
    std::fs::write(mod_dir.join("PackedScripts.bsa"), b"not a valid bsa").unwrap();

    let report = SKYRIM_SAVE_ANALYZER
        .analyze_mod_removal("mod", &mod_dir, &[saves])
        .unwrap();
    assert_eq!(report.safety, ModSafety::Unknown);
    assert!(report.is_blocked());
    assert!(
        report
            .blocking_findings
            .iter()
            .any(|f| f.dependency_kind == SaveDependencyKind::ParseIncomplete)
    );
}
