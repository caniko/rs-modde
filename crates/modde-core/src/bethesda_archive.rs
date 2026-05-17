//! BSA/BA2 archive table-of-contents and extraction helpers.
//!
//! The reader supports Bethesda BSA v104/v105 and BA2 `GNRL` archives. `DX10`
//! texture BA2 archives are indexed but intentionally not extracted yet.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use flate2::read::ZlibDecoder;
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

fn extract_entry(file: &mut (impl Read + Seek), entry: &ArchiveFileEntry) -> Result<Vec<u8>> {
    match entry.format {
        ArchiveFormat::Bsa => extract_bsa_entry(file, entry),
        ArchiveFormat::Ba2Gnrl => extract_ba2_gnrl_entry(file, entry),
        ArchiveFormat::Ba2Dx10 => bail!(
            "BA2 DX10 texture extraction is not supported for '{}'",
            entry.path
        ),
    }
}

fn extract_entry_to_writer(
    file: &mut (impl Read + Seek),
    entry: &ArchiveFileEntry,
    writer: &mut impl std::io::Write,
) -> Result<()> {
    match entry.format {
        ArchiveFormat::Bsa => extract_bsa_entry_to_writer(file, entry, writer),
        ArchiveFormat::Ba2Gnrl => extract_ba2_gnrl_entry_to_writer(file, entry, writer),
        ArchiveFormat::Ba2Dx10 => bail!(
            "BA2 DX10 texture extraction is not supported for '{}'",
            entry.path
        ),
    }
}

fn extract_bsa_entry(file: &mut (impl Read + Seek), entry: &ArchiveFileEntry) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(entry.offset))?;

    let mut remaining = entry.packed_size;
    if entry.embedded_name {
        let name_len = u64::from(read_u8(file)?);
        let mut skip = vec![0u8; name_len as usize];
        file.read_exact(&mut skip)?;
        remaining = remaining
            .checked_sub(name_len + 1)
            .context("BSA entry embedded filename exceeds entry size")?;
    }

    if entry.compressed {
        let expected_size = read_u32_le(file)?;
        remaining = remaining
            .checked_sub(4)
            .context("compressed BSA entry missing uncompressed size prefix")?;
        let mut packed = vec![0u8; remaining as usize];
        file.read_exact(&mut packed)?;
        let data = decompress_bsa_payload(entry, &packed, expected_size as usize)?;
        ensure!(
            data.len() == expected_size as usize,
            "decompressed BSA entry '{}' size mismatch: expected {}, got {}",
            entry.path,
            expected_size,
            data.len()
        );
        Ok(data)
    } else {
        let mut data = vec![0u8; remaining as usize];
        file.read_exact(&mut data)?;
        Ok(data)
    }
}

fn extract_bsa_entry_to_writer(
    file: &mut (impl Read + Seek),
    entry: &ArchiveFileEntry,
    writer: &mut impl std::io::Write,
) -> Result<()> {
    file.seek(SeekFrom::Start(entry.offset))?;

    let mut remaining = entry.packed_size;
    if entry.embedded_name {
        let name_len = u64::from(read_u8(file)?);
        std::io::copy(&mut (&mut *file).take(name_len), &mut std::io::sink())?;
        remaining = remaining
            .checked_sub(name_len + 1)
            .context("BSA entry embedded filename exceeds entry size")?;
    }

    if entry.compressed {
        let expected_size = read_u32_le(file)?;
        remaining = remaining
            .checked_sub(4)
            .context("compressed BSA entry missing uncompressed size prefix")?;
        let mut packed = vec![0u8; remaining as usize];
        file.read_exact(&mut packed)?;
        let data = decompress_bsa_payload(entry, &packed, expected_size as usize)?;
        writer.write_all(&data)?;
        let written = data.len() as u64;
        ensure!(
            written == u64::from(expected_size),
            "decompressed BSA entry '{}' size mismatch: expected {}, got {}",
            entry.path,
            expected_size,
            written
        );
    } else {
        let written = std::io::copy(&mut (&mut *file).take(remaining), writer)?;
        ensure!(
            written == remaining,
            "BSA entry '{}' size mismatch: expected {}, got {}",
            entry.path,
            remaining,
            written
        );
    }
    writer.flush()?;
    Ok(())
}

fn extract_ba2_gnrl_entry(
    file: &mut (impl Read + Seek),
    entry: &ArchiveFileEntry,
) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(entry.offset))?;
    let mut data = vec![0u8; entry.packed_size as usize];
    file.read_exact(&mut data)?;

    if !entry.compressed {
        return Ok(data);
    }

    let mut decoder = ZlibDecoder::new(&data[..]);
    let mut unpacked = Vec::with_capacity(entry.size as usize);
    decoder.read_to_end(&mut unpacked)?;
    ensure!(
        unpacked.len() == entry.size as usize,
        "decompressed BA2 entry '{}' size mismatch: expected {}, got {}",
        entry.path,
        entry.size,
        unpacked.len()
    );
    Ok(unpacked)
}

fn extract_ba2_gnrl_entry_to_writer(
    file: &mut (impl Read + Seek),
    entry: &ArchiveFileEntry,
    writer: &mut impl std::io::Write,
) -> Result<()> {
    file.seek(SeekFrom::Start(entry.offset))?;

    if !entry.compressed {
        let written = std::io::copy(&mut (&mut *file).take(entry.packed_size), writer)?;
        ensure!(
            written == entry.packed_size,
            "BA2 entry '{}' size mismatch: expected {}, got {}",
            entry.path,
            entry.packed_size,
            written
        );
        writer.flush()?;
        return Ok(());
    }

    let mut data = vec![0u8; entry.packed_size as usize];
    file.read_exact(&mut data)?;
    let mut decoder = ZlibDecoder::new(&data[..]);
    let written = std::io::copy(&mut decoder, writer)?;
    ensure!(
        written == entry.size,
        "decompressed BA2 entry '{}' size mismatch: expected {}, got {}",
        entry.path,
        entry.size,
        written
    );
    writer.flush()?;
    Ok(())
}

fn decompress_bsa_payload(
    entry: &ArchiveFileEntry,
    packed: &[u8],
    expected_size: usize,
) -> Result<Vec<u8>> {
    if entry.bsa_version >= 105 {
        if packed.starts_with(&[0x04, 0x22, 0x4d, 0x18]) {
            let mut decoder = lz4_flex::frame::FrameDecoder::new(packed);
            let mut data = Vec::with_capacity(expected_size);
            decoder.read_to_end(&mut data).with_context(|| {
                format!("failed to LZ4-frame-decompress BSA entry '{}'", entry.path)
            })?;
            Ok(data)
        } else {
            lz4_flex::block::decompress(packed, expected_size)
                .with_context(|| format!("failed to LZ4-decompress BSA entry '{}'", entry.path))
        }
    } else {
        let mut decoder = ZlibDecoder::new(packed);
        let mut data = Vec::with_capacity(expected_size);
        decoder
            .read_to_end(&mut data)
            .with_context(|| format!("failed to zlib-decompress BSA entry '{}'", entry.path))?;
        Ok(data)
    }
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
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::{Cursor, Write};

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn lz4_frame(data: &[u8]) -> Vec<u8> {
        let mut encoder = lz4_flex::frame::FrameEncoder::new(Vec::new());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn write_temp(data: &[u8]) -> tempfile::NamedTempFile {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(data).unwrap();
        tmp
    }

    fn build_test_bsa(folders: &[(&str, &[(&str, &[u8], bool)])]) -> Vec<u8> {
        build_test_bsa_with_version_and_flags(folders, 105, 0x03)
    }

    fn build_test_bsa_with_flags(
        folders: &[(&str, &[(&str, &[u8], bool)])],
        archive_flags: u32,
    ) -> Vec<u8> {
        build_test_bsa_with_version_and_flags(folders, 105, archive_flags)
    }

    fn build_test_bsa_with_version_and_flags(
        folders: &[(&str, &[(&str, &[u8], bool)])],
        version: u32,
        archive_flags: u32,
    ) -> Vec<u8> {
        let mut buf = Vec::new();
        let folder_count = folders.len() as u32;
        let file_count: u32 = folders.iter().map(|(_, files)| files.len() as u32).sum();
        let total_folder_name_len: u32 =
            folders.iter().map(|(name, _)| name.len() as u32 + 2).sum();
        let total_file_name_len: u32 = folders
            .iter()
            .flat_map(|(_, files)| files.iter())
            .map(|(name, _, _)| name.len() as u32 + 1)
            .sum();

        buf.extend_from_slice(BSA_MAGIC);
        buf.extend_from_slice(&version.to_le_bytes());
        buf.extend_from_slice(&36u32.to_le_bytes());
        buf.extend_from_slice(&archive_flags.to_le_bytes());
        buf.extend_from_slice(&folder_count.to_le_bytes());
        buf.extend_from_slice(&file_count.to_le_bytes());
        buf.extend_from_slice(&total_folder_name_len.to_le_bytes());
        buf.extend_from_slice(&total_file_name_len.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());

        for (_, files) in folders {
            buf.extend_from_slice(&0u64.to_le_bytes());
            buf.extend_from_slice(&(files.len() as u32).to_le_bytes());
            if version == 105 {
                buf.extend_from_slice(&0u32.to_le_bytes());
                buf.extend_from_slice(&0u64.to_le_bytes());
            } else {
                buf.extend_from_slice(&0u32.to_le_bytes());
            }
        }

        let mut file_record_positions = Vec::new();
        let mut payloads = Vec::new();
        for (folder_name, files) in folders {
            buf.push((folder_name.len() + 1) as u8);
            buf.extend_from_slice(folder_name.as_bytes());
            buf.push(0);

            for (file_name, data, compressed) in *files {
                let mut payload = if *compressed {
                    let packed = if version >= 105 {
                        lz4_frame(data)
                    } else {
                        zlib(data)
                    };
                    let mut payload = Vec::new();
                    payload.extend_from_slice(&(data.len() as u32).to_le_bytes());
                    payload.extend_from_slice(&packed);
                    payload
                } else {
                    data.to_vec()
                };
                if archive_flags & BSA_EMBED_FILE_NAMES != 0 {
                    let mut embedded = Vec::new();
                    embedded.push(file_name.len() as u8);
                    embedded.extend_from_slice(file_name.as_bytes());
                    embedded.extend_from_slice(&payload);
                    payload = embedded;
                }
                let mut size_flags = payload.len() as u32;
                if *compressed {
                    size_flags |= BSA_SIZE_COMPRESS_TOGGLE;
                }

                buf.extend_from_slice(&0u64.to_le_bytes());
                buf.extend_from_slice(&size_flags.to_le_bytes());
                file_record_positions.push(buf.len());
                buf.extend_from_slice(&0u32.to_le_bytes());
                payloads.push(payload);
            }
        }

        for (_, files) in folders {
            for (name, _, _) in *files {
                buf.extend_from_slice(name.as_bytes());
                buf.push(0);
            }
        }

        for (offset_pos, payload) in file_record_positions.into_iter().zip(payloads) {
            let offset = buf.len() as u32;
            buf[offset_pos..offset_pos + 4].copy_from_slice(&offset.to_le_bytes());
            buf.extend_from_slice(&payload);
        }

        buf
    }

    fn build_test_bsa_v104(folders: &[(&str, &[(&str, &[u8], bool)])]) -> Vec<u8> {
        build_test_bsa_with_version_and_flags(folders, 104, 0x03)
    }

    fn build_test_ba2(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut buf = Vec::new();
        let file_count = files.len() as u32;
        let header_size = 24usize;
        let records_size = file_count as usize * 36;
        let name_table_size: usize = files.iter().map(|(name, _, _)| 2 + name.len()).sum();
        let data_start = header_size + records_size + name_table_size;

        buf.extend_from_slice(BA2_MAGIC);
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(b"GNRL");
        buf.extend_from_slice(&file_count.to_le_bytes());
        buf.extend_from_slice(&((header_size + records_size) as u64).to_le_bytes());

        let mut payloads = Vec::new();
        let mut running_offset = data_start as u64;
        for (_, data, compressed) in files {
            let payload = if *compressed {
                zlib(data)
            } else {
                data.to_vec()
            };
            buf.extend_from_slice(&0u32.to_le_bytes());
            buf.extend_from_slice(&0u32.to_le_bytes());
            buf.extend_from_slice(&0u32.to_le_bytes());
            buf.extend_from_slice(&0u32.to_le_bytes());
            buf.extend_from_slice(&running_offset.to_le_bytes());
            buf.extend_from_slice(
                &(if *compressed { payload.len() as u32 } else { 0 }).to_le_bytes(),
            );
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(&0xBAADF00Du32.to_le_bytes());
            running_offset += payload.len() as u64;
            payloads.push(payload);
        }

        for (name, _, _) in files {
            buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
            buf.extend_from_slice(name.as_bytes());
        }

        for payload in payloads {
            buf.extend_from_slice(&payload);
        }

        buf
    }

    #[test]
    fn parse_synthetic_bsa_v105() {
        let data = build_test_bsa(&[
            (
                "textures",
                &[
                    ("sky.dds", b"sky".as_slice(), false),
                    ("ground.dds", b"ground", false),
                ],
            ),
            ("meshes", &[("tree.nif", b"tree", false)]),
        ]);
        let mut cursor = Cursor::new(&data);
        cursor.set_position(4);
        let entries = read_bsa(&mut cursor).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].path, "textures/sky.dds");
        assert_eq!(entries[1].path, "textures/ground.dds");
        assert_eq!(entries[2].path, "meshes/tree.nif");
    }

    #[test]
    fn extract_synthetic_bsa_v105_lz4_compressed() {
        let data = build_test_bsa(&[(
            "textures",
            &[
                ("sky.dds", b"sky bytes".as_slice(), false),
                ("cloud.dds", b"cloud bytes", true),
            ],
        )]);
        let tmp = write_temp(&data);
        let index = ArchiveIndex::read(tmp.path()).unwrap();
        assert_eq!(index.files[0].size, b"sky bytes".len() as u64);
        assert_eq!(index.files[1].size, b"cloud bytes".len() as u64);
        assert_eq!(
            index.extract_file("textures/sky.dds").unwrap(),
            b"sky bytes"
        );
        assert_eq!(
            index.extract_file("Textures\\Cloud.dds").unwrap(),
            b"cloud bytes"
        );
    }

    #[test]
    fn extract_synthetic_bsa_v104_zlib_compressed() {
        let data = build_test_bsa_v104(&[(
            "textures",
            &[("cloud.dds", b"cloud bytes".as_slice(), true)],
        )]);
        let tmp = write_temp(&data);
        let index = ArchiveIndex::read(tmp.path()).unwrap();
        assert_eq!(index.files[0].size, b"cloud bytes".len() as u64);
        assert_eq!(
            index.extract_file("Textures\\Cloud.dds").unwrap(),
            b"cloud bytes"
        );
    }

    #[test]
    fn extract_synthetic_bsa_with_embedded_names() {
        let data = build_test_bsa_with_flags(
            &[(
                "scripts",
                &[("quest.pex", b"script bytes".as_slice(), true)],
            )],
            0x03 | BSA_EMBED_FILE_NAMES,
        );
        let tmp = write_temp(&data);
        let index = ArchiveIndex::read(tmp.path()).unwrap();
        assert_eq!(index.files[0].size, b"script bytes".len() as u64);
        assert_eq!(
            index.extract_file("scripts/quest.pex").unwrap(),
            b"script bytes"
        );
    }

    #[test]
    fn bethesda_magic_probe_distinguishes_formats() {
        let bsa = write_temp(&build_test_bsa(&[(
            "textures",
            &[("sky.dds", b"sky".as_slice(), false)],
        )]));
        let other = write_temp(b"PK\x03\x04not bethesda");

        assert!(ArchiveIndex::has_bethesda_magic(bsa.path()).unwrap());
        assert!(!ArchiveIndex::has_bethesda_magic(other.path()).unwrap());
    }

    #[test]
    fn parse_and_extract_synthetic_ba2_gnrl() {
        let data = build_test_ba2(&[
            ("textures\\sky.dds", b"sky bytes".as_slice(), false),
            ("meshes\\tree.nif", b"tree bytes", true),
        ]);
        let tmp = write_temp(&data);
        let index = ArchiveIndex::read(tmp.path()).unwrap();
        assert_eq!(index.files.len(), 2);
        assert_eq!(index.files[0].path, "textures/sky.dds");
        assert_eq!(
            index.extract_file("textures/sky.dds").unwrap(),
            b"sky bytes"
        );
        assert_eq!(
            index.extract_file("meshes/tree.nif").unwrap(),
            b"tree bytes"
        );
    }

    #[test]
    fn normalize_path_handles_backslashes_and_case() {
        assert_eq!(normalize_path("Textures\\Sky.DDS"), "textures/sky.dds");
        assert_eq!(normalize_path("/textures/sky.dds"), "textures/sky.dds");
        assert_eq!(normalize_path("./meshes/tree.nif"), "meshes/tree.nif");
    }

    #[test]
    fn bad_magic_returns_error() {
        let tmp = write_temp(b"NOPE____");
        let result = ArchiveIndex::read(tmp.path());
        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains("unrecognised archive format"));
    }
}
