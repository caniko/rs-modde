//! Native Bethesda ESP/ESM/ESL record parsing and conservative `FormID`
//! reference validation.
//!
//! The parser handles the Creation Engine container format directly: TES4
//! headers, `GRUP` nesting, records, zlib-compressed record payloads, normal
//! subrecords, and `XXXX` extended-size subrecords. Reference extraction is
//! intentionally data-driven and narrow; schema entries are added only when a
//! fixture verifies the field layout used by the validator.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use flate2::read::ZlibDecoder;

use super::plugin_header;

const TES4_SIGNATURE: &[u8; 4] = b"TES4";
const GRUP_SIGNATURE: &[u8; 4] = b"GRUP";
const COMPRESSED_RECORD_FLAG: u32 = 0x0004_0000;
const ESL_FLAG: u32 = plugin_header::flags::ESL;

/// Validation result for record-level reference resolution.
#[derive(Debug, Clone, Default)]
pub struct RecordValidationReport {
    pub parsed_plugins: usize,
    pub unresolved: Vec<UnresolvedFormReference>,
    pub parse_errors: Vec<RecordParseFailure>,
    pub unsupported_record_types: Vec<String>,
}

impl RecordValidationReport {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.unresolved.is_empty() && self.parse_errors.is_empty()
    }
}

/// A reference whose target `FormID` could not be resolved in the active load
/// order plus the source plugin's masters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedFormReference {
    pub source_plugin: String,
    pub source_record: String,
    pub source_form_id: u32,
    pub subrecord: String,
    pub target_form_id: u32,
    pub expected_plugin: Option<String>,
    pub mod_index_out_of_range: bool,
}

/// An active plugin that could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordParseFailure {
    pub plugin: String,
    pub path: PathBuf,
    pub error: String,
}

#[derive(Debug, Clone)]
struct PluginDocument {
    filename: String,
    masters: Vec<String>,
    is_light: bool,
    records: Vec<ParsedRecord>,
}

#[derive(Debug, Clone)]
struct ParsedRecord {
    signature: [u8; 4],
    form_id: u32,
    references: Vec<RecordReference>,
}

#[derive(Debug, Clone)]
struct RecordReference {
    subrecord: [u8; 4],
    target_form_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ResolvedFormKey {
    plugin: String,
    object_id: u32,
    compact: bool,
}

#[derive(Debug, Clone)]
struct ResolvedReference {
    key: Option<ResolvedFormKey>,
    expected_plugin: Option<String>,
    mod_index_out_of_range: bool,
}

#[derive(Debug, Clone, Copy)]
struct ReferenceSchema {
    record: &'static [u8; 4],
    subrecord: &'static [u8; 4],
    offset: usize,
    stride: Option<usize>,
}

// Schema source policy:
// - `REFR`/`ACHR`/`ACRE` `NAME`, `CONT`/`NPC_`/`CREA` `CNTO`, and `WEAP`
//   `CNAM` layouts are
//   verified by binary fixtures in this module's tests. They intentionally
//   cover only FormID slots whose fixture layout is explicit.
const REFERENCE_SCHEMAS: &[ReferenceSchema] = &[
    ReferenceSchema {
        record: b"REFR",
        subrecord: b"NAME",
        offset: 0,
        stride: None,
    },
    ReferenceSchema {
        record: b"ACHR",
        subrecord: b"NAME",
        offset: 0,
        stride: None,
    },
    ReferenceSchema {
        record: b"ACRE",
        subrecord: b"NAME",
        offset: 0,
        stride: None,
    },
    ReferenceSchema {
        record: b"CONT",
        subrecord: b"CNTO",
        offset: 0,
        stride: Some(8),
    },
    ReferenceSchema {
        record: b"NPC_",
        subrecord: b"CNTO",
        offset: 0,
        stride: Some(8),
    },
    ReferenceSchema {
        record: b"CREA",
        subrecord: b"CNTO",
        offset: 0,
        stride: Some(8),
    },
    ReferenceSchema {
        record: b"WEAP",
        subrecord: b"CNAM",
        offset: 0,
        stride: None,
    },
];

const COMMON_UNSUPPORTED_RECORD_TYPES: &[&str] = &["ARMO", "LVLI", "FLST"];

/// Validate active Bethesda plugins for unresolved record-level `FormID`
/// references.
#[must_use]
pub fn validate_record_references(
    data_dir: &Path,
    active_plugins: &[&str],
    _game_id: &str,
) -> RecordValidationReport {
    let active_plugins = active_plugins
        .iter()
        .copied()
        .filter(|plugin| is_bethesda_plugin(plugin))
        .collect::<Vec<_>>();
    if active_plugins.is_empty() {
        return RecordValidationReport::default();
    }

    let active_by_lower = active_plugins
        .iter()
        .map(|plugin| (plugin.to_lowercase(), (*plugin).to_string()))
        .collect::<HashMap<_, _>>();

    let mut docs = Vec::new();
    let mut parse_errors = Vec::new();

    for plugin in &active_plugins {
        let path = data_dir.join(plugin);
        if !path.exists() {
            parse_errors.push(RecordParseFailure {
                plugin: (*plugin).to_string(),
                path,
                error: "active plugin file is missing".to_string(),
            });
            continue;
        }

        match parse_plugin_document(&path) {
            Ok(doc) => docs.push(doc),
            Err(err) => parse_errors.push(RecordParseFailure {
                plugin: (*plugin).to_string(),
                path,
                error: err.to_string(),
            }),
        }
    }

    let mut defined = HashSet::new();
    let light_by_lower = docs
        .iter()
        .map(|doc| (doc.filename.to_lowercase(), doc.is_light))
        .collect::<HashMap<_, _>>();
    for doc in &docs {
        for record in &doc.records {
            if let Some(key) = definition_key(doc, record.form_id) {
                defined.insert(key);
            }
        }
    }

    let unresolved = docs
        .iter()
        .flat_map(|doc| {
            doc.records.iter().flat_map(|record| {
                record.references.iter().filter_map(|reference| {
                    if reference.target_form_id == 0 {
                        return None;
                    }
                    let resolved = resolve_reference(
                        doc,
                        reference.target_form_id,
                        &active_by_lower,
                        &light_by_lower,
                    );
                    match resolved.key {
                        Some(key) if defined.contains(&key) => None,
                        _ => Some(UnresolvedFormReference {
                            source_plugin: doc.filename.clone(),
                            source_record: signature_to_string(&record.signature),
                            source_form_id: record.form_id,
                            subrecord: signature_to_string(&reference.subrecord),
                            target_form_id: reference.target_form_id,
                            expected_plugin: resolved.expected_plugin,
                            mod_index_out_of_range: resolved.mod_index_out_of_range,
                        }),
                    }
                })
            })
        })
        .collect();
    let unsupported_record_types = unsupported_record_types(&docs);

    RecordValidationReport {
        parsed_plugins: docs.len(),
        unresolved,
        parse_errors,
        unsupported_record_types,
    }
}

fn unsupported_record_types(docs: &[PluginDocument]) -> Vec<String> {
    let supported = REFERENCE_SCHEMAS
        .iter()
        .map(|schema| signature_to_string(schema.record))
        .collect::<HashSet<_>>();
    let mut seen = docs
        .iter()
        .flat_map(|doc| doc.records.iter())
        .map(|record| signature_to_string(&record.signature))
        .filter(|signature| {
            COMMON_UNSUPPORTED_RECORD_TYPES.contains(&signature.as_str())
                || (!supported.contains(signature) && may_contain_formid_references(signature))
        })
        .collect::<Vec<_>>();
    seen.sort();
    seen.dedup();
    seen
}

fn may_contain_formid_references(signature: &str) -> bool {
    matches!(signature, "ARMO" | "LVLI" | "FLST")
}

fn parse_plugin_document(path: &Path) -> Result<PluginDocument> {
    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    parse_plugin_bytes(&filename, &bytes)
}

fn parse_plugin_bytes(filename: &str, bytes: &[u8]) -> Result<PluginDocument> {
    let mut cursor = Cursor::new(bytes);
    let header = read_record_header(&mut cursor)?;
    if &header.signature != TES4_SIGNATURE {
        bail!(
            "not a valid Bethesda plugin: expected TES4, got {}",
            signature_to_string(&header.signature)
        );
    }

    let tes4_data = read_exact_vec(&mut cursor, header.data_size as usize)?;
    let masters = parse_masters(&tes4_data)?;
    let is_light = header.flags & ESL_FLAG != 0 || filename.to_ascii_lowercase().ends_with(".esl");

    let mut records = Vec::new();
    while remaining(&cursor) >= 24 {
        parse_top_level_item(&mut cursor, bytes.len() as u64, &mut records)?;
    }

    Ok(PluginDocument {
        filename: filename.to_string(),
        masters,
        is_light,
        records,
    })
}

fn parse_top_level_item(
    cursor: &mut Cursor<&[u8]>,
    container_end: u64,
    records: &mut Vec<ParsedRecord>,
) -> Result<()> {
    let signature = peek_signature(cursor)?;
    if &signature == GRUP_SIGNATURE {
        parse_group(cursor, container_end, records)
    } else {
        let record = parse_record(cursor)?;
        if &record.signature != TES4_SIGNATURE {
            records.push(record);
        }
        Ok(())
    }
}

fn parse_group(
    cursor: &mut Cursor<&[u8]>,
    container_end: u64,
    records: &mut Vec<ParsedRecord>,
) -> Result<()> {
    let start = cursor.position();
    expect_signature(cursor, GRUP_SIGNATURE)?;
    let group_size = read_u32(cursor)?;
    if group_size < 24 {
        bail!("invalid GRUP size {group_size} at offset {start}");
    }
    let group_end = start + u64::from(group_size);
    if group_end > container_end || group_end > cursor.get_ref().len() as u64 {
        bail!("GRUP at offset {start} extends past its container");
    }

    skip(cursor, 16)?;
    while cursor.position() < group_end {
        parse_top_level_item(cursor, group_end, records)?;
    }
    Ok(())
}

fn parse_record(cursor: &mut Cursor<&[u8]>) -> Result<ParsedRecord> {
    let header = read_record_header(cursor)?;
    let raw_data = read_exact_vec(cursor, header.data_size as usize)?;
    let data = if header.flags & COMPRESSED_RECORD_FLAG != 0 {
        decompress_record(&raw_data)?
    } else {
        raw_data
    };
    let references = extract_references(&header.signature, &data)?;
    Ok(ParsedRecord {
        signature: header.signature,
        form_id: header.form_id,
        references,
    })
}

fn decompress_record(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 4 {
        bail!("compressed record payload is missing the uncompressed size");
    }
    let expected = u32::from_le_bytes(data[0..4].try_into().expect("slice length checked"));
    let mut decoder = ZlibDecoder::new(&data[4..]);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .context("failed to decompress compressed record payload")?;
    if out.len() != expected as usize {
        bail!(
            "compressed record size mismatch: expected {expected}, got {}",
            out.len()
        );
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy)]
struct RecordHeader {
    signature: [u8; 4],
    data_size: u32,
    flags: u32,
    form_id: u32,
}

fn read_record_header(cursor: &mut Cursor<&[u8]>) -> Result<RecordHeader> {
    let signature = read_signature(cursor)?;
    let data_size = read_u32(cursor)?;
    let flags = read_u32(cursor)?;
    let form_id = read_u32(cursor)?;
    skip(cursor, 8)?;
    Ok(RecordHeader {
        signature,
        data_size,
        flags,
        form_id,
    })
}

fn parse_masters(data: &[u8]) -> Result<Vec<String>> {
    let mut cursor = Cursor::new(data);
    let mut masters = Vec::new();
    let mut extended_size = None;
    while remaining(&cursor) >= 6 {
        let sig = read_signature(&mut cursor)?;
        let mut size = usize::from(read_u16(&mut cursor)?);
        if &sig == b"XXXX" {
            if size != 4 {
                bail!("invalid XXXX size {size} in TES4 header");
            }
            extended_size = Some(read_u32(&mut cursor)? as usize);
            continue;
        }
        if let Some(next_size) = extended_size.take() {
            size = next_size;
        }
        let body = read_exact_vec(&mut cursor, size)?;
        if &sig == b"MAST" {
            masters.push(nul_terminated_string(&body)?);
        }
    }
    Ok(masters)
}

fn extract_references(record: &[u8; 4], data: &[u8]) -> Result<Vec<RecordReference>> {
    let mut cursor = Cursor::new(data);
    let mut references = Vec::new();
    let mut extended_size = None;
    while remaining(&cursor) >= 6 {
        let subrecord = read_signature(&mut cursor)?;
        let mut size = usize::from(read_u16(&mut cursor)?);
        if &subrecord == b"XXXX" {
            if size != 4 {
                bail!(
                    "invalid XXXX size {size} in {}",
                    signature_to_string(record)
                );
            }
            extended_size = Some(read_u32(&mut cursor)? as usize);
            continue;
        }
        if let Some(next_size) = extended_size.take() {
            size = next_size;
        }
        let body = read_exact_vec(&mut cursor, size)?;
        for schema in REFERENCE_SCHEMAS
            .iter()
            .filter(|schema| schema.record == record && schema.subrecord == &subrecord)
        {
            collect_schema_references(schema, &body, &subrecord, &mut references);
        }
    }
    Ok(references)
}

fn collect_schema_references(
    schema: &ReferenceSchema,
    body: &[u8],
    subrecord: &[u8; 4],
    references: &mut Vec<RecordReference>,
) {
    let Some(stride) = schema.stride else {
        if let Some(form_id) = read_form_id_at(body, schema.offset) {
            references.push(RecordReference {
                subrecord: *subrecord,
                target_form_id: form_id,
            });
        }
        return;
    };

    let mut offset = schema.offset;
    while offset + 4 <= body.len() {
        if let Some(form_id) = read_form_id_at(body, offset) {
            references.push(RecordReference {
                subrecord: *subrecord,
                target_form_id: form_id,
            });
        }
        offset = match offset.checked_add(stride) {
            Some(next) => next,
            None => break,
        };
    }
}

fn read_form_id_at(body: &[u8], offset: usize) -> Option<u32> {
    body.get(offset..offset + 4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("slice length checked")))
}

fn definition_key(doc: &PluginDocument, form_id: u32) -> Option<ResolvedFormKey> {
    if form_id == 0 {
        return None;
    }
    Some(ResolvedFormKey {
        plugin: doc.filename.to_lowercase(),
        object_id: object_id_for_plugin(doc.is_light, form_id),
        compact: doc.is_light,
    })
}

fn resolve_reference(
    source: &PluginDocument,
    form_id: u32,
    active_by_lower: &HashMap<String, String>,
    light_by_lower: &HashMap<String, bool>,
) -> ResolvedReference {
    let raw_index = (form_id >> 24) as usize;
    let target_name = match raw_index.cmp(&source.masters.len()) {
        std::cmp::Ordering::Less => Some(source.masters[raw_index].clone()),
        std::cmp::Ordering::Equal => Some(source.filename.clone()),
        std::cmp::Ordering::Greater => None,
    };

    let Some(target_name) = target_name else {
        return ResolvedReference {
            key: None,
            expected_plugin: None,
            mod_index_out_of_range: true,
        };
    };

    let target_lower = target_name.to_lowercase();
    let Some(active_name) = active_by_lower.get(&target_lower) else {
        return ResolvedReference {
            key: None,
            expected_plugin: Some(target_name),
            mod_index_out_of_range: false,
        };
    };

    let compact = light_by_lower
        .get(&target_lower)
        .copied()
        .unwrap_or_else(|| active_name.to_ascii_lowercase().ends_with(".esl"))
        || form_id >> 24 == 0xfe;
    ResolvedReference {
        key: Some(ResolvedFormKey {
            plugin: target_lower,
            object_id: object_id_for_plugin(compact, form_id),
            compact,
        }),
        expected_plugin: Some(active_name.clone()),
        mod_index_out_of_range: false,
    }
}

fn object_id_for_plugin(light: bool, form_id: u32) -> u32 {
    if light {
        form_id & 0x0000_0fff
    } else {
        form_id & 0x00ff_ffff
    }
}

fn is_bethesda_plugin(plugin: &str) -> bool {
    Path::new(plugin)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "esp" | "esm" | "esl"))
}

fn nul_terminated_string(bytes: &[u8]) -> Result<String> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8(bytes[..end].to_vec()).context("invalid UTF-8 string in plugin header")
}

fn peek_signature(cursor: &mut Cursor<&[u8]>) -> Result<[u8; 4]> {
    let pos = cursor.position();
    let signature = read_signature(cursor)?;
    cursor.set_position(pos);
    Ok(signature)
}

fn expect_signature(cursor: &mut Cursor<&[u8]>, expected: &[u8; 4]) -> Result<()> {
    let found = read_signature(cursor)?;
    if &found != expected {
        bail!(
            "expected {}, got {}",
            signature_to_string(expected),
            signature_to_string(&found)
        );
    }
    Ok(())
}

fn read_signature(cursor: &mut Cursor<&[u8]>) -> Result<[u8; 4]> {
    let mut buf = [0u8; 4];
    cursor.read_exact(&mut buf)?;
    Ok(buf)
}

fn read_u32(cursor: &mut Cursor<&[u8]>) -> Result<u32> {
    let mut buf = [0u8; 4];
    cursor.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u16(cursor: &mut Cursor<&[u8]>) -> Result<u16> {
    let mut buf = [0u8; 2];
    cursor.read_exact(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

fn read_exact_vec(cursor: &mut Cursor<&[u8]>, len: usize) -> Result<Vec<u8>> {
    if remaining(cursor) < len as u64 {
        return Err(anyhow!(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!("needed {len} bytes, only {} remain", remaining(cursor))
        )));
    }
    let mut buf = vec![0u8; len];
    cursor.read_exact(&mut buf)?;
    Ok(buf)
}

fn skip(cursor: &mut Cursor<&[u8]>, len: u64) -> Result<()> {
    let next = cursor.position() + len;
    if next > cursor.get_ref().len() as u64 {
        bail!("truncated plugin data while skipping {len} bytes");
    }
    cursor.set_position(next);
    Ok(())
}

fn remaining(cursor: &Cursor<&[u8]>) -> u64 {
    cursor
        .get_ref()
        .len()
        .saturating_sub(cursor.position() as usize) as u64
}

fn signature_to_string(signature: &[u8; 4]) -> String {
    String::from_utf8_lossy(signature).to_string()
}

impl fmt::Display for UnresolvedFormReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.expected_plugin {
            _ if self.mod_index_out_of_range => write!(
                f,
                "{} {} {:08X} references FormID {:08X} with a mod index beyond the master list via {}",
                self.source_plugin,
                self.source_record,
                self.source_form_id,
                self.target_form_id,
                self.subrecord
            ),
            Some(plugin) => write!(
                f,
                "{} {} {:08X} references unresolved {:08X} in {} via {}",
                self.source_plugin,
                self.source_record,
                self.source_form_id,
                self.target_form_id,
                plugin,
                self.subrecord
            ),
            None => write!(
                f,
                "{} {} {:08X} references unresolved {:08X} via {}",
                self.source_plugin,
                self.source_record,
                self.source_form_id,
                self.target_form_id,
                self.subrecord
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    fn plugin(record_flags: u32, masters: &[&str], records: Vec<Vec<u8>>) -> Vec<u8> {
        let mut tes4 = Vec::new();
        tes4.extend(subrecord(b"HEDR", &[0; 12]));
        for master in masters {
            let mut body = master.as_bytes().to_vec();
            body.push(0);
            tes4.extend(subrecord(b"MAST", &body));
            tes4.extend(subrecord(b"DATA", &[0; 8]));
        }

        let mut bytes = record(b"TES4", record_flags, 0, tes4);
        for record in records {
            bytes.extend(record);
        }
        bytes
    }

    fn record(sig: &[u8; 4], flags: u32, form_id: u32, data: Vec<u8>) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(sig);
        bytes.extend(&(data.len() as u32).to_le_bytes());
        bytes.extend(&flags.to_le_bytes());
        bytes.extend(&form_id.to_le_bytes());
        bytes.extend(&0u32.to_le_bytes());
        bytes.extend(&44u16.to_le_bytes());
        bytes.extend(&0u16.to_le_bytes());
        bytes.extend(data);
        bytes
    }

    fn compressed_record(sig: &[u8; 4], form_id: u32, data: Vec<u8>) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&data).unwrap();
        let compressed = encoder.finish().unwrap();
        let mut payload = Vec::new();
        payload.extend(&(data.len() as u32).to_le_bytes());
        payload.extend(compressed);
        record(sig, COMPRESSED_RECORD_FLAG, form_id, payload)
    }

    fn group(records: Vec<Vec<u8>>) -> Vec<u8> {
        let data_len: usize = records.iter().map(Vec::len).sum();
        let size = 24 + data_len;
        let mut bytes = Vec::new();
        bytes.extend(b"GRUP");
        bytes.extend(&(size as u32).to_le_bytes());
        bytes.extend(b"WRLD");
        bytes.extend(&0u32.to_le_bytes());
        bytes.extend(&0u32.to_le_bytes());
        bytes.extend(&0u32.to_le_bytes());
        for record in records {
            bytes.extend(record);
        }
        bytes
    }

    fn subrecord(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(sig);
        bytes.extend(&(body.len() as u16).to_le_bytes());
        bytes.extend(body);
        bytes
    }

    fn extended_subrecord(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(b"XXXX");
        bytes.extend(&4u16.to_le_bytes());
        bytes.extend(&(body.len() as u32).to_le_bytes());
        bytes.extend(sig);
        bytes.extend(&0u16.to_le_bytes());
        bytes.extend(body);
        bytes
    }

    fn name_ref(form_id: u32) -> Vec<u8> {
        subrecord(b"NAME", &form_id.to_le_bytes())
    }

    fn cnto_ref(form_id: u32, count: u32) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend(&form_id.to_le_bytes());
        body.extend(&count.to_le_bytes());
        subrecord(b"CNTO", &body)
    }

    fn cnam_ref(form_id: u32) -> Vec<u8> {
        subrecord(b"CNAM", &form_id.to_le_bytes())
    }

    #[test]
    fn parses_tes4_header_and_masters() {
        let bytes = plugin(0, &["Skyrim.esm", "Update.esm"], Vec::new());
        let doc = parse_plugin_bytes("Test.esp", &bytes).unwrap();
        assert_eq!(doc.masters, vec!["Skyrim.esm", "Update.esm"]);
        assert!(doc.records.is_empty());
    }

    #[test]
    fn resolves_reference_to_same_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        let target = record(b"CONT", 0, 0x0100_0800, Vec::new());
        let reference = record(b"REFR", 0, 0x0100_0801, name_ref(0x0100_0800));
        std::fs::write(
            tmp.path().join("Self.esp"),
            plugin(0, &["Skyrim.esm"], vec![target, reference]),
        )
        .unwrap();

        let report = validate_record_references(tmp.path(), &["Self.esp"], "skyrim-se");
        assert!(report.is_empty(), "{report:?}");
    }

    #[test]
    fn reports_unresolved_reference_in_present_master() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("Skyrim.esm"),
            plugin(plugin_header::flags::ESM, &[], Vec::new()),
        )
        .unwrap();
        let reference = record(b"REFR", 0, 0x0100_0800, name_ref(0x0000_1234));
        std::fs::write(
            tmp.path().join("Broken.esp"),
            plugin(0, &["Skyrim.esm"], vec![reference]),
        )
        .unwrap();

        let report =
            validate_record_references(tmp.path(), &["Skyrim.esm", "Broken.esp"], "skyrim-se");
        assert_eq!(report.unresolved.len(), 1);
        assert_eq!(
            report.unresolved[0].expected_plugin.as_deref(),
            Some("Skyrim.esm")
        );
        assert_eq!(report.unresolved[0].subrecord, "NAME");
    }

    #[test]
    fn disabled_plugin_is_not_a_valid_target() {
        let tmp = tempfile::tempdir().unwrap();
        let target = record(b"CONT", 0, 0x0000_0800, Vec::new());
        std::fs::write(
            tmp.path().join("Disabled.esm"),
            plugin(plugin_header::flags::ESM, &[], vec![target]),
        )
        .unwrap();
        let reference = record(b"REFR", 0, 0x0100_0800, name_ref(0x0000_0800));
        std::fs::write(
            tmp.path().join("Broken.esp"),
            plugin(0, &["Disabled.esm"], vec![reference]),
        )
        .unwrap();

        let report = validate_record_references(tmp.path(), &["Broken.esp"], "skyrim-se");
        assert_eq!(report.unresolved.len(), 1);
        assert_eq!(
            report.unresolved[0].expected_plugin.as_deref(),
            Some("Disabled.esm")
        );
    }

    #[test]
    fn parses_nested_group_records() {
        let tmp = tempfile::tempdir().unwrap();
        let target = record(b"CONT", 0, 0x0000_0800, Vec::new());
        let reference = record(b"REFR", 0, 0x0000_0801, name_ref(0x0000_0800));
        std::fs::write(
            tmp.path().join("Grouped.esm"),
            plugin(
                plugin_header::flags::ESM,
                &[],
                vec![group(vec![group(vec![target, reference])])],
            ),
        )
        .unwrap();

        let report = validate_record_references(tmp.path(), &["Grouped.esm"], "skyrim-se");
        assert!(report.is_empty(), "{report:?}");
    }

    #[test]
    fn parses_compressed_record_references() {
        let tmp = tempfile::tempdir().unwrap();
        let target = record(b"CONT", 0, 0x0000_0800, Vec::new());
        let reference = compressed_record(b"REFR", 0x0000_0801, name_ref(0x0000_0800));
        std::fs::write(
            tmp.path().join("Compressed.esm"),
            plugin(plugin_header::flags::ESM, &[], vec![target, reference]),
        )
        .unwrap();

        let report = validate_record_references(tmp.path(), &["Compressed.esm"], "skyrim-se");
        assert!(report.is_empty(), "{report:?}");
    }

    #[test]
    fn parses_extended_subrecord_sizes() {
        let tmp = tempfile::tempdir().unwrap();
        let target = record(b"MISC", 0, 0x0000_0800, Vec::new());
        let mut body = Vec::new();
        body.extend(&0x0000_0800u32.to_le_bytes());
        body.extend(&1u32.to_le_bytes());
        let reference = record(b"CONT", 0, 0x0000_0801, extended_subrecord(b"CNTO", &body));
        std::fs::write(
            tmp.path().join("Extended.esm"),
            plugin(plugin_header::flags::ESM, &[], vec![target, reference]),
        )
        .unwrap();

        let report = validate_record_references(tmp.path(), &["Extended.esm"], "skyrim-se");
        assert!(report.is_empty(), "{report:?}");
    }

    #[test]
    fn malformed_plugin_reports_parse_failure() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Bad.esp"), b"TES4").unwrap();
        let report = validate_record_references(tmp.path(), &["Bad.esp"], "skyrim-se");
        assert_eq!(report.parse_errors.len(), 1);
    }

    #[test]
    fn validates_light_plugin_compact_ids_conservatively() {
        let tmp = tempfile::tempdir().unwrap();
        let target = record(b"MISC", 0, 0x0000_0800, Vec::new());
        let reference = record(b"REFR", 0, 0x0000_0801, name_ref(0x0000_0800));
        std::fs::write(
            tmp.path().join("Light.esl"),
            plugin(plugin_header::flags::ESL, &[], vec![target, reference]),
        )
        .unwrap();

        let report = validate_record_references(tmp.path(), &["Light.esl"], "skyrim-se");
        assert!(report.is_empty(), "{report:?}");
    }

    #[test]
    fn validates_esl_flagged_esp_compact_ids_conservatively() {
        let tmp = tempfile::tempdir().unwrap();
        let target = record(b"MISC", 0, 0x0000_0800, Vec::new());
        let reference = record(b"REFR", 0, 0x0000_0801, name_ref(0x0000_0800));
        std::fs::write(
            tmp.path().join("LightFlagged.esp"),
            plugin(plugin_header::flags::ESL, &[], vec![target, reference]),
        )
        .unwrap();

        let report = validate_record_references(tmp.path(), &["LightFlagged.esp"], "skyrim-se");
        assert!(report.is_empty(), "{report:?}");
    }

    #[test]
    fn container_item_reference_uses_cnto_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let item = record(b"MISC", 0, 0x0000_0800, Vec::new());
        let container = record(b"CONT", 0, 0x0000_0801, cnto_ref(0x0000_0800, 3));
        std::fs::write(
            tmp.path().join("Container.esm"),
            plugin(plugin_header::flags::ESM, &[], vec![item, container]),
        )
        .unwrap();

        let report = validate_record_references(tmp.path(), &["Container.esm"], "skyrim-se");
        assert!(report.is_empty(), "{report:?}");
    }

    #[test]
    fn weapon_reference_uses_cnam_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let enchantment = record(b"ENCH", 0, 0x0000_0800, Vec::new());
        let weapon = record(b"WEAP", 0, 0x0000_0801, cnam_ref(0x0000_0800));
        std::fs::write(
            tmp.path().join("Weapon.esm"),
            plugin(plugin_header::flags::ESM, &[], vec![enchantment, weapon]),
        )
        .unwrap();

        let report = validate_record_references(tmp.path(), &["Weapon.esm"], "skyrim-se");
        assert!(report.is_empty(), "{report:?}");
    }

    #[test]
    fn out_of_range_mod_index_has_specific_diagnostic() {
        let tmp = tempfile::tempdir().unwrap();
        let reference = record(b"REFR", 0, 0x0000_0801, name_ref(0x0200_0800));
        std::fs::write(
            tmp.path().join("Broken.esm"),
            plugin(plugin_header::flags::ESM, &[], vec![reference]),
        )
        .unwrap();

        let report = validate_record_references(tmp.path(), &["Broken.esm"], "skyrim-se");
        assert_eq!(report.unresolved.len(), 1);
        assert!(report.unresolved[0].mod_index_out_of_range);
        assert!(
            report.unresolved[0]
                .to_string()
                .contains("mod index beyond the master list")
        );
    }

    #[test]
    fn unsupported_record_types_are_reported_without_failing_clean_validation() {
        let tmp = tempfile::tempdir().unwrap();
        let armor = record(b"ARMO", 0, 0x0000_0800, Vec::new());
        std::fs::write(
            tmp.path().join("Armor.esm"),
            plugin(plugin_header::flags::ESM, &[], vec![armor]),
        )
        .unwrap();

        let report = validate_record_references(tmp.path(), &["Armor.esm"], "skyrim-se");
        assert!(report.is_empty(), "{report:?}");
        assert_eq!(report.unsupported_record_types, ["ARMO"]);
    }
}
