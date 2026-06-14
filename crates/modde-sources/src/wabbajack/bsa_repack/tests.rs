use super::*;
use std::io::Read;

#[tokio::test]
async fn test_create_bsa_empty_errors() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");
    let result = create_bsa(&[], staging.path(), &output).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_create_bsa_writes_magic() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    // Create a dummy file in staging
    let file_dir = staging.path().join("meshes");
    tokio::fs::create_dir_all(&file_dir).await.unwrap();
    tokio::fs::write(file_dir.join("test.nif"), b"fake nif data")
        .await
        .unwrap();

    let states = vec![BSAFileState {
        path: "meshes\\test.nif".to_string(),
        hash: 0,
        size: 13,
    }];

    create_bsa(&states, staging.path(), &output).await.unwrap();

    let mut f = std::fs::File::open(&output).unwrap();
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, BSA_MAGIC);
}

#[tokio::test]
async fn test_create_ba2_writes_magic() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.ba2");

    let file_dir = staging.path().join("data");
    tokio::fs::create_dir_all(&file_dir).await.unwrap();
    tokio::fs::write(file_dir.join("test.txt"), b"test content")
        .await
        .unwrap();

    let states = vec![BSAFileState {
        path: "data\\test.txt".to_string(),
        hash: 0,
        size: 12,
    }];

    create_bsa(&states, staging.path(), &output).await.unwrap();

    let mut f = std::fs::File::open(&output).unwrap();
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, BA2_MAGIC);
}

#[test]
fn test_bsa_hash_not_zero() {
    let hash = bsa_hash_file("test.nif");
    assert_ne!(hash, 0);
}

#[test]
fn test_bsa_hash_deterministic() {
    let h1 = bsa_hash_file("meshes\\armor.nif");
    let h2 = bsa_hash_file("meshes\\armor.nif");
    assert_eq!(h1, h2);
}

#[test]
fn test_bsa_hash_empty_string() {
    assert_eq!(bsa_hash_path(""), 0);
    assert_eq!(bsa_hash_file(""), 0);
    assert_eq!(bsa_hash_folder(""), 0);
}

#[test]
fn test_bsa_hash_case_insensitive() {
    let h1 = bsa_hash_file("Test.Nif");
    let h2 = bsa_hash_file("test.nif");
    assert_eq!(h1, h2);
}

#[test]
fn test_bsa_hash_extension_nif_adjusts() {
    let with_ext = bsa_hash_path("model.nif");
    let without_ext = bsa_hash_path("model");
    assert_ne!(with_ext, without_ext);
}

#[test]
fn test_bsa_hash_extension_dds_adjusts() {
    let with_ext = bsa_hash_path("texture.dds");
    let without_ext = bsa_hash_path("texture");
    assert_ne!(with_ext, without_ext);
}

#[test]
fn test_bsa_hash_extension_wav_adjusts() {
    let with_ext = bsa_hash_path("sound.wav");
    let without_ext = bsa_hash_path("sound");
    assert_ne!(with_ext, without_ext);
}

#[test]
fn test_bsa_hash_extension_kf_adjusts() {
    let with_ext = bsa_hash_path("anim.kf");
    let without_ext = bsa_hash_path("anim");
    assert_ne!(with_ext, without_ext);
}

#[test]
fn test_ba2_crc32_empty() {
    assert_eq!(ba2_crc32(b""), 0);
}

#[test]
fn test_ba2_crc32_deterministic() {
    let h1 = ba2_crc32(b"hello world");
    let h2 = ba2_crc32(b"hello world");
    assert_eq!(h1, h2);
}

#[test]
fn test_ba2_crc32_case_insensitive() {
    let h1 = ba2_crc32(b"ABC");
    let h2 = ba2_crc32(b"abc");
    assert_eq!(h1, h2);
}

#[tokio::test]
async fn test_create_bsa_multiple_files_in_folder() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    let file_dir = staging.path().join("meshes");
    tokio::fs::create_dir_all(&file_dir).await.unwrap();
    tokio::fs::write(file_dir.join("armor.nif"), b"armor data")
        .await
        .unwrap();
    tokio::fs::write(file_dir.join("weapon.nif"), b"weapon data")
        .await
        .unwrap();

    let states = vec![
        BSAFileState {
            path: "meshes\\armor.nif".to_string(),
            hash: 0,
            size: 10,
        },
        BSAFileState {
            path: "meshes\\weapon.nif".to_string(),
            hash: 0,
            size: 11,
        },
    ];

    create_bsa(&states, staging.path(), &output).await.unwrap();
    assert!(output.exists());

    let mut f = std::fs::File::open(&output).unwrap();
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, BSA_MAGIC);
}

#[tokio::test]
async fn test_create_bsa_multiple_folders() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    let meshes_dir = staging.path().join("meshes");
    let textures_dir = staging.path().join("textures");
    tokio::fs::create_dir_all(&meshes_dir).await.unwrap();
    tokio::fs::create_dir_all(&textures_dir).await.unwrap();
    tokio::fs::write(meshes_dir.join("test.nif"), b"nif data")
        .await
        .unwrap();
    tokio::fs::write(textures_dir.join("test.dds"), b"dds data")
        .await
        .unwrap();

    let states = vec![
        BSAFileState {
            path: "meshes\\test.nif".to_string(),
            hash: 0,
            size: 8,
        },
        BSAFileState {
            path: "textures\\test.dds".to_string(),
            hash: 0,
            size: 8,
        },
    ];

    create_bsa(&states, staging.path(), &output).await.unwrap();
    assert!(output.exists());

    let data = std::fs::read(&output).unwrap();
    assert!(data.len() > 36); // header + at least some content
}

#[tokio::test]
async fn test_create_bsa_round_trips_through_archive_index() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    tokio::fs::create_dir_all(staging.path().join("textures"))
        .await
        .unwrap();
    tokio::fs::create_dir_all(staging.path().join("meshes"))
        .await
        .unwrap();

    let texture_data = vec![b'x'; 4096];
    let mesh_data = b"mesh data that should remain uncompressed".to_vec();
    tokio::fs::write(staging.path().join("textures/sky.dds"), &texture_data)
        .await
        .unwrap();
    tokio::fs::write(staging.path().join("meshes/tree.nif"), &mesh_data)
        .await
        .unwrap();

    // Deliberately not grouped by folder; the writer must keep record,
    // filename, and data ordering aligned after folder sorting.
    let states = vec![
        BSAFileState {
            path: "textures\\sky.dds".to_string(),
            hash: 0,
            size: texture_data.len() as u64,
        },
        BSAFileState {
            path: "meshes\\tree.nif".to_string(),
            hash: 0,
            size: mesh_data.len() as u64,
        },
    ];

    create_bsa(&states, staging.path(), &output).await.unwrap();

    let index = modde_core::bethesda_archive::ArchiveIndex::read(&output).unwrap();
    assert_eq!(
        index.extract_file("textures/sky.dds").unwrap(),
        texture_data
    );
    assert_eq!(index.extract_file("meshes/tree.nif").unwrap(), mesh_data);
}

#[tokio::test]
async fn test_create_bsa_missing_file_uses_empty() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    // Don't create the file in staging - it should still succeed with empty data
    let states = vec![BSAFileState {
        path: "meshes\\missing.nif".to_string(),
        hash: 0,
        size: 100,
    }];

    let result = create_bsa(&states, staging.path(), &output).await;
    assert!(result.is_ok());
    assert!(output.exists());
}

#[tokio::test]
async fn test_create_ba2_multiple_files() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.ba2");

    let data_dir = staging.path().join("data");
    tokio::fs::create_dir_all(&data_dir).await.unwrap();
    tokio::fs::write(data_dir.join("file1.txt"), b"content one")
        .await
        .unwrap();
    tokio::fs::write(data_dir.join("file2.txt"), b"content two")
        .await
        .unwrap();

    let states = vec![
        BSAFileState {
            path: "data\\file1.txt".to_string(),
            hash: 0,
            size: 11,
        },
        BSAFileState {
            path: "data\\file2.txt".to_string(),
            hash: 0,
            size: 11,
        },
    ];

    create_bsa(&states, staging.path(), &output).await.unwrap();

    let mut f = std::fs::File::open(&output).unwrap();
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, BA2_MAGIC);
}

#[tokio::test]
async fn test_create_ba2_round_trips_through_archive_index() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.ba2");

    tokio::fs::create_dir_all(staging.path().join("data"))
        .await
        .unwrap();
    tokio::fs::write(staging.path().join("data/file1.txt"), b"content one")
        .await
        .unwrap();
    tokio::fs::write(staging.path().join("data/file2.txt"), b"content two")
        .await
        .unwrap();

    let states = vec![
        BSAFileState {
            path: "data\\file1.txt".to_string(),
            hash: 0,
            size: 11,
        },
        BSAFileState {
            path: "data\\file2.txt".to_string(),
            hash: 0,
            size: 11,
        },
    ];

    create_bsa(&states, staging.path(), &output).await.unwrap();
    let index = modde_core::bethesda_archive::ArchiveIndex::read(&output).unwrap();
    assert_eq!(
        index.extract_file("data/file1.txt").unwrap(),
        b"content one"
    );
    assert_eq!(
        index.extract_file("data/file2.txt").unwrap(),
        b"content two"
    );
}

#[tokio::test]
async fn test_create_bsa_forward_slash_normalization() {
    let staging = tempfile::tempdir().unwrap();
    let output = staging.path().join("test.bsa");

    let file_dir = staging.path().join("meshes");
    tokio::fs::create_dir_all(&file_dir).await.unwrap();
    tokio::fs::write(file_dir.join("test.nif"), b"data")
        .await
        .unwrap();

    // Use forward slashes in the path - should be normalized to backslashes
    let states = vec![BSAFileState {
        path: "meshes/test.nif".to_string(),
        hash: 0,
        size: 4,
    }];

    let result = create_bsa(&states, staging.path(), &output).await;
    assert!(result.is_ok());
    assert!(output.exists());
}

mod hash;
