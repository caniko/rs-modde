use super::*;

pub(super) fn unsupported_record_types(docs: &[PluginDocument]) -> Vec<String> {
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

pub(super) fn may_contain_formid_references(signature: &str) -> bool {
    matches!(signature, "ARMO" | "LVLI" | "FLST")
}

pub(super) fn parse_plugin_document(path: &Path) -> Result<PluginDocument> {
    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    parse_plugin_bytes(&filename, &bytes)
}

pub(super) fn parse_plugin_bytes(filename: &str, bytes: &[u8]) -> Result<PluginDocument> {
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

pub(super) fn parse_top_level_item(
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

pub(super) fn parse_group(
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

pub(super) fn parse_record(cursor: &mut Cursor<&[u8]>) -> Result<ParsedRecord> {
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

pub(super) fn decompress_record(data: &[u8]) -> Result<Vec<u8>> {
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

pub(super) fn parse_masters(data: &[u8]) -> Result<Vec<String>> {
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

pub(super) fn extract_references(record: &[u8; 4], data: &[u8]) -> Result<Vec<RecordReference>> {
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

pub(super) fn collect_schema_references(
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

pub(super) fn read_form_id_at(body: &[u8], offset: usize) -> Option<u32> {
    body.get(offset..offset + 4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("slice length checked")))
}

pub(super) fn definition_key(doc: &PluginDocument, form_id: u32) -> Option<ResolvedFormKey> {
    if form_id == 0 {
        return None;
    }
    Some(ResolvedFormKey {
        plugin: doc.filename.to_lowercase(),
        object_id: object_id_for_plugin(doc.is_light, form_id),
        compact: doc.is_light,
    })
}

pub(super) fn resolve_reference(
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

pub(super) fn object_id_for_plugin(light: bool, form_id: u32) -> u32 {
    if light {
        form_id & 0x0000_0fff
    } else {
        form_id & 0x00ff_ffff
    }
}

pub(super) fn is_bethesda_plugin(plugin: &str) -> bool {
    Path::new(plugin)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "esp" | "esm" | "esl"))
}

pub(super) fn nul_terminated_string(bytes: &[u8]) -> Result<String> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end])
        .map(str::to_owned)
        .context("invalid UTF-8 string in plugin header")
}

pub(super) fn peek_signature(cursor: &mut Cursor<&[u8]>) -> Result<[u8; 4]> {
    let pos = cursor.position();
    let signature = read_signature(cursor)?;
    cursor.set_position(pos);
    Ok(signature)
}

pub(super) fn expect_signature(cursor: &mut Cursor<&[u8]>, expected: &[u8; 4]) -> Result<()> {
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

pub(super) fn read_signature(cursor: &mut Cursor<&[u8]>) -> Result<[u8; 4]> {
    let mut buf = [0u8; 4];
    cursor.read_exact(&mut buf)?;
    Ok(buf)
}

pub(super) fn read_u32(cursor: &mut Cursor<&[u8]>) -> Result<u32> {
    let mut buf = [0u8; 4];
    cursor.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

pub(super) fn read_u16(cursor: &mut Cursor<&[u8]>) -> Result<u16> {
    let mut buf = [0u8; 2];
    cursor.read_exact(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

pub(super) fn read_exact_vec(cursor: &mut Cursor<&[u8]>, len: usize) -> Result<Vec<u8>> {
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

pub(super) fn skip(cursor: &mut Cursor<&[u8]>, len: u64) -> Result<()> {
    let next = cursor.position() + len;
    if next > cursor.get_ref().len() as u64 {
        bail!("truncated plugin data while skipping {len} bytes");
    }
    cursor.set_position(next);
    Ok(())
}

pub(super) fn remaining(cursor: &Cursor<&[u8]>) -> u64 {
    cursor
        .get_ref()
        .len()
        .saturating_sub(cursor.position() as usize) as u64
}

pub(super) fn signature_to_string(signature: &[u8; 4]) -> String {
    String::from_utf8_lossy(signature).to_string()
}
