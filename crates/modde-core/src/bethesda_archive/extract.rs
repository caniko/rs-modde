use std::io::{Read, Seek, SeekFrom};

use anyhow::{Context, Result, bail, ensure};
use flate2::read::ZlibDecoder;

use super::{ArchiveFileEntry, ArchiveFormat, read_u8, read_u32_le};

pub(super) fn extract_entry(file: &mut (impl Read + Seek), entry: &ArchiveFileEntry) -> Result<Vec<u8>> {
    match entry.format {
        ArchiveFormat::Bsa => extract_bsa_entry(file, entry),
        ArchiveFormat::Ba2Gnrl => extract_ba2_gnrl_entry(file, entry),
        ArchiveFormat::Ba2Dx10 => bail!(
            "BA2 DX10 texture extraction is not supported for '{}'",
            entry.path
        ),
    }
}

pub(super) fn extract_entry_to_writer(
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
