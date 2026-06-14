//! BSA/BA2 archive table-of-contents and extraction helpers.
//!
//! The reader supports Bethesda BSA v104/v105 and BA2 `GNRL` archives. `DX10`
//! texture BA2 archives are indexed but intentionally not extracted yet.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
mod extract;
use extract::{extract_entry, extract_entry_to_writer};

use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveFormat {
    Bsa,
    Ba2Gnrl,
    Ba2Dx10,
}

/// A single file entry within an archive.
#[derive(Debug, Clone)]
pub struct ArchiveFileEntry {
    /// Normalized forward-slash relative path (e.g. `textures/sky.dds`).
    pub path: String,
    /// Uncompressed file size in bytes.
    pub size: u64,
    offset: u64,
    packed_size: u64,
    compressed: bool,
    bsa_version: u32,
    format: ArchiveFormat,
    embedded_name: bool,
}

/// Table-of-contents for a BSA or BA2 archive.
#[derive(Debug, Clone)]
pub struct ArchiveIndex {
    /// Path to the archive on disk.
    pub archive_path: PathBuf,
    /// All files listed in the archive.
    pub files: Vec<ArchiveFileEntry>,
}

const BSA_MAGIC: &[u8; 4] = b"BSA\0";
const BA2_MAGIC: &[u8; 4] = b"BTDX";
const BSA_ARCHIVE_COMPRESSED: u32 = 1 << 2;
const BSA_EMBED_FILE_NAMES: u32 = 1 << 8;
const BSA_SIZE_COMPRESS_TOGGLE: u32 = 0x4000_0000;
const BSA_SIZE_MASK: u32 = 0x3FFF_FFFF;

impl ArchiveIndex {
    /// Return true when the file has a Bethesda BSA/BA2 magic header.
    pub fn has_bethesda_magic(path: &Path) -> std::io::Result<bool> {
        let mut file = std::fs::File::open(path)?;
        let mut magic = [0u8; 4];
        file.read_exact(&mut magic)?;
        Ok(&magic == BSA_MAGIC || &magic == BA2_MAGIC)
    }

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

    /// Extract a single file from the archive using case-insensitive path matching.
    pub fn extract_file(&self, path: &str) -> Result<Vec<u8>> {
        let normalized = normalize_path(path);
        let entry = self.find_entry(path, &normalized)?;

        let mut file = std::fs::File::open(&self.archive_path)
            .with_context(|| format!("failed to open archive: {}", self.archive_path.display()))?;
        extract_entry(&mut file, entry)
    }

    /// Extract a single file into a writer using case-insensitive path matching.
    pub fn extract_file_to_writer(
        &self,
        path: &str,
        writer: &mut impl std::io::Write,
    ) -> Result<()> {
        let normalized = normalize_path(path);
        let entry = self.find_entry(path, &normalized)?;
        let mut file = std::fs::File::open(&self.archive_path)
            .with_context(|| format!("failed to open archive: {}", self.archive_path.display()))?;
        extract_entry_to_writer(&mut file, entry, writer)
    }

    fn find_entry(&self, raw_path: &str, normalized: &str) -> Result<&ArchiveFileEntry> {
        self.files
            .iter()
            .find(|entry| entry.path.eq_ignore_ascii_case(normalized))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "file '{}' not found in Bethesda archive {}",
                    raw_path,
                    self.archive_path.display()
                )
            })
    }
}

fn read_u16_le(r: &mut impl Read) -> Result<u16> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
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

fn read_bstring(r: &mut impl Read) -> Result<String> {
    let len = read_u8(r)? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    if buf.last() == Some(&0) {
        buf.pop();
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn read_u8(r: &mut impl Read) -> Result<u8> {
    let mut buf = [0u8; 1];
    r.read_exact(&mut buf)?;
    Ok(buf[0])
}

struct BsaHeader {
    version: u32,
    archive_flags: u32,
    folder_count: u32,
    file_count: u32,
}

struct BsaFolderRecord {
    file_count: u32,
}

struct BsaFileRecord {
    size_flags: u32,
    offset: u32,
}

fn read_bsa_header(r: &mut impl Read) -> Result<BsaHeader> {
    let version = read_u32_le(r)?;
    let _offset = read_u32_le(r)?;
    let archive_flags = read_u32_le(r)?;
    let folder_count = read_u32_le(r)?;
    let file_count = read_u32_le(r)?;
    let _total_folder_name_length = read_u32_le(r)?;
    let _total_file_name_length = read_u32_le(r)?;
    let _file_flags = read_u32_le(r)?;

    Ok(BsaHeader {
        version,
        archive_flags,
        folder_count,
        file_count,
    })
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
    let default_compressed = header.archive_flags & BSA_ARCHIVE_COMPRESSED != 0;
    let embedded_name = header.archive_flags & BSA_EMBED_FILE_NAMES != 0;

    let mut folder_records = Vec::with_capacity(folder_count);
    for _ in 0..folder_count {
        let _name_hash = read_u64_le(file)?;
        let file_count = read_u32_le(file)?;
        if header.version == 105 {
            let _padding = read_u32_le(file)?;
            let _offset = read_u64_le(file)?;
        } else {
            let _offset = read_u32_le(file)?;
        }
        folder_records.push(BsaFolderRecord { file_count });
    }

    let mut folder_names = Vec::with_capacity(folder_count);
    let mut file_records_by_folder: Vec<Vec<BsaFileRecord>> = Vec::with_capacity(folder_count);
    for folder_rec in &folder_records {
        folder_names.push(read_bstring(file)?);

        let mut records = Vec::with_capacity(folder_rec.file_count as usize);
        for _ in 0..folder_rec.file_count {
            let _name_hash = read_u64_le(file)?;
            let size_flags = read_u32_le(file)?;
            let offset = read_u32_le(file)?;
            records.push(BsaFileRecord { size_flags, offset });
        }
        file_records_by_folder.push(records);
    }

    let mut file_names = Vec::with_capacity(file_count);
    for _ in 0..file_count {
        file_names.push(read_null_terminated(file)?);
    }

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

            let toggle_compression = rec.size_flags & BSA_SIZE_COMPRESS_TOGGLE != 0;
            let compressed = default_compressed ^ toggle_compression;
            let packed_size = u64::from(rec.size_flags & BSA_SIZE_MASK);
            let size = if compressed { 0 } else { packed_size };

            entries.push(ArchiveFileEntry {
                path: normalize_path(&format!("{folder}/{file_name}")),
                size,
                offset: u64::from(rec.offset),
                packed_size,
                compressed,
                bsa_version: header.version,
                format: ArchiveFormat::Bsa,
                embedded_name,
            });
        }
    }

    populate_bsa_compressed_sizes(file, &mut entries)?;

    Ok(entries)
}

fn populate_bsa_compressed_sizes(
    file: &mut (impl Read + Seek),
    entries: &mut [ArchiveFileEntry],
) -> Result<()> {
    for entry in entries.iter_mut().filter(|entry| entry.compressed) {
        file.seek(SeekFrom::Start(entry.offset))?;
        let mut remaining = entry.packed_size;

        if entry.embedded_name {
            let name_len = u64::from(read_u8(file)?);
            file.seek(SeekFrom::Current(name_len as i64))?;
            remaining = remaining
                .checked_sub(name_len + 1)
                .context("BSA entry embedded filename exceeds entry size")?;
        }

        ensure!(
            remaining >= 4,
            "compressed BSA entry '{}' is missing an uncompressed size prefix",
            entry.path
        );
        entry.size = u64::from(read_u32_le(file)?);
    }

    Ok(())
}

fn read_ba2(file: &mut (impl Read + Seek)) -> Result<Vec<ArchiveFileEntry>> {
    let _version = read_u32_le(file)?;
    let mut type_buf = [0u8; 4];
    file.read_exact(&mut type_buf)?;
    let archive_type = std::str::from_utf8(&type_buf)
        .context("invalid BA2 type string")?
        .to_string();
    let file_count = read_u32_le(file)? as usize;
    let name_table_offset = read_u64_le(file)?;

    let mut records = Vec::with_capacity(file_count);
    match archive_type.as_str() {
        "GNRL" => {
            for _ in 0..file_count {
                let _name_hash = read_u32_le(file)?;
                let _ext = read_u32_le(file)?;
                let _dir_hash = read_u32_le(file)?;
                let _unknown = read_u32_le(file)?;
                let offset = read_u64_le(file)?;
                let packed_size = read_u32_le(file)?;
                let unpacked_size = read_u32_le(file)?;
                let _sentinel = read_u32_le(file)?;
                records.push((offset, packed_size, unpacked_size, ArchiveFormat::Ba2Gnrl));
            }
        }
        "DX10" => {
            for _ in 0..file_count {
                let _name_hash = read_u32_le(file)?;
                let _ext = read_u32_le(file)?;
                let _dir_hash = read_u32_le(file)?;
                let _unknown = read_u32_le(file)?;
                let _height = read_u32_le(file)?;
                let _mip_count = read_u32_le(file)?;
                let _dxgi_format = read_u32_le(file)?;
                let _tile_mode = read_u32_le(file)?;
                records.push((0, 0, 0, ArchiveFormat::Ba2Dx10));
            }
        }
        other => bail!("unsupported BA2 archive type: {other}"),
    }

    file.seek(SeekFrom::Start(name_table_offset))?;

    let mut entries = Vec::with_capacity(file_count);
    for (offset, packed_size, unpacked_size, format) in records {
        let len = read_u16_le(file)? as usize;
        let mut name_buf = vec![0u8; len];
        file.read_exact(&mut name_buf)?;
        let path = normalize_path(&String::from_utf8_lossy(&name_buf));
        let compressed = format == ArchiveFormat::Ba2Gnrl && packed_size != 0;
        entries.push(ArchiveFileEntry {
            path,
            size: u64::from(unpacked_size),
            offset,
            packed_size: if packed_size == 0 {
                u64::from(unpacked_size)
            } else {
                u64::from(packed_size)
            },
            compressed,
            bsa_version: 0,
            format,
            embedded_name: false,
        });
    }

    Ok(entries)
}

/// Normalize a file path: lowercase, forward slashes, strip leading slash/dot.
#[must_use]
pub fn normalize_path(raw: &str) -> String {
    let s = raw.replace('\\', "/").to_lowercase();
    s.trim_start_matches('/')
        .trim_start_matches("./")
        .to_string()
}

#[cfg(test)]
mod tests;
