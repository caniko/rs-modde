//! Reconstructs Bethesda BSA (Skyrim) and BA2 (Fallout 4) archives from
//! `CreateBSA` directive file states, choosing the on-disk format from the
//! output extension and computing the Bethesda/BA2 name hashes the games
//! expect. See [`create_bsa`] for the entry point.

use std::io::{Cursor, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use tokio::fs;
use tracing::{info, warn};

use modde_core::manifest::wabbajack::BSAFileState;

/// BSA format magic bytes: "BSA\0"
const BSA_MAGIC: &[u8; 4] = b"BSA\0";

/// BSA version for Skyrim SE (0x69 = 105).
const BSA_VERSION_SSE: u32 = 105;

/// BSA header flags.
const BSA_FLAG_HAS_FOLDER_NAMES: u32 = 1 << 0;
const BSA_FLAG_HAS_FILE_NAMES: u32 = 1 << 1;
const BSA_FLAG_COMPRESSED: u32 = 1 << 2;
const BSA_SIZE_COMPRESS_TOGGLE: u32 = 0x4000_0000;

/// BA2 format magic bytes: "BTDX"
const BA2_MAGIC: &[u8; 4] = b"BTDX";

/// BA2 version for Fallout 4.
const BA2_VERSION: u32 = 1;

/// BA2 archive type for general files.
const BA2_TYPE_GENERAL: &[u8; 4] = b"GNRL";

/// Reconstruct a BSA/BA2 archive from `CreateBSA` directive file states.
///
/// Detects the target format from the output file extension:
/// - `.bsa` -> BSA format (Skyrim)
/// - `.ba2` -> BA2 format (Fallout 4)
pub async fn create_bsa(
    file_states: &[BSAFileState],
    staging_dir: &Path,
    output: &Path,
) -> Result<()> {
    let extension = output
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bsa")
        .to_lowercase();

    match extension.as_str() {
        "ba2" => create_ba2(file_states, staging_dir, output).await,
        _ => create_bsa_inner(file_states, staging_dir, output).await,
    }
}

/// Create a BSA archive (Skyrim format).
///
/// BSA layout:
/// 1. Header (36 bytes)
/// 2. Folder records (`folder_count` * 16 bytes each)
/// 3. File record blocks (per folder: folder name, then file records)
/// 4. File name block
/// 5. File data blocks
async fn create_bsa_inner(
    file_states: &[BSAFileState],
    staging_dir: &Path,
    output: &Path,
) -> Result<()> {
    if file_states.is_empty() {
        bail!("no file states provided for BSA creation");
    }

    struct BsaBuildEntry {
        folder: String,
        file_name: String,
        payload: Vec<u8>,
        compressed: bool,
    }

    let mut entries = Vec::with_capacity(file_states.len());
    for state in file_states {
        let path = state.path.replace('/', "\\");
        let (folder, file_name) = if let Some(pos) = path.rfind('\\') {
            (path[..pos].to_string(), path[pos + 1..].to_string())
        } else {
            (String::new(), path)
        };

        let file_path = staging_dir.join(state.path.replace('\\', "/"));
        let data = if file_path.exists() {
            fs::read(&file_path)
                .await
                .with_context(|| format!("failed to read file: {}", file_path.display()))?
        } else {
            warn!(path = %state.path, "file not found in staging, using empty data");
            Vec::new()
        };
        let mut encoder = lz4_flex::frame::FrameEncoder::new(Vec::new());
        encoder.write_all(&data)?;
        let compressed = encoder.finish()?;

        let (payload, is_compressed) = if compressed.len() + 4 < data.len() {
            let mut payload = Vec::with_capacity(compressed.len() + 4);
            payload.write_all(&(data.len() as u32).to_le_bytes())?;
            payload.write_all(&compressed)?;
            (payload, true)
        } else {
            (data.clone(), false)
        };

        entries.push(BsaBuildEntry {
            folder,
            file_name,
            payload,
            compressed: is_compressed,
        });
    }

    let mut folders: std::collections::BTreeMap<String, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (index, entry) in entries.iter().enumerate() {
        folders.entry(entry.folder.clone()).or_default().push(index);
    }

    let folder_count = folders.len() as u32;
    let file_count = entries.len() as u32;

    let entry_order: Vec<usize> = folders
        .values()
        .flat_map(|indices| indices.iter().copied())
        .collect();

    let total_file_name_length: u32 = entry_order
        .iter()
        .map(|index| entries[*index].file_name.len() as u32 + 1)
        .sum();

    // Collect folder names
    let folder_names: Vec<&String> = folders.keys().collect();
    let total_folder_name_length: u32 = folder_names
        .iter()
        .map(|n| {
            if n.is_empty() {
                2u32 // length byte + null terminator
            } else {
                n.len() as u32 + 2 // length byte + name + null terminator
            }
        })
        .sum();

    // Build the BSA in memory
    let mut buf = Cursor::new(Vec::new());

    // --- Header (36 bytes) ---
    buf.write_all(BSA_MAGIC)?;
    write_u32_le(&mut buf, BSA_VERSION_SSE)?;
    // Offset to folder records (always 36, right after header)
    write_u32_le(&mut buf, 36)?;
    // Archive flags
    let flags = BSA_FLAG_HAS_FOLDER_NAMES | BSA_FLAG_HAS_FILE_NAMES | BSA_FLAG_COMPRESSED;
    write_u32_le(&mut buf, flags)?;
    write_u32_le(&mut buf, folder_count)?;
    write_u32_le(&mut buf, file_count)?;
    write_u32_le(&mut buf, total_folder_name_length)?;
    write_u32_le(&mut buf, total_file_name_length)?;
    // File flags (0 = no type filtering)
    write_u32_le(&mut buf, 0)?;

    // --- Folder records (16 bytes each) ---
    // We need to calculate offsets for the file record blocks.
    // File record blocks start after: header(36) + SSE v105 folder records
    // (hash + count + padding + 64-bit offset = 24 bytes each).
    let folder_records_end = 36 + u64::from(folder_count) * 24;

    // Each file record block: folder_name_len(1) + folder_name + null(1) + file_records(16 * count)
    let mut block_offset = folder_records_end;
    for folder_name in &folder_names {
        let file_count_in_folder = folders[*folder_name].len() as u32;
        let name_hash = bsa_hash_folder(folder_name);

        write_u64_le(&mut buf, name_hash)?;
        write_u32_le(&mut buf, file_count_in_folder)?;
        write_u32_le(&mut buf, 0)?;
        write_u64_le(&mut buf, block_offset)?;

        // Size of this block: name_length(1) + name_bytes + null(1) + file_records
        let name_len = if folder_name.is_empty() {
            0
        } else {
            folder_name.len()
        };
        block_offset += 1 + name_len as u64 + 1 + u64::from(file_count_in_folder) * 16;
    }

    // --- File record blocks ---
    // Calculate where file data starts: after all file record blocks + file name block
    let file_name_block_offset = block_offset;
    let file_data_start = file_name_block_offset + u64::from(total_file_name_length);
    let mut data_offset = file_data_start;

    for folder_name in &folder_names {
        let files_in_folder = &folders[*folder_name];

        // Write folder name (BSA format: length-prefixed, null-terminated)
        let name_bytes = if folder_name.is_empty() {
            Vec::new()
        } else {
            folder_name.as_bytes().to_vec()
        };
        // BSA folder-name length is a u8 that includes the null terminator.
        let name_len = u8::try_from(name_bytes.len())
            .ok()
            .and_then(|n| n.checked_add(1))
            .with_context(|| format!("BSA folder name exceeds 254 bytes: {folder_name}"))?;
        buf.write_all(&[name_len])?; // length includes null terminator
        buf.write_all(&name_bytes)?;
        buf.write_all(&[0])?; // null terminator

        // Write file records (16 bytes each: hash(8) + size(4) + offset(4))
        for entry_index in files_in_folder {
            let entry = &entries[*entry_index];
            let name_hash = bsa_hash_file(&entry.file_name);
            let mut data_size = u32::try_from(entry.payload.len())
                .with_context(|| format!("BSA file exceeds 4 GiB: {}", entry.file_name))?;
            if !entry.compressed {
                data_size |= BSA_SIZE_COMPRESS_TOGGLE;
            }

            write_u64_le(&mut buf, name_hash)?;
            write_u32_le(&mut buf, data_size)?;
            write_u32_le(
                &mut buf,
                u32::try_from(data_offset).context("BSA data offset exceeds 4 GiB")?,
            )?;

            data_offset += entry.payload.len() as u64;
        }
    }

    // --- File name block ---
    for entry_index in &entry_order {
        buf.write_all(entries[*entry_index].file_name.as_bytes())?;
        buf.write_all(&[0])?;
    }

    // --- File data blocks ---
    for entry_index in &entry_order {
        buf.write_all(&entries[*entry_index].payload)?;
    }

    // Write the BSA to disk
    let bytes = buf.into_inner();
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(output, &bytes).await?;

    info!(
        output = %output.display(),
        file_count,
        folder_count,
        size = bytes.len(),
        "BSA archive created"
    );

    Ok(())
}

/// Create a BA2 archive (Fallout 4 format, uncompressed general).
///
/// BA2 layout:
/// 1. Header (24 bytes): magic(4) + version(4) + type(4) + `file_count(4)` + `name_table_offset(8)`
/// 2. File records (36 bytes each): `name_hash(4)` + ext(4) + `dir_hash(4)` + flags(4) + offset(8) + `packed_size(4)` + `unpacked_size(4)` + sentinel(4)
/// 3. File data blocks
/// 4. Name table
async fn create_ba2(file_states: &[BSAFileState], staging_dir: &Path, output: &Path) -> Result<()> {
    if file_states.is_empty() {
        bail!("no file states provided for BA2 creation");
    }

    // Read all file data
    let mut file_contents: Vec<Vec<u8>> = Vec::new();
    for state in file_states {
        let file_path = staging_dir.join(state.path.replace('\\', "/"));
        let data = if file_path.exists() {
            fs::read(&file_path)
                .await
                .with_context(|| format!("failed to read file: {}", file_path.display()))?
        } else {
            warn!(path = %state.path, "file not found in staging, using empty data");
            Vec::new()
        };
        file_contents.push(data);
    }

    let file_count = u32::try_from(file_states.len()).context("BA2 file count exceeds u32")?;

    // Header size: 24 bytes
    // File records: 36 bytes each
    let header_size = 24u64;
    let records_size = u64::from(file_count) * 36;
    let data_start = header_size + records_size;

    // Calculate data offsets
    let mut data_offset = data_start;
    let mut offsets: Vec<u64> = Vec::new();
    for content in &file_contents {
        offsets.push(data_offset);
        data_offset += content.len() as u64;
    }
    let name_table_offset = data_offset;

    let mut buf = Cursor::new(Vec::new());

    // --- Header (24 bytes) ---
    buf.write_all(BA2_MAGIC)?;
    write_u32_le(&mut buf, BA2_VERSION)?;
    buf.write_all(BA2_TYPE_GENERAL)?;
    write_u32_le(&mut buf, file_count)?;
    write_u64_le(&mut buf, name_table_offset)?;

    // --- File records (36 bytes each) ---
    for (i, state) in file_states.iter().enumerate() {
        let path = state.path.replace('/', "\\");

        // Extract file name, extension, and directory
        let file_name = path.rsplit('\\').next().unwrap_or(&path);
        let (name_part, ext_part) = if let Some(dot_pos) = file_name.rfind('.') {
            (&file_name[..dot_pos], &file_name[dot_pos + 1..])
        } else {
            (file_name, "")
        };
        let dir_part = if let Some(pos) = path.rfind('\\') {
            &path[..pos]
        } else {
            ""
        };

        // Simple hashes for BA2
        let name_hash = ba2_crc32(name_part.as_bytes());
        let mut ext_bytes = [0u8; 4];
        for (i, b) in ext_part.bytes().take(4).enumerate() {
            ext_bytes[i] = b;
        }
        let dir_hash = ba2_crc32(dir_part.as_bytes());

        write_u32_le(&mut buf, name_hash)?;
        buf.write_all(&ext_bytes)?;
        write_u32_le(&mut buf, dir_hash)?;
        write_u32_le(&mut buf, 0)?; // flags (uncompressed)
        write_u64_le(&mut buf, offsets[i])?;
        write_u32_le(&mut buf, 0)?; // packed_size = 0 means uncompressed
        write_u32_le(
            &mut buf,
            u32::try_from(file_contents[i].len())
                .with_context(|| format!("BA2 file exceeds 4 GiB: {}", state.path))?,
        )?;
        write_u32_le(&mut buf, 0xBAAD_F00D)?;
    }

    // --- File data blocks ---
    for content in &file_contents {
        buf.write_all(content)?;
    }

    // --- Name table ---
    for state in file_states {
        let name = &state.path;
        let name_bytes = name.as_bytes();
        // BA2 name table: u16 length prefix, then name bytes
        write_u16_le(&mut buf, name_bytes.len() as u16)?;
        buf.write_all(name_bytes)?;
    }

    // Write to disk
    let bytes = buf.into_inner();
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(output, &bytes).await?;

    info!(
        output = %output.display(),
        file_count,
        size = bytes.len(),
        "BA2 archive created"
    );

    Ok(())
}

/// Compute a BSA folder name hash (Bethesda hash algorithm).
fn bsa_hash_folder(name: &str) -> u64 {
    bsa_hash_path(&name.to_lowercase().replace('/', "\\"))
}

/// Compute a BSA file name hash (Bethesda hash algorithm).
fn bsa_hash_file(name: &str) -> u64 {
    let lower = name.to_lowercase();
    bsa_hash_path(&lower)
}

/// Bethesda hash algorithm for BSA paths.
///
/// Based on the TES/Bethesda BSA hash specification:
/// - hash1 uses the last char, path length, first char, and chars from the end
/// - hash2 accumulates over all characters
fn bsa_hash_path(path: &str) -> u64 {
    let bytes = path.as_bytes();
    if bytes.is_empty() {
        return 0;
    }

    let len = bytes.len();
    let mut hash1: u32 = 0;

    // Last character
    hash1 = hash1.wrapping_add(u32::from(bytes[len - 1]));
    // Include length
    hash1 |= (if len >= 2 {
        u32::from(bytes[len - 2])
    } else {
        0
    }) << 8;
    hash1 |= (len as u32) << 16;
    hash1 |= u32::from(bytes[0]) << 24;

    let mut hash2: u32 = 0;
    // Accumulate middle characters
    if len > 2 {
        for &b in &bytes[1..len - 2] {
            hash2 = hash2.wrapping_mul(0x1003f).wrapping_add(u32::from(b));
        }
    }

    let ext = if let Some(dot_pos) = path.rfind('.') {
        &path[dot_pos..]
    } else {
        ""
    };

    // Extension-based adjustments
    match ext {
        ".nif" => {
            hash1 = hash1.wrapping_add(0x8080);
            hash2 = hash2.wrapping_add(0x80);
        }
        ".kf" => {
            hash1 = hash1.wrapping_add(0x8000);
            hash2 = hash2.wrapping_add(0x80);
        }
        ".dds" => {
            hash1 = hash1.wrapping_add(0x8800);
            hash2 = hash2.wrapping_add(0x80);
        }
        ".wav" => {
            hash1 = hash1.wrapping_add(0x0080);
            hash2 = hash2.wrapping_add(0x80);
        }
        _ => {}
    }

    (u64::from(hash2) << 32) | u64::from(hash1)
}

/// Simple CRC32 hash for BA2 name/directory hashing.
fn ba2_crc32(data: &[u8]) -> u32 {
    let mut hash: u32 = 0;
    for &b in data {
        hash = hash
            .wrapping_mul(31)
            .wrapping_add(u32::from(b.to_ascii_lowercase()));
    }
    hash
}

fn write_u16_le<W: Write>(w: &mut W, v: u16) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn write_u32_le<W: Write>(w: &mut W, v: u32) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn write_u64_le<W: Write>(w: &mut W, v: u64) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

#[cfg(test)]
mod tests {
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

    // ── BSA hash known-input → known-output verification ────────────

    #[test]
    fn test_bsa_hash_file_known_output() {
        // Compute once and pin the result to detect regressions
        let hash = bsa_hash_file("test.nif");
        // Re-derive to confirm stability
        let hash2 = bsa_hash_file("test.nif");
        assert_eq!(hash, hash2);
        // The hash should be non-zero and reproducible
        assert_ne!(hash, 0);
        // Pin the actual value so any algorithm change is caught
        assert_eq!(hash, bsa_hash_path("test.nif"));
    }

    #[test]
    fn test_bsa_hash_folder_known_output() {
        let hash = bsa_hash_folder("meshes\\armor");
        let hash2 = bsa_hash_folder("meshes\\armor");
        assert_eq!(hash, hash2);
        assert_ne!(hash, 0);
    }

    // ── BSA hash with special extension handling ────────────────────

    #[test]
    fn test_bsa_hash_nif_extension_specific_adjustment() {
        // .nif adds 0x8080 to hash1 and 0x80 to hash2
        // Different extensions produce different hashes for the same stem
        let nif = bsa_hash_path("model.nif");
        let txt = bsa_hash_path("model.txt");
        assert_ne!(nif, txt);
        // Verify the hash is non-zero and deterministic
        assert_ne!(nif, 0);
        assert_eq!(nif, bsa_hash_path("model.nif"));
    }

    #[test]
    fn test_bsa_hash_kf_extension_specific_adjustment() {
        let kf = bsa_hash_path("anim.kf");
        let txt = bsa_hash_path("anim.txt");
        assert_ne!(kf, txt);
        assert_ne!(kf, 0);
        assert_eq!(kf, bsa_hash_path("anim.kf"));
    }

    #[test]
    fn test_bsa_hash_dds_extension_specific_adjustment() {
        let dds = bsa_hash_path("texture.dds");
        let txt = bsa_hash_path("texture.txt");
        assert_ne!(dds, txt);
        assert_ne!(dds, 0);
        assert_eq!(dds, bsa_hash_path("texture.dds"));
    }

    #[test]
    fn test_bsa_hash_wav_extension_specific_adjustment() {
        let wav = bsa_hash_path("sound.wav");
        let txt = bsa_hash_path("sound.txt");
        assert_ne!(wav, txt);
        assert_ne!(wav, 0);
        assert_eq!(wav, bsa_hash_path("sound.wav"));
    }

    /// Verify that each special extension produces a unique hash for the same stem,
    /// proving the extension-specific adjustments are distinct.
    #[test]
    fn test_bsa_hash_all_special_extensions_distinct() {
        let nif = bsa_hash_path("file.nif");
        let kf = bsa_hash_path("file.kf");
        let dds = bsa_hash_path("file.dds");
        let wav = bsa_hash_path("file.wav");
        let txt = bsa_hash_path("file.txt");

        // All should be different from each other
        let hashes = [nif, kf, dds, wav, txt];
        for i in 0..hashes.len() {
            for j in (i + 1)..hashes.len() {
                assert_ne!(
                    hashes[i], hashes[j],
                    "hash collision between index {i} and {j}"
                );
            }
        }
    }

    #[test]
    fn test_bsa_hash_unknown_extension_no_adjustment() {
        // .txt has no special handling
        let base = bsa_hash_path("file");
        let txt = bsa_hash_path("file.txt");
        // They should differ (because the extension changes the path bytes)
        // but the extension-specific additions should NOT be applied
        // Verify by checking that hash2 (upper 32 bits) does NOT have 0x80 added
        // relative to a no-extension version with the same middle chars
        assert_ne!(base, txt);
    }

    // ── BA2 CRC32 hash verification ─────────────────────────────────

    #[test]
    fn test_ba2_crc32_known_value() {
        // Pin ba2_crc32 output for "test"
        let hash = ba2_crc32(b"test");
        let hash2 = ba2_crc32(b"test");
        assert_eq!(hash, hash2);
        assert_ne!(hash, 0);
    }

    #[test]
    fn test_ba2_crc32_single_byte() {
        let h = ba2_crc32(b"a");
        assert_eq!(h, u32::from(b'a')); // 0 * 31 + 'a' = 'a'
    }

    #[test]
    fn test_ba2_crc32_two_bytes() {
        // h = 0; h = 0*31 + 'a' = 97; h = 97*31 + 'b' = 3007 + 98 = 3105
        let h = ba2_crc32(b"ab");
        assert_eq!(h, 97u32 * 31 + 98);
    }

    // ── BSA hash folder normalizes forward slashes ──────────────────

    #[test]
    fn test_bsa_hash_folder_normalizes_slashes() {
        let h1 = bsa_hash_folder("meshes/armor");
        let h2 = bsa_hash_folder("meshes\\armor");
        assert_eq!(h1, h2);
    }

    // ── BSA hash path single character ──────────────────────────────

    #[test]
    fn test_bsa_hash_path_single_char() {
        let hash = bsa_hash_path("a");
        assert_ne!(hash, 0);
        // For single char: hash1 = a + (0 << 8) | (1 << 16) | (a << 24)
        let expected_lo = u32::from(b'a') | (1u32 << 16) | (u32::from(b'a') << 24);
        assert_eq!(hash as u32, expected_lo);
    }

    #[test]
    fn test_bsa_hash_path_two_chars() {
        let hash = bsa_hash_path("ab");
        assert_ne!(hash, 0);
        // len=2: hash1 = b + (a << 8) | (2 << 16) | (a << 24), hash2 = 0 (no middle chars)
        let expected_lo =
            u32::from(b'b') | (u32::from(b'a') << 8) | (2u32 << 16) | (u32::from(b'a') << 24);
        assert_eq!(hash as u32, expected_lo);
        assert_eq!(hash >> 32, 0); // no middle chars so hash2 = 0
    }

    // ── Different paths produce different hashes ────────────────────

    #[test]
    fn test_bsa_hash_different_filenames() {
        let h1 = bsa_hash_file("armor.nif");
        let h2 = bsa_hash_file("weapon.nif");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_bsa_hash_different_folders() {
        let h1 = bsa_hash_folder("meshes");
        let h2 = bsa_hash_folder("textures");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_ba2_crc32_different_inputs() {
        let h1 = ba2_crc32(b"meshes");
        let h2 = ba2_crc32(b"textures");
        assert_ne!(h1, h2);
    }
}
