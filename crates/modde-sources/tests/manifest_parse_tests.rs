use modde_core::manifest::wabbajack::{ArchiveState, InstallDirective};
use modde_sources::wabbajack::manifest::parse_wabbajack_file;
use std::io::Write;

/// Helper: create a zip file on disk with the given entries.
fn write_zip(path: &std::path::Path, entries: &[(&str, &[u8])]) {
    let file = std::fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap();
}

/// Minimal valid manifest JSON.
fn valid_manifest_json() -> String {
    serde_json::json!({
        "Name": "TestList",
        "Author": "tester",
        "Description": "desc",
        "Game": "SkyrimSE",
        "Version": "1.0.0",
        "Archives": [],
        "Directives": []
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// 1. parse_wabbajack_file with a valid zip containing "modlist"
// ---------------------------------------------------------------------------
#[test]
fn parse_valid_wabbajack_with_modlist_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.wabbajack");

    write_zip(&path, &[("modlist", valid_manifest_json().as_bytes())]);

    let manifest = parse_wabbajack_file(&path).expect("should parse successfully");
    assert_eq!(manifest.name, "TestList");
    assert_eq!(manifest.author, "tester");
    assert_eq!(manifest.game, "SkyrimSE");
    assert_eq!(manifest.version, "1.0.0");
    assert!(manifest.archives.is_empty());
    assert!(manifest.directives.is_empty());
}

// ---------------------------------------------------------------------------
// 1b. parse_wabbajack_file also finds .json entry
// ---------------------------------------------------------------------------
#[test]
fn parse_valid_wabbajack_with_json_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.wabbajack");

    write_zip(
        &path,
        &[("manifest.json", valid_manifest_json().as_bytes())],
    );

    let manifest = parse_wabbajack_file(&path).expect("should parse successfully");
    assert_eq!(manifest.name, "TestList");
}

// ---------------------------------------------------------------------------
// 2. parse_wabbajack_file with zip missing manifest
// ---------------------------------------------------------------------------
#[test]
fn parse_wabbajack_missing_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.wabbajack");

    // Zip with an entry that is NOT "modlist" and does NOT end in ".json"
    write_zip(&path, &[("readme.txt", b"hello")]);

    let result = parse_wabbajack_file(&path);
    assert!(result.is_err());
    let err_msg = format!("{:#}", result.unwrap_err());
    assert!(
        err_msg.contains("no manifest found"),
        "unexpected error: {err_msg}"
    );
}

// ---------------------------------------------------------------------------
// 3. parse_wabbajack_file with nonexistent file
// ---------------------------------------------------------------------------
#[test]
fn parse_wabbajack_nonexistent_file() {
    let result = parse_wabbajack_file(std::path::Path::new("/tmp/does_not_exist_12345.wabbajack"));
    assert!(result.is_err());
    let err_msg = format!("{:#}", result.unwrap_err());
    assert!(
        err_msg.contains("failed to open"),
        "unexpected error: {err_msg}"
    );
}

// ---------------------------------------------------------------------------
// 4. parse_wabbajack_file with invalid zip (random bytes)
// ---------------------------------------------------------------------------
#[test]
fn parse_wabbajack_invalid_zip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("garbage.wabbajack");

    std::fs::write(&path, b"this is not a zip file at all").unwrap();

    let result = parse_wabbajack_file(&path);
    assert!(result.is_err());
    let err_msg = format!("{:#}", result.unwrap_err());
    assert!(
        err_msg.contains("zip") || err_msg.contains("archive"),
        "unexpected error: {err_msg}"
    );
}

#[test]
fn parse_lotf_fixture_wabbajack_game_file_sources() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lotf-fixture.wabbajack");
    write_zip(
        &path,
        &[(
            "modlist",
            include_bytes!("fixtures/lotf_manifest.json").as_slice(),
        )],
    );
    let manifest = parse_wabbajack_file(&path).expect("parse LOTF .wabbajack");
    assert_eq!(manifest.name, "Legends of the Frost");

    let game_file_hashes: std::collections::HashSet<u64> = manifest
        .archives
        .iter()
        .filter_map(|archive| {
            matches!(
                archive.state.as_ref(),
                Some(ArchiveState::GameFileSourceDownloader { .. })
            )
            .then_some(archive.hash)
        })
        .collect();

    assert!(
        !game_file_hashes.is_empty(),
        "LOTF should contain GameFileSourceDownloader archives"
    );

    for directive in manifest.download_directives() {
        assert!(
            !game_file_hashes.contains(&directive.hash()),
            "game-file source should not be emitted as a download directive"
        );
    }

    let install_directives = manifest.install_directives();
    assert!(
        install_directives.iter().any(|directive| match directive {
            InstallDirective::FromArchive { archive_hash, .. }
            | InstallDirective::PatchedFromArchive { archive_hash, .. } =>
                game_file_hashes.contains(archive_hash),
            InstallDirective::InlineFile { .. } | InstallDirective::CreateBSA { .. } => false,
        }),
        "LOTF should plan at least one install directive from a game-file archive"
    );
}

#[test]
fn parse_3077_fixture_cyberpunk_mo2_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("3077-fixture.wabbajack");
    write_zip(
        &path,
        &[(
            "modlist",
            include_bytes!("fixtures/3077_manifest.json").as_slice(),
        )],
    );

    let manifest = parse_wabbajack_file(&path).expect("parse 3077 .wabbajack");
    assert_eq!(manifest.name, "Project 2077");
    assert_eq!(manifest.game, "Cyberpunk2077");

    let install_directives = manifest.install_directives();
    assert!(
        install_directives.iter().any(|directive| match directive {
            InstallDirective::FromArchive { to, .. }
            | InstallDirective::PatchedFromArchive { to, .. }
            | InstallDirective::InlineFile { to, .. }
            | InstallDirective::CreateBSA { to, .. } => {
                let to = to.replace('\\', "/").to_ascii_lowercase();
                to.starts_with("mods/")
                    && (to.contains("/bin/x64/plugins/cyber_engine_tweaks/mods/")
                        || to.contains("/archive/pc/mod/"))
            }
        }),
        "3077 fixture should preserve Cyberpunk MO2-staged CET/archive paths"
    );
}
