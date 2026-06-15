//! Native Bethesda ESP/ESM/ESL record parsing and conservative `FormID`
#![allow(clippy::wildcard_imports)]
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

mod display;
mod parser;

#[cfg(test)]
mod tests;

use parser::*;

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
