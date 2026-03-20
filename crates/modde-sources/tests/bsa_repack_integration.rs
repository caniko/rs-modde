use modde_core::manifest::wabbajack::BSAFileState;
use modde_sources::wabbajack::bsa_repack::create_bsa;

// ── BSA creation integration tests ──────────────────────────────────

#[tokio::test]
async fn test_create_bsa_deeply_nested_path() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    // Create deeply nested directory structure
    let deep_path = staging.path().join("textures/landscape/snow/detail");
    tokio::fs::create_dir_all(&deep_path).await.unwrap();
    tokio::fs::write(deep_path.join("snow01.dds"), b"snow texture data").await.unwrap();

    let states = vec![BSAFileState {
        path: "textures\\landscape\\snow\\detail\\snow01.dds".to_string(),
        hash: 0,
        size: 17,
    }];

    create_bsa(&states, staging.path(), &output).await.unwrap();
    assert!(output.exists());

    let data = std::fs::read(&output).unwrap();
    assert!(data.len() > 36, "BSA should have at least a header");
    assert_eq!(&data[..4], b"BSA\0");
}

#[tokio::test]
async fn test_create_bsa_root_level_file() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    // File at root level (no subdirectory)
    tokio::fs::write(staging.path().join("plugin.esp"), b"plugin data").await.unwrap();

    let states = vec![BSAFileState {
        path: "plugin.esp".to_string(),
        hash: 0,
        size: 11,
    }];

    create_bsa(&states, staging.path(), &output).await.unwrap();
    assert!(output.exists());
}

#[tokio::test]
async fn test_create_ba2_deeply_nested_path() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.ba2");

    let deep_path = staging.path().join("meshes/architecture/whiterun");
    tokio::fs::create_dir_all(&deep_path).await.unwrap();
    tokio::fs::write(deep_path.join("wall.nif"), b"mesh data").await.unwrap();

    let states = vec![BSAFileState {
        path: "meshes\\architecture\\whiterun\\wall.nif".to_string(),
        hash: 0,
        size: 9,
    }];

    create_bsa(&states, staging.path(), &output).await.unwrap();
    assert!(output.exists());

    let data = std::fs::read(&output).unwrap();
    assert_eq!(&data[..4], b"BTDX");
}

#[tokio::test]
async fn test_create_bsa_many_files_same_folder() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    let mesh_dir = staging.path().join("meshes");
    tokio::fs::create_dir_all(&mesh_dir).await.unwrap();

    let mut states = Vec::new();
    for i in 0..20 {
        let filename = format!("model_{i}.nif");
        tokio::fs::write(mesh_dir.join(&filename), format!("data_{i}")).await.unwrap();
        states.push(BSAFileState {
            path: format!("meshes\\{filename}"),
            hash: 0,
            size: format!("data_{i}").len() as u64,
        });
    }

    create_bsa(&states, staging.path(), &output).await.unwrap();
    assert!(output.exists());
    let data = std::fs::read(&output).unwrap();
    assert!(data.len() > 100, "BSA with 20 files should be substantial");
}

#[tokio::test]
async fn test_create_bsa_many_folders() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    let mut states = Vec::new();
    for folder in &["meshes", "textures", "sounds", "scripts", "interface"] {
        let dir = staging.path().join(folder);
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let filename = format!("{folder}_data.dat");
        tokio::fs::write(dir.join(&filename), format!("{folder} content")).await.unwrap();
        states.push(BSAFileState {
            path: format!("{folder}\\{filename}"),
            hash: 0,
            size: format!("{folder} content").len() as u64,
        });
    }

    create_bsa(&states, staging.path(), &output).await.unwrap();
    assert!(output.exists());
}

#[tokio::test]
async fn test_create_ba2_empty_errors() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.ba2");
    let result = create_bsa(&[], staging.path(), &output).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_create_bsa_creates_parent_dirs() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("deeply/nested/output/test.bsa");

    tokio::fs::write(staging.path().join("file.txt"), b"data").await.unwrap();

    let states = vec![BSAFileState {
        path: "file.txt".to_string(),
        hash: 0,
        size: 4,
    }];

    create_bsa(&states, staging.path(), &output).await.unwrap();
    assert!(output.exists());
}

#[tokio::test]
async fn test_create_bsa_large_file_compresses() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    // Create a large compressible file (repeated pattern)
    let data = "A".repeat(100_000);
    tokio::fs::create_dir_all(staging.path().join("data")).await.unwrap();
    tokio::fs::write(staging.path().join("data/large.txt"), data.as_bytes()).await.unwrap();

    let states = vec![BSAFileState {
        path: "data\\large.txt".to_string(),
        hash: 0,
        size: 100_000,
    }];

    create_bsa(&states, staging.path(), &output).await.unwrap();
    let bsa_data = std::fs::read(&output).unwrap();
    // Compressed BSA should be smaller than 100KB raw + header
    assert!(
        bsa_data.len() < 100_000,
        "BSA with compressible data should be compressed (got {} bytes)",
        bsa_data.len()
    );
}

// ── BSA/BA2 format detection ────────────────────────────────────────

#[tokio::test]
async fn test_extension_detection_bsa() {
    let staging = tempfile::tempdir().unwrap();
    tokio::fs::write(staging.path().join("file.dat"), b"test").await.unwrap();
    let states = vec![BSAFileState { path: "file.dat".to_string(), hash: 0, size: 4 }];

    let bsa_output = staging.path().join("test.bsa");
    create_bsa(&states, staging.path(), &bsa_output).await.unwrap();
    let bsa_data = std::fs::read(&bsa_output).unwrap();
    assert_eq!(&bsa_data[..4], b"BSA\0", "should be BSA format");
}

#[tokio::test]
async fn test_extension_detection_ba2() {
    let staging = tempfile::tempdir().unwrap();
    tokio::fs::write(staging.path().join("file.dat"), b"test").await.unwrap();
    let states = vec![BSAFileState { path: "file.dat".to_string(), hash: 0, size: 4 }];

    let ba2_output = staging.path().join("test.ba2");
    create_bsa(&states, staging.path(), &ba2_output).await.unwrap();
    let ba2_data = std::fs::read(&ba2_output).unwrap();
    assert_eq!(&ba2_data[..4], b"BTDX", "should be BA2 format");
}

#[tokio::test]
async fn test_unknown_extension_defaults_to_bsa() {
    let staging = tempfile::tempdir().unwrap();
    tokio::fs::write(staging.path().join("file.dat"), b"test").await.unwrap();
    let states = vec![BSAFileState { path: "file.dat".to_string(), hash: 0, size: 4 }];

    let unknown_output = staging.path().join("test.archive");
    create_bsa(&states, staging.path(), &unknown_output).await.unwrap();
    let data = std::fs::read(&unknown_output).unwrap();
    assert_eq!(&data[..4], b"BSA\0", "unknown extension should default to BSA");
}
