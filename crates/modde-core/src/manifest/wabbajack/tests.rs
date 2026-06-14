use super::*;

#[test]
fn truncate_str_never_splits_a_codepoint() {
    // 1 ASCII byte + 3-byte chars => byte 30 lands mid-codepoint.
    let s = format!("x{}", "€".repeat(20));
    assert!(!s.is_char_boundary(30));
    let t = truncate_str(&s, 30);
    assert!(t.len() <= 30 && s.is_char_boundary(t.len()));
    assert_eq!(truncate_str("abc", 30), "abc");
}

#[test]
fn display_name_does_not_panic_on_multibyte_url() {
    let url = format!("https://mega.nz/{}", "€".repeat(20));
    let _ = DownloadDirective::Mega { url, hash: 0 }.display_name();
}

#[test]
fn game_file_source_downloader_parses_and_is_not_downloaded() {
    let json = r#"{
        "Name": "Legends of the Frost synthetic",
        "Author": "test",
        "Description": "test",
        "Game": "SkyrimSE",
        "Version": "1.0.0",
        "Archives": [
            {
                "Hash": "AQAAAAAAAAA=",
                "Name": "Data_Skyrim.esm",
                "Size": 1024,
                "State": {
                    "$type": "GameFileSourceDownloader, Wabbajack.Lib",
                    "File": "Data\\Skyrim.esm"
                }
            },
            {
                "Hash": "AgAAAAAAAAA=",
                "Name": "mod.zip",
                "Size": 2048,
                "State": {
                    "$type": "HttpDownloader, Wabbajack.Lib",
                    "Url": "https://example.invalid/mod.zip",
                    "Headers": []
                }
            }
        ],
        "Directives": [
            {
                "$type": "FromArchive, Wabbajack.Lib",
                "ArchiveHashPath": ["AQAAAAAAAAA="],
                "To": "mods/Skyrim Base/Skyrim.esm"
            }
        ]
    }"#;

    let manifest: WabbajackManifest = serde_json::from_str(json).unwrap();
    let source = manifest.archives[0].state.as_ref().unwrap();
    assert_eq!(source.game_file_path(), Some("Data\\Skyrim.esm"));

    let downloads = manifest.download_directives();
    assert_eq!(downloads.len(), 1);
    assert_eq!(downloads[0].hash(), 2);

    let installs = manifest.install_directives();
    assert!(matches!(
        installs.as_slice(),
        [InstallDirective::FromArchive {
            archive_hash: 1,
            from,
            to,
            ..
        }] if from.is_empty() && to == "mods/Skyrim Base/Skyrim.esm"
    ));
}

#[test]
fn wabbajack_cdn_downloader_parses_as_authored_download() {
    let json = r#"{
        "Name": "Legends of the Frost synthetic",
        "Author": "test",
        "Description": "test",
        "Game": "SkyrimSE",
        "Version": "1.0.0",
        "Archives": [
            {
                "Hash": "AQAAAAAAAAA=",
                "Name": "Legends of the Frost - Generated Output.7z",
                "Size": 1024,
                "State": {
                    "$type": "WabbajackCDNDownloader+State, Wabbajack.Lib",
                    "Url": "https://authored-files.wabbajack.org/Generated%20Output.7z_abc",
                    "MungedName": "Generated Output.7z_abc"
                }
            },
            {
                "Hash": "AgAAAAAAAAA=",
                "Name": "mod.zip",
                "Size": 2048,
                "State": {
                    "$type": "HttpDownloader, Wabbajack.Lib",
                    "Url": "https://example.invalid/mod.zip",
                    "Headers": []
                }
            }
        ],
        "Directives": []
    }"#;

    let manifest: WabbajackManifest = serde_json::from_str(json).unwrap();
    assert!(matches!(
        manifest.archives[0].state.as_ref(),
        Some(ArchiveState::WabbajackCDNDownloader { metadata })
            if metadata.get("MungedName").and_then(serde_json::Value::as_str)
                == Some("Generated Output.7z_abc")
    ));

    let downloads = manifest.download_directives();
    assert_eq!(downloads.len(), 2);
    assert!(matches!(
        &downloads[0],
        DownloadDirective::WabbajackCdn { url, hash }
            if url == "https://authored-files.wabbajack.org/Generated%20Output.7z_abc"
                && *hash == 1
    ));
    assert_eq!(downloads[1].hash(), 2);
}

#[test]
fn moddb_downloader_parses_as_direct_url_download() {
    let json = r#"{
        "Name": "Legends of the Frost synthetic",
        "Author": "test",
        "Description": "test",
        "Game": "SkyrimSE",
        "Version": "1.0.0",
        "Archives": [
            {
                "Hash": "AQAAAAAAAAA=",
                "Name": "Skyrim_Realistic_Overhaul_Part_1.7z",
                "Size": 1024,
                "State": {
                    "$type": "ModDBDownloader, Wabbajack.Lib",
                    "Url": "https://www.moddb.com/downloads/start/116891",
                    "PrimaryKeyString": "ModDBDownloader+State|https://www.moddb.com/downloads/start/116891"
                }
            }
        ],
        "Directives": []
    }"#;

    let manifest: WabbajackManifest = serde_json::from_str(json).unwrap();
    assert!(matches!(
        manifest.archives[0].state.as_ref(),
        Some(ArchiveState::ModDBDownloader { url, metadata })
            if url == "https://www.moddb.com/downloads/start/116891"
                && metadata.contains_key("PrimaryKeyString")
    ));

    let downloads = manifest.download_directives();
    assert!(matches!(
        downloads.as_slice(),
        [DownloadDirective::DirectURL {
            url,
            headers,
            mirror_resolver,
            hash
        }]
            if url == "https://www.moddb.com/downloads/start/116891"
                && headers.is_empty()
                && mirror_resolver.as_ref().is_some_and(|resolver| {
                    resolver.name == "moddb-html-mirror"
                        && resolver.original_url == "https://www.moddb.com/downloads/start/116891"
                        && resolver.listing_url == "https://www.moddb.com/downloads/start/116891/all"
                        && resolver.link_id == "downloadon"
                        && resolver.user_agent.as_deref() == Some("Wabbajack/4.0 modde")
                })
                && *hash == 1
    ));
}

#[test]
fn moddb_downloader_derives_mirror_listing_url_with_query_string() {
    assert_eq!(
        moddb_download_id("https://www.moddb.com/downloads/start/116927?referer=x"),
        Some("116927")
    );
    let resolver =
        moddb_html_mirror_resolver("https://www.moddb.com/downloads/start/116927?referer=x")
            .unwrap();
    assert_eq!(
        resolver.listing_url,
        "https://www.moddb.com/downloads/start/116927/all"
    );
}

#[test]
fn create_bsa_file_state_hash_defaults_when_absent() {
    let json = r#"{
        "Name": "Legends of the Frost synthetic",
        "Author": "test",
        "Description": "test",
        "Game": "SkyrimSE",
        "Version": "1.0.0",
        "Archives": [],
        "Directives": [
            {
                "$type": "CreateBSA, Wabbajack.Lib",
                "TempID": "textures.bsa",
                "To": "mods/Generated/textures.bsa",
                "FileStates": [
                    {
                        "$type": "BSAFileState, Compression.BSA",
                        "FlipCompression": false,
                        "Index": 0,
                        "Path": "textures\\architecture\\riften\\riftenrope01.dds"
                    }
                ]
            }
        ]
    }"#;

    let manifest: WabbajackManifest = serde_json::from_str(json).unwrap();
    let installs = manifest.install_directives();
    assert!(matches!(
        installs.as_slice(),
        [InstallDirective::CreateBSA { file_states, .. }]
            if file_states.len() == 1
                && file_states[0].path == "textures\\architecture\\riften\\riftenrope01.dds"
                && file_states[0].hash == 0
    ));
}

#[test]
fn remapped_inline_file_parses_as_inline_install_directive() {
    let json = r#"{
        "Name": "Twisted synthetic",
        "Author": "test",
        "Description": "test",
        "Game": "SkyrimSpecialEdition",
        "Version": "1.0.0",
        "Archives": [],
        "Directives": [
            {
                "$type": "RemappedInlineFile",
                "Hash": "H6Wy/QKDBVE=",
                "Size": 6022,
                "SourceDataID": "db027c84-eb75-4852-ae01-72cc75abe3a1",
                "To": "mods\\BodySlide and Outfit Studio\\CalienteTools\\BodySlide\\Config.xml"
            }
        ]
    }"#;

    let manifest: WabbajackManifest = serde_json::from_str(json).unwrap();
    let installs = manifest.install_directives();
    assert!(matches!(
        installs.as_slice(),
        [InstallDirective::InlineFile { source_data_id, to }]
            if source_data_id == "db027c84-eb75-4852-ae01-72cc75abe3a1"
                && to == "mods\\BodySlide and Outfit Studio\\CalienteTools\\BodySlide\\Config.xml"
    ));
}

#[test]
fn install_directives_grouped_by_archive_returns_one_batch_per_archive() {
    let manifest = WabbajackManifest {
        name: "batch synthetic".into(),
        author: "test".into(),
        description: "test".into(),
        game: "SkyrimSE".into(),
        version: "1.0.0".into(),
        archives: vec![
            ArchiveEntry {
                hash: 10,
                name: "a.7z".into(),
                size: 1024,
                state: None,
            },
            ArchiveEntry {
                hash: 20,
                name: "b.7z".into(),
                size: 2048,
                state: None,
            },
        ],
        directives: vec![
            RawDirective::FromArchive {
                archive_hash_path: vec![
                    serde_json::Value::Number(10.into()),
                    serde_json::Value::String("z-last.txt".into()),
                ],
                to: "mods/a/z-last.txt".into(),
                size: 0,
            },
            RawDirective::InlineFile {
                hash: 0,
                size: 1,
                source_data_id: "inline".into(),
                to: "mods/inline.txt".into(),
            },
            RawDirective::PatchedFromArchive {
                archive_hash_path: vec![
                    serde_json::Value::Number(20.into()),
                    serde_json::Value::String("only.txt".into()),
                ],
                to: "mods/b/only.txt".into(),
                hash: 0,
                patch_id: "patch".into(),
                size: 123,
            },
            RawDirective::FromArchive {
                archive_hash_path: vec![
                    serde_json::Value::Number(10.into()),
                    serde_json::Value::String("a-first.txt".into()),
                ],
                to: "mods/a/a-first.txt".into(),
                size: 0,
            },
            RawDirective::CreateBSA {
                temp_id: "temp".into(),
                to: "mods/out.bsa".into(),
                file_states: vec![],
            },
        ],
    };

    let batches = manifest.install_directives_grouped_by_archive();
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].archive_hash, 10);
    assert_eq!(batches[0].archive_size_bytes, 1024);
    assert_eq!(batches[0].directives.len(), 2);
    assert_eq!(batches[0].directives[0].directive_index, 3);
    assert!(matches!(
        &batches[0].directives[0].directive,
        InstallDirective::FromArchive { from, .. } if from == "a-first.txt"
    ));
    assert_eq!(batches[0].directives[1].directive_index, 0);
    assert!(matches!(
        &batches[1].directives[..],
        [IndexedInstallDirective {
            directive_index: 2,
            directive: InstallDirective::PatchedFromArchive {
                archive_hash: 20,
                size: 123,
                ..
            }
        }]
    ));
}
