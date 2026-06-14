use super::*;

#[test]
fn find_entry_exact_match() {
    let dir = tempfile::tempdir().unwrap();
    let zip_path = dir.path().join("test.zip");
    create_zip_file(&zip_path, &[("data/meshes/test.nif", b"mesh")]);
    let archive = open_zip(&zip_path);

    let result = find_entry_in_archive(&archive, "data/meshes/test.nif").unwrap();
    assert_eq!(result, "data/meshes/test.nif");
}

// -----------------------------------------------------------------------
// 4. find_entry_in_archive with normalized paths (/ vs \)
// -----------------------------------------------------------------------
#[test]
fn find_entry_backslash_to_forward_slash() {
    let dir = tempfile::tempdir().unwrap();
    let zip_path = dir.path().join("test.zip");
    create_zip_file(&zip_path, &[("data/meshes/test.nif", b"mesh")]);
    let archive = open_zip(&zip_path);

    let result = find_entry_in_archive(&archive, "data\\meshes\\test.nif").unwrap();
    assert_eq!(result, "data/meshes/test.nif");
}

#[test]
fn find_entry_forward_slash_to_backslash() {
    let dir = tempfile::tempdir().unwrap();
    let zip_path = dir.path().join("test.zip");
    create_zip_file(&zip_path, &[("data\\meshes\\test.nif", b"mesh")]);
    let archive = open_zip(&zip_path);

    let result = find_entry_in_archive(&archive, "data/meshes/test.nif").unwrap();
    assert_eq!(result, "data\\meshes\\test.nif");
}

// -----------------------------------------------------------------------
// 5. find_entry_in_archive with case-insensitive fallback
// -----------------------------------------------------------------------
#[test]
fn find_entry_case_insensitive() {
    let dir = tempfile::tempdir().unwrap();
    let zip_path = dir.path().join("test.zip");
    create_zip_file(&zip_path, &[("Data/Meshes/Test.NIF", b"mesh")]);
    let archive = open_zip(&zip_path);

    let result = find_entry_in_archive(&archive, "data/meshes/test.nif").unwrap();
    assert_eq!(result, "Data/Meshes/Test.NIF");
}

#[test]
fn find_entry_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let zip_path = dir.path().join("test.zip");
    create_zip_file(&zip_path, &[("other.txt", b"data")]);
    let archive = open_zip(&zip_path);

    let result = find_entry_in_archive(&archive, "nonexistent.txt");
    assert!(result.is_err());
}

// -----------------------------------------------------------------------
// 6. archive_path formatting (hash as 016x)
// -----------------------------------------------------------------------
#[test]
fn archive_path_zero_padded_hex() {
    let store = PathBuf::from("/store");
    let hash: u64 = 0xDEADBEEF;
    let path = archive_path(&store, &hash);
    assert_eq!(path, PathBuf::from("/store/00000000deadbeef.archive"));
}

#[test]
fn archive_path_full_width_hash() {
    let store = PathBuf::from("/store");
    let hash: u64 = 0xFFFFFFFFFFFFFFFF;
    let path = archive_path(&store, &hash);
    assert_eq!(path, PathBuf::from("/store/ffffffffffffffff.archive"));
}

#[test]
fn archive_path_zero_hash() {
    let store = PathBuf::from("/store");
    let hash: u64 = 0;
    let path = archive_path(&store, &hash);
    assert_eq!(path, PathBuf::from("/store/0000000000000000.archive"));
}

// -----------------------------------------------------------------------
// 7. directive_name for each directive type
// -----------------------------------------------------------------------
#[test]
fn directive_name_nexus() {
    let d = DownloadDirective::Nexus {
        game_id: "skyrimse".into(),
        mod_id: 12345.into(),
        file_id: 1.into(),
        hash: 0,
    };
    assert_eq!(d.display_name(), "nexus:12345");
}

#[test]
fn directive_name_github() {
    let d = DownloadDirective::GitHub {
        user: "user".into(),
        repo: "myrepo".into(),
        tag: "v1".into(),
        asset: "a.zip".into(),
        hash: 0,
    };
    assert_eq!(d.display_name(), "github:myrepo");
}

#[test]
fn directive_name_gdrive() {
    let d = DownloadDirective::GoogleDrive {
        id: "abc123".into(),
        hash: 0,
    };
    assert_eq!(d.display_name(), "gdrive:abc123");
}

#[test]
fn directive_name_mega() {
    let d = DownloadDirective::Mega {
        url: "https://mega.nz/file/ABCDEF#key".into(),
        hash: 0,
    };
    let name = d.display_name();
    assert!(name.starts_with("mega:"));
    assert!(name.len() <= 35); // "mega:" + up to 30 chars
}

#[test]
fn directive_name_direct_url() {
    let d = DownloadDirective::DirectURL {
        url: "https://example.com/files/mod.zip".into(),
        headers: HashMap::new(),
        mirror_resolver: None,
        hash: 0,
    };
    let name = d.display_name();
    assert!(name.starts_with("http:"));
    assert!(name.len() <= 35); // "http:" + up to 30 chars
}

#[test]
fn directive_name_mega_short_url() {
    let d = DownloadDirective::Mega {
        url: "https://mega.nz/short".into(),
        hash: 0,
    };
    let name = d.display_name();
    assert_eq!(name, "mega:https://mega.nz/short");
}

// -----------------------------------------------------------------------
// 8. directive_hash extraction for each directive type
// -----------------------------------------------------------------------
#[test]
fn directive_hash_nexus() {
    let d = DownloadDirective::Nexus {
        game_id: "s".into(),
        mod_id: 1.into(),
        file_id: 1.into(),
        hash: 0xABCD,
    };
    assert_eq!(d.hash(), 0xABCD);
}

#[test]
fn directive_hash_github() {
    let d = DownloadDirective::GitHub {
        user: "u".into(),
        repo: "r".into(),
        tag: "t".into(),
        asset: "a".into(),
        hash: 999,
    };
    assert_eq!(d.hash(), 999);
}

#[test]
fn directive_hash_gdrive() {
    let d = DownloadDirective::GoogleDrive {
        id: "x".into(),
        hash: 42,
    };
    assert_eq!(d.hash(), 42);
}

#[test]
fn directive_hash_mega() {
    let d = DownloadDirective::Mega {
        url: "u".into(),
        hash: 7777,
    };
    assert_eq!(d.hash(), 7777);
}

#[test]
fn directive_hash_direct_url() {
    let d = DownloadDirective::DirectURL {
        url: "u".into(),
        headers: HashMap::new(),
        mirror_resolver: None,
        hash: 0xFFFF,
    };
    assert_eq!(d.hash(), 0xFFFF);
}

// -----------------------------------------------------------------------
// 9. extract_from_zip with valid zip
// -----------------------------------------------------------------------
#[test]
fn extract_from_zip_valid() {
    let dir = tempfile::tempdir().unwrap();
    let zip_path = dir.path().join("test.zip");
    create_zip_file(&zip_path, &[("inner/file.txt", b"hello world")]);

    let data = extract_from_zip(&zip_path, "inner/file.txt").unwrap();
    assert_eq!(data, b"hello world");
}

// -----------------------------------------------------------------------
// 10. extract_from_zip with nonexistent entry
// -----------------------------------------------------------------------
#[test]
fn extract_from_zip_missing_entry() {
    let dir = tempfile::tempdir().unwrap();
    let zip_path = dir.path().join("test.zip");
    create_zip_file(&zip_path, &[("exists.txt", b"data")]);

    let result = extract_from_zip(&zip_path, "does_not_exist.txt");
    assert!(result.is_err());
    let err_msg = format!("{:#}", result.unwrap_err());
    assert!(err_msg.contains("not found"), "unexpected error: {err_msg}");
}

// -----------------------------------------------------------------------
// 11. normalize_path
// -----------------------------------------------------------------------
#[test]
fn normalize_path_backslashes() {
    assert_eq!(normalize_path("mods\\test\\file.txt"), "mods/test/file.txt");
}

#[test]
fn normalize_path_already_forward() {
    assert_eq!(normalize_path("mods/test/file.txt"), "mods/test/file.txt");
}
