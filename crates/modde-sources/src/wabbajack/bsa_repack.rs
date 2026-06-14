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
mod tests;
