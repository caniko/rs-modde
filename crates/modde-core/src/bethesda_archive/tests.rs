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
