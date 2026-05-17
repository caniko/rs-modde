use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{Cursor, Read as _, Seek, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

const COPY_BUFFER: usize = 1 << 20;

#[derive(Debug, Clone)]
pub enum ArchiveRequestKind {
    WriteFile {
        to: PathBuf,
        expected_size: Option<u64>,
    },
    Bytes,
}

#[derive(Debug, Clone)]
pub struct ArchiveRequest {
    pub directive_index: usize,
    pub from: String,
    pub inner_path: Option<String>,
    pub kind: ArchiveRequestKind,
}

#[derive(Debug, Default)]
pub struct ArchiveBatchOutput {
    pub bytes: HashMap<usize, Vec<u8>>,
}

pub enum ArchiveInput<'a> {
    Path(&'a Path),
    Bytes { name: &'a str, bytes: &'a [u8] },
}

pub struct ArchiveBatchExtractor;

impl ArchiveBatchExtractor {
    pub fn extract_selected(
        path: &Path,
        requests: &[ArchiveRequest],
    ) -> Result<ArchiveBatchOutput> {
        Self::extract_selected_from(ArchiveInput::Path(path), requests)
    }

    pub fn extract_selected_from(
        input: ArchiveInput<'_>,
        requests: &[ArchiveRequest],
    ) -> Result<ArchiveBatchOutput> {
        for request in requests {
            validate_archive_entry(&request.from)?;
            if let Some(inner_path) = &request.inner_path {
                validate_archive_entry(inner_path)?;
            }
            if let ArchiveRequestKind::WriteFile { to, .. } = &request.kind
                && let Some(parent) = to.parent()
            {
                std::fs::create_dir_all(parent)?;
            }
        }

        match input {
            ArchiveInput::Path(path) => {
                if has_zip_magic(path).unwrap_or(false) {
                    return extract_zip(File::open(path)?, path.display().to_string(), requests);
                }

                if modde_core::bethesda_archive::ArchiveIndex::has_bethesda_magic(path)
                    .unwrap_or(false)
                {
                    return extract_bethesda(path, requests);
                }

                #[cfg(feature = "rar")]
                if has_rar_magic(path).unwrap_or(false) {
                    return extract_rar(path, requests);
                }

                #[cfg(not(feature = "rar"))]
                if has_rar_magic(path).unwrap_or(false) {
                    bail!(
                        "RAR archive detected but modde-sources was built without the rar feature"
                    );
                }

                if sevenz_rust2::Archive::open(path).is_ok() {
                    return extract_seven_z(
                        File::open(path)?,
                        path.display().to_string(),
                        requests,
                    );
                }

                bail!(
                    "unsupported archive format for {}; supported by default: zip, 7z, BSA, BA2{}",
                    path.display(),
                    if cfg!(feature = "rar") { ", rar" } else { "" }
                )
            }
            ArchiveInput::Bytes { name, bytes } => {
                if bytes_have_zip_magic(bytes) {
                    return extract_zip(Cursor::new(bytes), name.to_string(), requests);
                }

                if let Ok(output) = extract_seven_z(Cursor::new(bytes), name.to_string(), requests)
                {
                    return Ok(output);
                }

                bail!(
                    "unsupported in-memory archive format for {name}; supported in memory: zip, 7z"
                )
            }
        }
    }
}

fn extract_zip<R: std::io::Read + Seek>(
    reader: R,
    label: String,
    requests: &[ArchiveRequest],
) -> Result<ArchiveBatchOutput> {
    let mut archive = zip::ZipArchive::new(reader)
        .with_context(|| format!("failed to read zip archive {label}"))?;
    let mut output = ArchiveBatchOutput::default();
    let mut found = HashSet::new();

    for request in requests {
        let entry_name = find_zip_entry(&archive, &request.from)?;
        let mut entry = archive.by_name(&entry_name)?;
        validate_zip_entry(&entry)?;
        if request.inner_path.is_none() {
            validate_declared_entry_size(entry.size(), std::slice::from_ref(request))?;
        }
        if request.inner_path.is_some() {
            let mut data = Vec::new();
            entry.read_to_end(&mut data)?;
            satisfy_maybe_nested_requests_from_bytes(
                &data,
                std::slice::from_ref(request),
                &mut output,
            )?;
            found.insert(request.directive_index);
            continue;
        }
        match &request.kind {
            ArchiveRequestKind::WriteFile { to, expected_size } => {
                let mut out = File::create(to)
                    .with_context(|| format!("failed to create output {}", to.display()))?;
                copy_streaming(&mut entry, &mut out, *expected_size)?;
            }
            ArchiveRequestKind::Bytes => {
                let mut data = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut data)?;
                output.bytes.insert(request.directive_index, data);
            }
        }
        found.insert(request.directive_index);
    }

    ensure_all_found(&label, requests, &found)?;
    Ok(output)
}

fn extract_bethesda(path: &Path, requests: &[ArchiveRequest]) -> Result<ArchiveBatchOutput> {
    let index = modde_core::bethesda_archive::ArchiveIndex::read(path)
        .with_context(|| format!("failed to read Bethesda archive {}", path.display()))?;
    let mut output = ArchiveBatchOutput::default();
    for request in requests {
        if request.inner_path.is_some() {
            let data = index.extract_file(&request.from)?;
            satisfy_maybe_nested_requests_from_bytes(
                &data,
                std::slice::from_ref(request),
                &mut output,
            )?;
            continue;
        }
        match &request.kind {
            ArchiveRequestKind::WriteFile { to, expected_size } => {
                let mut out = File::create(to)
                    .with_context(|| format!("failed to create output {}", to.display()))?;
                let mut checked = SizeCheckedWriter::new(&mut out, *expected_size);
                index.extract_file_to_writer(&request.from, &mut checked)?;
                checked.finish()?;
            }
            ArchiveRequestKind::Bytes => {
                let data = index.extract_file(&request.from)?;
                output.bytes.insert(request.directive_index, data);
            }
        }
    }
    Ok(output)
}

fn extract_seven_z<R: std::io::Read + Seek>(
    reader: R,
    label: String,
    requests: &[ArchiveRequest],
) -> Result<ArchiveBatchOutput> {
    let by_path = requests_by_normalized_path(requests);
    let mut output = ArchiveBatchOutput::default();
    let mut found = HashSet::new();
    let mut reader = sevenz_rust2::ArchiveReader::new(reader, sevenz_rust2::Password::empty())
        .with_context(|| format!("failed to open 7z archive {label}"))?;

    reader.for_each_entries(|entry, input| {
        let key = normalize_path(entry.name()).to_lowercase();
        let Some(matched_requests) = by_path.get(&key) else {
            std::io::copy(input, &mut std::io::sink())
                .map_err(|e| sevenz_rust2::Error::Io(e, "drain skipped entry".into()))?;
            return Ok(true);
        };
        if matched_requests
            .iter()
            .all(|request| request.inner_path.is_none())
        {
            validate_declared_entry_size(entry.size(), matched_requests)
                .map_err(|e| sevenz_rust2::Error::Io(e, "validate matched entry size".into()))?;
        }
        satisfy_requests_from_reader(input, matched_requests, &mut output)
            .map_err(|e| sevenz_rust2::Error::Io(e, "extract matched entry".into()))?;
        found.extend(
            matched_requests
                .iter()
                .map(|request| request.directive_index),
        );
        Ok(true)
    })?;

    ensure_all_found(&label, requests, &found)?;
    Ok(output)
}

#[cfg(feature = "rar")]
fn extract_rar(path: &Path, requests: &[ArchiveRequest]) -> Result<ArchiveBatchOutput> {
    let by_path = requests_by_normalized_path(requests);
    let mut output = ArchiveBatchOutput::default();
    let mut found = HashSet::new();
    let mut archive = unrar::Archive::new(path)
        .open_for_processing()
        .with_context(|| format!("failed to open RAR archive {}", path.display()))?;

    while let Some(header) = archive.read_header()? {
        let key = normalize_path(&header.entry().filename.to_string_lossy()).to_lowercase();
        let Some(matched_requests) = by_path.get(&key) else {
            archive = header.skip()?;
            continue;
        };

        let (data, next) = header.read()?;
        satisfy_requests_from_bytes(&data, matched_requests, &mut output)?;
        found.extend(
            matched_requests
                .iter()
                .map(|request| request.directive_index),
        );
        archive = next;
        if found.len() == requests.len() {
            break;
        }
    }

    ensure_all_found(&path.display().to_string(), requests, &found)?;
    Ok(output)
}

fn requests_by_normalized_path(
    requests: &[ArchiveRequest],
) -> HashMap<String, Vec<ArchiveRequest>> {
    let mut by_path: HashMap<String, Vec<ArchiveRequest>> = HashMap::new();
    for request in requests {
        by_path
            .entry(normalize_path(&request.from).to_lowercase())
            .or_default()
            .push(request.clone());
    }
    by_path
}

fn satisfy_requests_from_reader(
    input: &mut dyn std::io::Read,
    requests: &[ArchiveRequest],
    output: &mut ArchiveBatchOutput,
) -> std::io::Result<()> {
    if requests.iter().any(|request| request.inner_path.is_some()) {
        let mut data = Vec::new();
        input.read_to_end(&mut data)?;
        satisfy_maybe_nested_requests_from_bytes(&data, requests, output)?;
        return Ok(());
    }

    if requests
        .iter()
        .any(|request| matches!(request.kind, ArchiveRequestKind::Bytes))
    {
        let data = read_to_vec(input, expected_write_size(requests)?)?;
        satisfy_requests_from_bytes(&data, requests, output)?;
        return Ok(());
    }

    let mut writers = Vec::new();
    for request in requests {
        if let ArchiveRequestKind::WriteFile { to, .. } = &request.kind {
            writers.push(File::create(to)?);
        }
    }
    copy_streaming_to_many(input, &mut writers, expected_write_size(requests)?)?;
    Ok(())
}

fn satisfy_requests_from_bytes(
    data: &[u8],
    requests: &[ArchiveRequest],
    output: &mut ArchiveBatchOutput,
) -> std::io::Result<()> {
    if requests.iter().any(|request| request.inner_path.is_some()) {
        satisfy_maybe_nested_requests_from_bytes(data, requests, output)?;
        return Ok(());
    }

    for request in requests {
        match &request.kind {
            ArchiveRequestKind::WriteFile { to, expected_size } => {
                validate_final_output_size(data.len() as u64, *expected_size)?;
                std::fs::write(to, data)?;
            }
            ArchiveRequestKind::Bytes => {
                output.bytes.insert(request.directive_index, data.to_vec());
            }
        }
    }
    Ok(())
}

fn satisfy_maybe_nested_requests_from_bytes(
    data: &[u8],
    requests: &[ArchiveRequest],
    output: &mut ArchiveBatchOutput,
) -> std::io::Result<()> {
    let mut direct_requests = Vec::new();
    let mut nested_requests = Vec::new();

    for request in requests {
        if let Some(inner_path) = &request.inner_path {
            nested_requests.push(ArchiveRequest {
                directive_index: request.directive_index,
                from: inner_path.clone(),
                inner_path: None,
                kind: request.kind.clone(),
            });
        } else {
            direct_requests.push(request.clone());
        }
    }

    if !direct_requests.is_empty() {
        satisfy_requests_from_bytes(data, &direct_requests, output)?;
    }

    if nested_requests.is_empty() {
        return Ok(());
    }

    let nested_output = if bytes_have_bethesda_magic(data) {
        let mut temp = tempfile::NamedTempFile::new()?;
        temp.write_all(data)?;
        temp.flush()?;
        ArchiveBatchExtractor::extract_selected(temp.path(), &nested_requests)
    } else {
        ArchiveBatchExtractor::extract_selected_from(
            ArchiveInput::Bytes {
                name: "nested archive",
                bytes: data,
            },
            &nested_requests,
        )
    }
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{e:#}")))?;

    output.bytes.extend(nested_output.bytes);
    Ok(())
}

fn copy_streaming(
    input: &mut dyn std::io::Read,
    output: &mut dyn std::io::Write,
    expected_size: Option<u64>,
) -> std::io::Result<u64> {
    let mut buf = vec![0_u8; COPY_BUFFER];
    let mut total = 0_u64;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        validate_output_size(total, expected_size)?;
        output.write_all(&buf[..n])?;
    }
    output.flush()?;
    validate_final_output_size(total, expected_size)?;
    Ok(total)
}

fn copy_streaming_to_many(
    input: &mut dyn std::io::Read,
    outputs: &mut [File],
    expected_size: Option<u64>,
) -> std::io::Result<u64> {
    let mut buf = vec![0_u8; COPY_BUFFER];
    let mut total = 0_u64;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        validate_output_size(total, expected_size)?;
        for output in outputs.iter_mut() {
            output.write_all(&buf[..n])?;
        }
    }
    for output in outputs {
        output.flush()?;
    }
    validate_final_output_size(total, expected_size)?;
    Ok(total)
}

fn read_to_vec(
    input: &mut dyn std::io::Read,
    expected_size: Option<u64>,
) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0_u8; COPY_BUFFER];
    let mut data = Vec::new();
    let mut total = 0_u64;
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        validate_output_size(total, expected_size)?;
        data.extend_from_slice(&buf[..n]);
    }
    validate_final_output_size(total, expected_size)?;
    Ok(data)
}

fn expected_write_size(requests: &[ArchiveRequest]) -> std::io::Result<Option<u64>> {
    let mut expected = None;
    for request in requests {
        let ArchiveRequestKind::WriteFile {
            expected_size: Some(size),
            ..
        } = request.kind
        else {
            continue;
        };
        if let Some(previous) = expected
            && previous != size
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "duplicate archive entry has inconsistent expected sizes: {previous} and {size}"
                ),
            ));
        }
        expected = Some(size);
    }
    Ok(expected)
}

fn validate_declared_entry_size(actual: u64, requests: &[ArchiveRequest]) -> std::io::Result<()> {
    if let Some(expected) = expected_write_size(requests)?
        && actual != expected
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("archive entry size mismatch: expected {expected}, got {actual}"),
        ));
    }
    Ok(())
}

fn validate_output_size(actual: u64, expected: Option<u64>) -> std::io::Result<()> {
    if let Some(expected) = expected
        && actual > expected
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "archive entry output exceeds expected size: expected {expected}, got {actual}"
            ),
        ));
    }
    Ok(())
}

fn validate_final_output_size(actual: u64, expected: Option<u64>) -> std::io::Result<()> {
    validate_output_size(actual, expected)?;
    if let Some(expected) = expected
        && actual != expected
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("archive entry output size mismatch: expected {expected}, got {actual}"),
        ));
    }
    Ok(())
}

struct SizeCheckedWriter<W> {
    inner: W,
    expected_size: Option<u64>,
    written: u64,
}

impl<W> SizeCheckedWriter<W> {
    fn new(inner: W, expected_size: Option<u64>) -> Self {
        Self {
            inner,
            expected_size,
            written: 0,
        }
    }

    fn finish(&self) -> std::io::Result<()> {
        validate_final_output_size(self.written, self.expected_size)
    }
}

impl<W: std::io::Write> std::io::Write for SizeCheckedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let next = self.written.saturating_add(buf.len() as u64);
        validate_output_size(next, self.expected_size)?;
        let written = self.inner.write(buf)?;
        self.written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn ensure_all_found(
    label: &str,
    requests: &[ArchiveRequest],
    found: &HashSet<usize>,
) -> Result<()> {
    let missing: Vec<&ArchiveRequest> = requests
        .iter()
        .filter(|request| !found.contains(&request.directive_index))
        .collect();
    if !missing.is_empty() {
        let mut unique = BTreeMap::<&str, usize>::new();
        for request in &missing {
            *unique.entry(request.from.as_str()).or_default() += 1;
        }
        let shown = unique
            .into_iter()
            .map(|(path, count)| {
                if count == 1 {
                    path.to_string()
                } else {
                    format!("{path} (x{count})")
                }
            })
            .collect::<Vec<_>>();
        bail!(
            "{} requested entr{} missing from {} ({} unique): {}",
            missing.len(),
            if missing.len() == 1 { "y" } else { "ies" },
            label,
            shown.len(),
            shown.join(", ")
        );
    }
    Ok(())
}

fn has_rar_magic(path: &Path) -> std::io::Result<bool> {
    let mut file = File::open(path)?;
    let mut magic = [0_u8; 8];
    let len = file.read(&mut magic)?;
    Ok(magic[..len].starts_with(b"Rar!\x1A\x07\x00")
        || magic[..len].starts_with(b"Rar!\x1A\x07\x01\x00"))
}

fn has_zip_magic(path: &Path) -> std::io::Result<bool> {
    let mut file = File::open(path)?;
    let mut magic = [0_u8; 4];
    let len = file.read(&mut magic)?;
    Ok(bytes_have_zip_magic(&magic[..len]))
}

fn bytes_have_zip_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x07\x08")
}

fn bytes_have_bethesda_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(b"BSA\0") || bytes.starts_with(b"BTDX")
}

fn find_zip_entry<R: std::io::Read + Seek>(
    archive: &zip::ZipArchive<R>,
    path: &str,
) -> Result<String> {
    let normalized = normalize_path(path);
    let backslash = path.replace('/', "\\");
    for i in 0..archive.len() {
        let name = archive.name_for_index(i).unwrap_or_default();
        if name == path || name == normalized || name == backslash {
            return Ok(name.to_string());
        }
        let lower = name.to_lowercase();
        if lower == path.to_lowercase()
            || lower == normalized.to_lowercase()
            || lower == backslash.to_lowercase()
        {
            return Ok(name.to_string());
        }
    }
    bail!("entry not found: {path}");
}

fn validate_zip_entry<R: std::io::Read + ?Sized>(entry: &zip::read::ZipFile<'_, R>) -> Result<()> {
    validate_archive_entry(entry.name())?;
    if entry.is_symlink() {
        bail!("archive entry is a symlink (rejected): {}", entry.name());
    }
    Ok(())
}

fn validate_archive_entry(name: &str) -> Result<()> {
    let normalized = normalize_path(name);
    if normalized.starts_with('/') {
        bail!("archive entry contains absolute path: {name}");
    }
    if normalized.split('/').any(|component| component == "..") {
        bail!("archive entry contains path traversal: {name}");
    }
    Ok(())
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, data) in entries {
                zip.start_file(*name, options).unwrap();
                zip.write_all(data).unwrap();
            }
            zip.finish().unwrap();
        }
        cursor.into_inner()
    }

    fn write_7z(path: &Path, entries: &[(&str, &[u8])]) {
        let temp = tempfile::tempdir().unwrap();
        for (name, data) in entries {
            let file_path = temp.path().join(name.replace('\\', "/"));
            std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
            std::fs::write(file_path, data).unwrap();
        }

        let status = Command::new("7zz")
            .arg("a")
            .arg("-t7z")
            .arg("-mx=1")
            .arg(path)
            .arg(".")
            .current_dir(temp.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "7zz failed to create fixture archive");
    }

    #[test]
    fn zip_batch_writes_files_and_returns_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("fixture.zip");
        write_zip(
            &archive,
            &[("Data/A.txt", b"alpha"), ("Data/B.txt", b"beta")],
        );
        let out = temp.path().join("out").join("a.txt");

        let output = ArchiveBatchExtractor::extract_selected(
            &archive,
            &[
                ArchiveRequest {
                    directive_index: 7,
                    from: "data/a.txt".to_string(),
                    inner_path: None,
                    kind: ArchiveRequestKind::WriteFile {
                        to: out.clone(),
                        expected_size: None,
                    },
                },
                ArchiveRequest {
                    directive_index: 9,
                    from: "Data/B.txt".to_string(),
                    inner_path: None,
                    kind: ArchiveRequestKind::Bytes,
                },
            ],
        )
        .unwrap();

        assert_eq!(std::fs::read(out).unwrap(), b"alpha");
        assert_eq!(output.bytes.get(&9).unwrap(), b"beta");
    }

    #[test]
    fn zip_write_rejects_entry_larger_than_expected_size() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("fixture.zip");
        write_zip(&archive, &[("Data/A.txt", b"alpha")]);
        let out = temp.path().join("out").join("a.txt");

        let err = ArchiveBatchExtractor::extract_selected(
            &archive,
            &[ArchiveRequest {
                directive_index: 7,
                from: "data/a.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::WriteFile {
                    to: out.clone(),
                    expected_size: Some(3),
                },
            }],
        )
        .unwrap_err();

        assert!(format!("{err:#}").contains("entry size mismatch"));
    }

    #[test]
    fn zip_write_rejects_entry_smaller_than_expected_size() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("fixture.zip");
        write_zip(&archive, &[("Data/A.txt", b"alpha")]);
        let out = temp.path().join("out").join("a.txt");

        let err = ArchiveBatchExtractor::extract_selected(
            &archive,
            &[ArchiveRequest {
                directive_index: 7,
                from: "data/a.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::WriteFile {
                    to: out.clone(),
                    expected_size: Some(6),
                },
            }],
        )
        .unwrap_err();

        assert!(format!("{err:#}").contains("entry size mismatch"));
    }

    #[test]
    fn traversal_request_is_rejected_before_writing() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("fixture.zip");
        write_zip(&archive, &[("safe.txt", b"ok")]);
        let out = temp.path().join("out.txt");

        let err = ArchiveBatchExtractor::extract_selected(
            &archive,
            &[ArchiveRequest {
                directive_index: 1,
                from: "../escape.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::WriteFile {
                    to: out.clone(),
                    expected_size: Some(10),
                },
            }],
        )
        .unwrap_err();

        assert!(format!("{err:#}").contains("path traversal"));
        assert!(!out.exists());
    }

    #[test]
    fn in_memory_zip_batch_writes_files_and_returns_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let archive = zip_bytes(&[("Data/A.txt", b"alpha"), ("Data/B.txt", b"beta")]);
        let out = temp.path().join("out").join("a.txt");

        let output = ArchiveBatchExtractor::extract_selected_from(
            ArchiveInput::Bytes {
                name: "fixture.zip",
                bytes: &archive,
            },
            &[
                ArchiveRequest {
                    directive_index: 7,
                    from: "data/a.txt".to_string(),
                    inner_path: None,
                    kind: ArchiveRequestKind::WriteFile {
                        to: out.clone(),
                        expected_size: None,
                    },
                },
                ArchiveRequest {
                    directive_index: 9,
                    from: "Data/B.txt".to_string(),
                    inner_path: None,
                    kind: ArchiveRequestKind::Bytes,
                },
            ],
        )
        .unwrap();

        assert_eq!(std::fs::read(out).unwrap(), b"alpha");
        assert_eq!(output.bytes.get(&9).unwrap(), b"beta");
    }

    #[test]
    fn seven_z_duplicate_write_requests_share_one_entry() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("fixture.7z");
        write_7z(&archive, &[("Data/Dupe.txt", b"same bytes")]);
        let out_a = temp.path().join("out-a.txt");
        let out_b = temp.path().join("out-b.txt");

        ArchiveBatchExtractor::extract_selected(
            &archive,
            &[
                ArchiveRequest {
                    directive_index: 1,
                    from: "data/dupe.txt".to_string(),
                    inner_path: None,
                    kind: ArchiveRequestKind::WriteFile {
                        to: out_a.clone(),
                        expected_size: None,
                    },
                },
                ArchiveRequest {
                    directive_index: 2,
                    from: "Data\\Dupe.txt".to_string(),
                    inner_path: None,
                    kind: ArchiveRequestKind::WriteFile {
                        to: out_b.clone(),
                        expected_size: None,
                    },
                },
            ],
        )
        .unwrap();

        assert_eq!(std::fs::read(out_a).unwrap(), b"same bytes");
        assert_eq!(std::fs::read(out_b).unwrap(), b"same bytes");
    }

    #[test]
    fn seven_z_duplicate_bytes_and_write_requests_share_one_entry() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("fixture.7z");
        write_7z(&archive, &[("Data/Dupe.txt", b"shared bytes")]);
        let out = temp.path().join("out.txt");

        let output = ArchiveBatchExtractor::extract_selected(
            &archive,
            &[
                ArchiveRequest {
                    directive_index: 1,
                    from: "Data/Dupe.txt".to_string(),
                    inner_path: None,
                    kind: ArchiveRequestKind::Bytes,
                },
                ArchiveRequest {
                    directive_index: 2,
                    from: "data\\dupe.txt".to_string(),
                    inner_path: None,
                    kind: ArchiveRequestKind::WriteFile {
                        to: out.clone(),
                        expected_size: None,
                    },
                },
            ],
        )
        .unwrap();

        assert_eq!(output.bytes.get(&1).unwrap(), b"shared bytes");
        assert_eq!(std::fs::read(out).unwrap(), b"shared bytes");
    }

    #[test]
    fn seven_z_duplicate_bytes_and_write_rejects_size_mismatch() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("fixture.7z");
        write_7z(&archive, &[("Data/Dupe.txt", b"shared bytes")]);
        let out = temp.path().join("out.txt");

        let err = ArchiveBatchExtractor::extract_selected(
            &archive,
            &[
                ArchiveRequest {
                    directive_index: 1,
                    from: "Data/Dupe.txt".to_string(),
                    inner_path: None,
                    kind: ArchiveRequestKind::Bytes,
                },
                ArchiveRequest {
                    directive_index: 2,
                    from: "data\\dupe.txt".to_string(),
                    inner_path: None,
                    kind: ArchiveRequestKind::WriteFile {
                        to: out.clone(),
                        expected_size: Some(3),
                    },
                },
            ],
        )
        .unwrap_err();

        let msg = format!("{err:#}");
        assert!(msg.contains("exceeds expected size") || msg.contains("entry size mismatch"));
        assert!(!out.exists());
    }

    #[tokio::test]
    async fn zip_entry_can_satisfy_nested_bsa_request() {
        let temp = tempfile::tempdir().unwrap();
        let bsa_root = temp.path().join("bsa-root");
        let source_path = bsa_root.join("meshes/actors/test.nif");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(&source_path, b"nested nif").unwrap();

        let bsa_path = temp.path().join("inner.bsa");
        crate::wabbajack::bsa_repack::create_bsa(
            &[modde_core::manifest::wabbajack::BSAFileState {
                path: "meshes/actors/test.nif".to_string(),
                hash: 0,
                size: 10,
            }],
            &bsa_root,
            &bsa_path,
        )
        .await
        .unwrap();

        let archive = temp.path().join("outer.zip");
        let bsa_bytes = std::fs::read(&bsa_path).unwrap();
        write_zip(&archive, &[("Inner.bsa", &bsa_bytes)]);
        let out = temp.path().join("out.nif");

        ArchiveBatchExtractor::extract_selected(
            &archive,
            &[ArchiveRequest {
                directive_index: 42,
                from: "inner.bsa".to_string(),
                inner_path: Some("meshes\\actors\\test.nif".to_string()),
                kind: ArchiveRequestKind::WriteFile {
                    to: out.clone(),
                    expected_size: None,
                },
            }],
        )
        .unwrap();

        assert_eq!(std::fs::read(out).unwrap(), b"nested nif");
    }

    #[test]
    fn missing_entry_error_deduplicates_repeated_paths() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("fixture.7z");
        write_7z(&archive, &[("Data/Present.txt", b"present")]);

        let err = ArchiveBatchExtractor::extract_selected(
            &archive,
            &[
                ArchiveRequest {
                    directive_index: 1,
                    from: "Data/Missing.txt".to_string(),
                    inner_path: None,
                    kind: ArchiveRequestKind::Bytes,
                },
                ArchiveRequest {
                    directive_index: 2,
                    from: "Data/Missing.txt".to_string(),
                    inner_path: None,
                    kind: ArchiveRequestKind::Bytes,
                },
            ],
        )
        .unwrap_err();
        let msg = format!("{err:#}");

        assert!(msg.contains("2 requested entries missing"));
        assert!(msg.contains("1 unique"));
        assert!(msg.contains("Data/Missing.txt (x2)"));
    }

    #[test]
    #[cfg(not(feature = "rar"))]
    fn rar_magic_without_rar_feature_reports_explicit_error() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("fixture.archive");
        std::fs::write(&archive, b"Rar!\x1A\x07\x01\x00not a complete rar").unwrap();

        let err = ArchiveBatchExtractor::extract_selected(
            &archive,
            &[ArchiveRequest {
                directive_index: 1,
                from: "file.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::Bytes,
            }],
        )
        .unwrap_err();

        assert!(
            format!("{err:#}").contains(
                "RAR archive detected but modde-sources was built without the rar feature"
            )
        );
    }

    #[test]
    #[cfg(feature = "rar")]
    fn rar_magic_with_archive_extension_routes_to_rar_reader() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("fixture.archive");
        std::fs::write(&archive, b"Rar!\x1A\x07\x01\x00not a complete rar").unwrap();

        let err = ArchiveBatchExtractor::extract_selected(
            &archive,
            &[ArchiveRequest {
                directive_index: 1,
                from: "file.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::Bytes,
            }],
        )
        .unwrap_err();
        let msg = format!("{err:#}");

        assert!(!msg.contains("unsupported archive format"), "{msg}");
        assert!(
            msg.contains("failed to open RAR archive") || msg.contains("requested entry missing"),
            "{msg}"
        );
    }
}
