//! BSA/BA2 archive table-of-contents reader.
//!
//! Reads only the file listing from Bethesda archive formats — no extraction
//! or decompression is performed. This enables archive-aware conflict detection
//! without the cost of unpacking archives.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use tracing::debug;

/// A single file entry within an archive.
#[derive(Debug, Clone)]
pub struct ArchiveFileEntry {
    /// Normalized forward-slash relative path (e.g. `textures/sky.dds`).
    pub path: String,
    /// Uncompressed file size in bytes.
    pub size: u64,
}

/// Table-of-contents for a BSA or BA2 archive.
#[derive(Debug, Clone)]
pub struct ArchiveIndex {
    /// Path to the archive on disk.
    pub archive_path: PathBuf,
    /// All files listed in the archive.
    pub files: Vec<ArchiveFileEntry>,
}

/// BSA magic bytes: `BSA\0`.
const BSA_MAGIC: &[u8; 4] = b"BSA\0";

/// BA2 magic bytes: `BTDX`.
const BA2_MAGIC: &[u8; 4] = b"BTDX";

impl ArchiveIndex {
    /// Read the file listing from a BSA or BA2 archive.
    ///
    /// Auto-detects the format from the magic bytes at the start of the file.
    pub fn read(path: &Path) -> Result<Self> {
        let mut file = std::fs::File::open(path)
            .with_context(|| format!("failed to open archive: {}", path.display()))?;

        let mut magic = [0u8; 4];
        file.read_exact(&mut magic)
            .context("failed to read archive magic bytes")?;

        let files = if &magic == BSA_MAGIC {
            debug!(path = %path.display(), "reading BSA archive index");
            read_bsa(&mut file)?
        } else if &magic == BA2_MAGIC {
            debug!(path = %path.display(), "reading BA2 archive index");
            read_ba2(&mut file)?
        } else {
            bail!(
                "unrecognised archive format (magic: {:?}) for {}",
                magic,
                path.display()
            );
        };

        debug!(path = %path.display(), file_count = files.len(), "archive index read");

        Ok(Self {
            archive_path: path.to_path_buf(),
            files,
        })
    }
}

// ── BSA reader ───────────────────────────────────────────────────────

/// BSA header (after the 4-byte magic).
struct BsaHeader {
    version: u32,
    _offset: u32,
    _archive_flags: u32,
    folder_count: u32,
    file_count: u32,
    _total_folder_name_length: u32,
    _total_file_name_length: u32,
    _file_flags: u32,
}

fn read_u32_le(r: &mut impl Read) -> Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64_le(r: &mut impl Read) -> Result<u64> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

fn read_bsa_header(r: &mut impl Read) -> Result<BsaHeader> {
    Ok(BsaHeader {
        version: read_u32_le(r)?,
        _offset: read_u32_le(r)?,
        _archive_flags: read_u32_le(r)?,
        folder_count: read_u32_le(r)?,
        file_count: read_u32_le(r)?,
        _total_folder_name_length: read_u32_le(r)?,
        _total_file_name_length: read_u32_le(r)?,
        _file_flags: read_u32_le(r)?,
    })
}

/// Read a null-terminated string, consuming the null byte.
fn read_null_terminated(r: &mut impl Read) -> Result<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        r.read_exact(&mut byte)?;
        if byte[0] == 0 {
            break;
        }
        buf.push(byte[0]);
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Read a BSA length-prefixed string (1 byte length, then chars, no null terminator).
fn read_bstring(r: &mut impl Read) -> Result<String> {
    let mut len_buf = [0u8; 1];
    r.read_exact(&mut len_buf)?;
    let len = len_buf[0] as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    // BSA bstrings sometimes include trailing null in the length
    if buf.last() == Some(&0) {
        buf.pop();
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Folder record in a BSA.
struct BsaFolderRecord {
    _name_hash: u64,
    file_count: u32,
    _offset: u64,
}

/// File record in a BSA.
struct BsaFileRecord {
    _name_hash: u64,
    size: u32,
    _offset: u32,
}

fn read_bsa(file: &mut (impl Read + Seek)) -> Result<Vec<ArchiveFileEntry>> {
    let header = read_bsa_header(file)?;
    ensure!(
        header.version == 104 || header.version == 105,
        "unsupported BSA version: {} (expected 104 or 105)",
        header.version
    );

    let folder_count = header.folder_count as usize;
    let file_count = header.file_count as usize;

    // Read folder records.
    let mut folder_records = Vec::with_capacity(folder_count);
    for _ in 0..folder_count {
        let name_hash = read_u64_le(file)?;
        let fc = read_u32_le(file)?;
        // v105 (SSE) has extra padding (4 bytes) + 8-byte offset
        let offset = if header.version == 105 {
            let _padding = read_u32_le(file)?;
            read_u64_le(file)?
        } else {
            // v104: 4-byte offset
            u64::from(read_u32_le(file)?)
        };
        folder_records.push(BsaFolderRecord {
            _name_hash: name_hash,
            file_count: fc,
            _offset: offset,
        });
    }

    // Read file record blocks (folder name + file records for each folder).
    // Each block: bstring folder name, then N file records.
    let mut folder_names = Vec::with_capacity(folder_count);
    let mut file_records_by_folder: Vec<Vec<BsaFileRecord>> = Vec::with_capacity(folder_count);

    for folder_rec in &folder_records {
        let folder_name = read_bstring(file)?;
        folder_names.push(folder_name);

        let mut records = Vec::with_capacity(folder_rec.file_count as usize);
        for _ in 0..folder_rec.file_count {
            let name_hash = read_u64_le(file)?;
            let size = read_u32_le(file)?;
            let offset = read_u32_le(file)?;
            records.push(BsaFileRecord {
                _name_hash: name_hash,
                size: size & 0x3FFF_FFFF, // mask off compression flag bits
                _offset: offset,
            });
        }
        file_records_by_folder.push(records);
    }

    // Read the file name block: file_count null-terminated strings in order.
    let mut file_names = Vec::with_capacity(file_count);
    for _ in 0..file_count {
        file_names.push(read_null_terminated(file)?);
    }

    // Combine folder names with file names.
    let mut entries = Vec::with_capacity(file_count);
    let mut name_idx = 0usize;
    for (folder_idx, records) in file_records_by_folder.iter().enumerate() {
        let folder = &folder_names[folder_idx];
        for rec in records {
            if name_idx >= file_names.len() {
                bail!("BSA file name index out of bounds");
            }
            let file_name = &file_names[name_idx];
            name_idx += 1;

            let path = normalize_path(&format!("{folder}/{file_name}"));
            entries.push(ArchiveFileEntry {
                path,
                size: u64::from(rec.size),
            });
        }
    }

    Ok(entries)
}

// ── BA2 reader ───────────────────────────────────────────────────────

fn read_ba2(file: &mut (impl Read + Seek)) -> Result<Vec<ArchiveFileEntry>> {
    // After the 4-byte magic, read the rest of the header.
    let version = read_u32_le(file)?;
    let mut type_buf = [0u8; 4];
    file.read_exact(&mut type_buf)?;
    let archive_type = std::str::from_utf8(&type_buf)
        .context("invalid BA2 type string")?
        .to_string();
    let file_count = read_u32_le(file)? as usize;
    let name_table_offset = read_u64_le(file)?;

    debug!(version, archive_type = %archive_type, file_count, name_table_offset, "BA2 header");

    // Read file records to get sizes.
    let mut sizes = Vec::with_capacity(file_count);

    match archive_type.as_str() {
        "GNRL" => {
            for _ in 0..file_count {
                let _name_hash = read_u32_le(file)?;
                let _ext = read_u32_le(file)?; // 4-byte extension
                let _dir_hash = read_u32_le(file)?;
                let _unknown = read_u32_le(file)?; // flags / unknown
                let _offset = read_u64_le(file)?;
                let _packed_size = read_u32_le(file)?;
                let unpacked_size = read_u32_le(file)?;
                let _sentinel = read_u32_le(file)?; // 0xBAADF00D
                sizes.push(u64::from(unpacked_size));
            }
        }
        "DX10" => {
            for _ in 0..file_count {
                let _name_hash = read_u32_le(file)?;
                let _ext = read_u32_le(file)?;
                let _dir_hash = read_u32_le(file)?;
                let _unknown = read_u32_le(file)?; // unk8
                let _height = read_u32_le(file)?; // actually u16+u16 but we skip
                let _mip_count = read_u32_le(file)?; // actually u8+... skip
                let _dxgi_format = read_u32_le(file)?; // skip remaining DX10 fields
                let _tile_mode = read_u32_le(file)?;
                // DX10 records are 24 bytes of header + variable chunks;
                // we only need names, so store 0 as size placeholder.
                sizes.push(0);
            }
        }
        other => {
            bail!("unsupported BA2 archive type: {other}");
        }
    }

    // Seek to name table and read file names.
    file.seek(SeekFrom::Start(name_table_offset))?;

    let mut entries = Vec::with_capacity(file_count);
    for i in 0..file_count {
        // BA2 name table: u16 length prefix, then string bytes (no null terminator).
        let mut len_buf = [0u8; 2];
        file.read_exact(&mut len_buf)?;
        let len = u16::from_le_bytes(len_buf) as usize;
        let mut name_buf = vec![0u8; len];
        file.read_exact(&mut name_buf)?;

        let raw_name = String::from_utf8_lossy(&name_buf).into_owned();
        let path = normalize_path(&raw_name);

        entries.push(ArchiveFileEntry {
            path,
            size: sizes.get(i).copied().unwrap_or(0),
        });
    }

    Ok(entries)
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Normalize a file path: lowercase, forward slashes, strip leading slash/dot.
fn normalize_path(raw: &str) -> String {
    let s = raw.replace('\\', "/").to_lowercase();
    s.trim_start_matches('/')
        .trim_start_matches("./")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Build a minimal v105 BSA in memory with the given folder->files mapping.
    fn build_test_bsa(folders: &[(&str, &[(&str, u32)])]) -> Vec<u8> {
        let mut buf = Vec::new();

        let folder_count = folders.len() as u32;
        let file_count: u32 = folders.iter().map(|(_, files)| files.len() as u32).sum();

        // Compute total folder name length (each bstring: 1 byte len + chars + null).
        let total_folder_name_len: u32 = folders
            .iter()
            .map(|(name, _)| name.len() as u32 + 2) // +1 for length byte, +1 for null in bstring
            .sum();

        // Total file name length (null-terminated strings).
        let total_file_name_len: u32 = folders
            .iter()
            .flat_map(|(_, files)| files.iter())
            .map(|(name, _)| name.len() as u32 + 1) // +1 for null
            .sum();

        // Magic
        buf.extend_from_slice(b"BSA\0");
        // Version = 105 (SSE)
        buf.extend_from_slice(&105u32.to_le_bytes());
        // Offset to folder records = 36 (header size)
        buf.extend_from_slice(&36u32.to_le_bytes());
        // Archive flags
        buf.extend_from_slice(&0x03u32.to_le_bytes()); // has dir/file names
        // Folder count
        buf.extend_from_slice(&folder_count.to_le_bytes());
        // File count
        buf.extend_from_slice(&file_count.to_le_bytes());
        // Total folder name length
        buf.extend_from_slice(&total_folder_name_len.to_le_bytes());
        // Total file name length
        buf.extend_from_slice(&total_file_name_len.to_le_bytes());
        // File flags
        buf.extend_from_slice(&0u32.to_le_bytes());

        // Folder records (v105: hash(8) + count(4) + padding(4) + offset(8) = 24 bytes each)
        for (_, files) in folders {
            buf.extend_from_slice(&0u64.to_le_bytes()); // name hash (unused by our reader)
            buf.extend_from_slice(&(files.len() as u32).to_le_bytes());
            buf.extend_from_slice(&0u32.to_le_bytes()); // padding
            buf.extend_from_slice(&0u64.to_le_bytes()); // offset (unused by our reader)
        }

        // File record blocks: for each folder, bstring name + file records.
        for (folder_name, files) in folders {
            // bstring: length byte (includes trailing null) + chars + null
            let bstr_len = (folder_name.len() + 1) as u8; // +1 for null
            buf.push(bstr_len);
            buf.extend_from_slice(folder_name.as_bytes());
            buf.push(0); // null terminator

            for (_, size) in *files {
                buf.extend_from_slice(&0u64.to_le_bytes()); // name hash
                buf.extend_from_slice(&size.to_le_bytes()); // size
                buf.extend_from_slice(&0u32.to_le_bytes()); // offset
            }
        }

        // File name block: null-terminated strings.
        for (_, files) in folders {
            for (name, _) in *files {
                buf.extend_from_slice(name.as_bytes());
                buf.push(0);
            }
        }

        buf
    }

    /// Build a minimal GNRL BA2 in memory.
    fn build_test_ba2(files: &[(&str, u32)]) -> Vec<u8> {
        let mut buf = Vec::new();
        let file_count = files.len() as u32;

        // Magic
        buf.extend_from_slice(b"BTDX");
        // Version
        buf.extend_from_slice(&1u32.to_le_bytes());
        // Type
        buf.extend_from_slice(b"GNRL");
        // File count
        buf.extend_from_slice(&file_count.to_le_bytes());

        // We need to know the name table offset. Header is 4+4+4+4+8 = 24 bytes.
        // Each GNRL record is 36 bytes.
        let header_size: u64 = 24;
        let records_size: u64 = u64::from(file_count) * 36;
        let name_table_offset = header_size + records_size;

        buf.extend_from_slice(&name_table_offset.to_le_bytes());

        // GNRL file records (36 bytes each)
        for (_, size) in files {
            buf.extend_from_slice(&0u32.to_le_bytes()); // name_hash
            buf.extend_from_slice(&0u32.to_le_bytes()); // ext
            buf.extend_from_slice(&0u32.to_le_bytes()); // dir_hash
            buf.extend_from_slice(&0u32.to_le_bytes()); // unknown/flags
            buf.extend_from_slice(&0u64.to_le_bytes()); // offset
            buf.extend_from_slice(&0u32.to_le_bytes()); // packed_size
            buf.extend_from_slice(&size.to_le_bytes()); // unpacked_size
            buf.extend_from_slice(&0xBAADF00Du32.to_le_bytes()); // sentinel
        }

        // Name table: u16 length prefix + string bytes.
        for (name, _) in files {
            let len = name.len() as u16;
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(name.as_bytes());
        }

        buf
    }

    #[test]
    fn parse_synthetic_bsa_v105() {
        let data = build_test_bsa(&[
            ("textures", &[("sky.dds", 1024), ("ground.dds", 2048)]),
            ("meshes", &[("tree.nif", 512)]),
        ]);

        // Full data cursor, seek past magic (as ArchiveIndex::read does).
        let mut cursor = Cursor::new(&data);
        cursor.set_position(4);
        let entries = read_bsa(&mut cursor).unwrap();

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].path, "textures/sky.dds");
        assert_eq!(entries[0].size, 1024);
        assert_eq!(entries[1].path, "textures/ground.dds");
        assert_eq!(entries[1].size, 2048);
        assert_eq!(entries[2].path, "meshes/tree.nif");
        assert_eq!(entries[2].size, 512);
    }

    #[test]
    fn parse_synthetic_ba2_gnrl() {
        let data = build_test_ba2(&[("textures\\sky.dds", 1024), ("meshes\\tree.nif", 512)]);

        let mut cursor = Cursor::new(&data);
        cursor.set_position(4);
        let entries = read_ba2(&mut cursor).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "textures/sky.dds");
        assert_eq!(entries[0].size, 1024);
        assert_eq!(entries[1].path, "meshes/tree.nif");
        assert_eq!(entries[1].size, 512);
    }

    #[test]
    fn normalize_path_handles_backslashes_and_case() {
        assert_eq!(normalize_path("Textures\\Sky.DDS"), "textures/sky.dds");
        assert_eq!(normalize_path("/textures/sky.dds"), "textures/sky.dds");
        assert_eq!(normalize_path("./meshes/tree.nif"), "meshes/tree.nif");
    }

    #[test]
    fn bad_magic_returns_error() {
        let data = b"NOPE____";
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut tmp.as_file(), data).unwrap();
        let result = ArchiveIndex::read(tmp.path());
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("unrecognised archive format"));
    }

    #[test]
    fn empty_bsa_parses() {
        let data = build_test_bsa(&[]);
        let mut cursor = Cursor::new(&data);
        cursor.set_position(4);
        let entries = read_bsa(&mut cursor).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn empty_ba2_parses() {
        let data = build_test_ba2(&[]);
        let mut cursor = Cursor::new(&data);
        cursor.set_position(4);
        let entries = read_ba2(&mut cursor).unwrap();
        assert!(entries.is_empty());
    }
}
