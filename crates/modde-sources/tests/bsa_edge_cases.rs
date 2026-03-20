use modde_core::manifest::wabbajack::BSAFileState;
use modde_sources::wabbajack::bsa_repack::create_bsa;

// ── BSA with files that have very long paths ────────────────────────

#[tokio::test]
async fn test_bsa_very_long_path() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    // Build a deeply nested path with a total length over 200 chars
    let deep = "a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/z";
    let fs_path = staging.path().join(deep);
    tokio::fs::create_dir_all(&fs_path).await.unwrap();
    tokio::fs::write(fs_path.join("longname.dat"), b"long path data")
        .await
        .unwrap();

    let bsa_path = format!("{}\\longname.dat", deep.replace('/', "\\"));
    let states = vec![BSAFileState {
        path: bsa_path,
        hash: 0,
        size: 14,
    }];

    create_bsa(&states, staging.path(), &output).await.unwrap();
    assert!(output.exists());
    let data = std::fs::read(&output).unwrap();
    assert_eq!(&data[..4], b"BSA\0");
}

// ── BSA with ASCII-only filenames using mixed case and underscores ──

#[tokio::test]
async fn test_bsa_ascii_filenames() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    let dir = staging.path().join("data");
    tokio::fs::create_dir_all(&dir).await.unwrap();

    let filenames = vec![
        "UPPERCASE.TXT",
        "lowercase.txt",
        "MiXeD_CaSe.dat",
        "with-dashes.nif",
        "with_underscores.dds",
    ];

    let mut states = Vec::new();
    for name in &filenames {
        tokio::fs::write(dir.join(name), format!("content of {name}"))
            .await
            .unwrap();
        states.push(BSAFileState {
            path: format!("data\\{name}"),
            hash: 0,
            size: format!("content of {name}").len() as u64,
        });
    }

    create_bsa(&states, staging.path(), &output).await.unwrap();
    assert!(output.exists());
    let data = std::fs::read(&output).unwrap();
    assert!(data.len() > 36);
}

// ── BA2 with a single file ──────────────────────────────────────────

#[tokio::test]
async fn test_ba2_single_file() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.ba2");

    tokio::fs::write(staging.path().join("single.txt"), b"only file")
        .await
        .unwrap();

    let states = vec![BSAFileState {
        path: "single.txt".to_string(),
        hash: 0,
        size: 9,
    }];

    create_bsa(&states, staging.path(), &output).await.unwrap();

    let data = std::fs::read(&output).unwrap();
    assert_eq!(&data[..4], b"BTDX");

    // Header(24) + 1 record(36) + data(9) + name_table(2 + 10)
    // Verify the file count in the header is 1 (bytes 12..16)
    let file_count = u32::from_le_bytes(data[12..16].try_into().unwrap());
    assert_eq!(file_count, 1);
}

// ── BSA compression ratio: compressible vs incompressible data ──────

#[tokio::test]
async fn test_bsa_compression_ratio_compressible() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    let dir = staging.path().join("data");
    tokio::fs::create_dir_all(&dir).await.unwrap();

    // Highly compressible: repeated zeros
    let compressible = vec![0u8; 50_000];
    tokio::fs::write(dir.join("zeros.bin"), &compressible)
        .await
        .unwrap();

    let states = vec![BSAFileState {
        path: "data\\zeros.bin".to_string(),
        hash: 0,
        size: 50_000,
    }];

    create_bsa(&states, staging.path(), &output).await.unwrap();
    let bsa_data = std::fs::read(&output).unwrap();
    // Compressed BSA should be much smaller than 50KB
    assert!(
        bsa_data.len() < 50_000,
        "BSA with compressible data should compress; got {} bytes",
        bsa_data.len()
    );
}

#[tokio::test]
async fn test_bsa_compression_ratio_incompressible() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    let dir = staging.path().join("data");
    tokio::fs::create_dir_all(&dir).await.unwrap();

    // Already-compressed / random-like data (hard to compress further)
    // Use a pseudo-random sequence
    let incompressible: Vec<u8> = (0..1000).map(|i| ((i * 37 + 13) % 256) as u8).collect();
    tokio::fs::write(dir.join("random.bin"), &incompressible)
        .await
        .unwrap();

    let states = vec![BSAFileState {
        path: "data\\random.bin".to_string(),
        hash: 0,
        size: 1000,
    }];

    create_bsa(&states, staging.path(), &output).await.unwrap();
    let bsa_data = std::fs::read(&output).unwrap();
    // Should still succeed; data stored uncompressed when compression doesn't help
    assert!(bsa_data.len() > 36);
}

// ── Path normalization: forward slash to backslash ──────────────────

#[tokio::test]
async fn test_bsa_forward_slash_path_normalization() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    let dir = staging.path().join("textures/armor");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(dir.join("plate.dds"), b"dds data")
        .await
        .unwrap();

    // Use forward slashes in path -- should be normalized internally
    let states = vec![BSAFileState {
        path: "textures/armor/plate.dds".to_string(),
        hash: 0,
        size: 8,
    }];

    let result = create_bsa(&states, staging.path(), &output).await;
    assert!(result.is_ok());
    assert!(output.exists());
}

#[tokio::test]
async fn test_ba2_forward_slash_path_normalization() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.ba2");

    let dir = staging.path().join("meshes/weapons");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(dir.join("sword.nif"), b"nif data")
        .await
        .unwrap();

    // Forward slashes
    let states = vec![BSAFileState {
        path: "meshes/weapons/sword.nif".to_string(),
        hash: 0,
        size: 8,
    }];

    let result = create_bsa(&states, staging.path(), &output).await;
    assert!(result.is_ok());
}

// ── Multiple files in nested subdirectories verifying folder grouping ─

#[tokio::test]
async fn test_bsa_nested_subdirectory_folder_grouping() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    // Create files in multiple nested directories
    let dirs = vec![
        "meshes/armor/iron",
        "meshes/armor/steel",
        "textures/armor/iron",
        "textures/armor/steel",
    ];
    let mut states = Vec::new();

    for (i, dir) in dirs.iter().enumerate() {
        let fs_dir = staging.path().join(dir);
        tokio::fs::create_dir_all(&fs_dir).await.unwrap();
        let filename = format!("file_{i}.dat");
        let content = format!("content_{i}");
        tokio::fs::write(fs_dir.join(&filename), content.as_bytes())
            .await
            .unwrap();
        states.push(BSAFileState {
            path: format!("{}\\{}", dir.replace('/', "\\"), filename),
            hash: 0,
            size: content.len() as u64,
        });
    }

    create_bsa(&states, staging.path(), &output).await.unwrap();
    assert!(output.exists());

    let data = std::fs::read(&output).unwrap();
    assert_eq!(&data[..4], b"BSA\0");

    // Parse folder_count from header (offset 16, u32 LE)
    let folder_count = u32::from_le_bytes(data[16..20].try_into().unwrap());
    assert_eq!(folder_count, 4, "should have 4 distinct folders");

    // Parse file_count from header (offset 20, u32 LE)
    let file_count = u32::from_le_bytes(data[20..24].try_into().unwrap());
    assert_eq!(file_count, 4, "should have 4 files total");
}

#[tokio::test]
async fn test_bsa_files_in_same_deep_folder() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    let dir = staging.path().join("meshes/armor/dragonscale");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(dir.join("helmet.nif"), b"helmet")
        .await
        .unwrap();
    tokio::fs::write(dir.join("cuirass.nif"), b"cuirass")
        .await
        .unwrap();
    tokio::fs::write(dir.join("boots.nif"), b"boots")
        .await
        .unwrap();

    let states = vec![
        BSAFileState {
            path: "meshes\\armor\\dragonscale\\helmet.nif".to_string(),
            hash: 0,
            size: 6,
        },
        BSAFileState {
            path: "meshes\\armor\\dragonscale\\cuirass.nif".to_string(),
            hash: 0,
            size: 7,
        },
        BSAFileState {
            path: "meshes\\armor\\dragonscale\\boots.nif".to_string(),
            hash: 0,
            size: 5,
        },
    ];

    create_bsa(&states, staging.path(), &output).await.unwrap();

    let data = std::fs::read(&output).unwrap();
    let folder_count = u32::from_le_bytes(data[16..20].try_into().unwrap());
    assert_eq!(folder_count, 1, "all files in one folder");

    let file_count = u32::from_le_bytes(data[20..24].try_into().unwrap());
    assert_eq!(file_count, 3, "three files total");
}

// ── BA2 header structure verification ───────────────────────────────

#[tokio::test]
async fn test_ba2_header_structure() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.ba2");

    let dir = staging.path().join("data");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(dir.join("a.txt"), b"alpha").await.unwrap();
    tokio::fs::write(dir.join("b.txt"), b"bravo").await.unwrap();

    let states = vec![
        BSAFileState {
            path: "data\\a.txt".to_string(),
            hash: 0,
            size: 5,
        },
        BSAFileState {
            path: "data\\b.txt".to_string(),
            hash: 0,
            size: 5,
        },
    ];

    create_bsa(&states, staging.path(), &output).await.unwrap();

    let data = std::fs::read(&output).unwrap();
    // Magic
    assert_eq!(&data[..4], b"BTDX");
    // Version (u32 LE at offset 4)
    let version = u32::from_le_bytes(data[4..8].try_into().unwrap());
    assert_eq!(version, 1);
    // Type (4 bytes at offset 8)
    assert_eq!(&data[8..12], b"GNRL");
    // File count (u32 LE at offset 12)
    let file_count = u32::from_le_bytes(data[12..16].try_into().unwrap());
    assert_eq!(file_count, 2);
}
