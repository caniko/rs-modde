//! Bethesda plugin (ESP/ESM/ESL) header parser.
//!
//! Reads only the first ~1KB of a plugin file to extract:
//! - Plugin version (Form 43 vs Form 44)
//! - Master file dependencies
//! - Plugin flags (ESM, ESL)
//!
//! This avoids loading entire multi-GB plugin files.

use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result, bail};

/// Plugin record type identifier.
const TES4_SIGNATURE: &[u8; 4] = b"TES4";

/// The MAST sub-record signature (master dependency).
const MAST_SIGNATURE: &[u8; 4] = b"MAST";

/// Plugin version for Skyrim LE (Form 43) — outdated for SSE.
pub const FORM_43_VERSION: f32 = 0.94;

/// Plugin version for Skyrim SE (Form 44).
pub const FORM_44_VERSION: f32 = 1.70;

/// Flags in the TES4 record header.
pub mod flags {
    /// Plugin is flagged as a master file (ESM).
    pub const ESM: u32 = 0x0000_0001;
    /// Plugin is flagged as light (ESL).
    pub const ESL: u32 = 0x0000_0200;
}

/// Parsed header from a Bethesda plugin file.
#[derive(Debug, Clone)]
pub struct PluginHeader {
    /// The plugin filename (just the name, not the full path).
    pub filename: String,
    /// Record flags from the TES4 header.
    pub record_flags: u32,
    /// Plugin version (e.g., 0.94 for Form 43, 1.70 for Form 44).
    pub version: f32,
    /// Number of records in the plugin.
    pub num_records: u32,
    /// Master file dependencies.
    pub masters: Vec<String>,
}

impl PluginHeader {
    /// Whether this plugin uses the outdated Form 43 format (Skyrim LE).
    #[must_use]
    pub fn is_form_43(&self) -> bool {
        self.version < FORM_44_VERSION - 0.01
    }

    /// Whether this plugin is flagged as a master (ESM).
    #[must_use]
    pub fn is_esm(&self) -> bool {
        self.record_flags & flags::ESM != 0 || self.filename.to_lowercase().ends_with(".esm")
    }

    /// Whether this plugin is flagged as a light plugin (ESL).
    #[must_use]
    pub fn is_esl(&self) -> bool {
        self.record_flags & flags::ESL != 0 || self.filename.to_lowercase().ends_with(".esl")
    }
}

/// A validation warning for a plugin.
#[derive(Debug, Clone)]
pub enum PluginWarning {
    /// Plugin uses Form 43 format (Skyrim LE) in a Form 44 game (SSE).
    Form43 { plugin: String, version: f32 },
    /// Plugin depends on a master that is not in the active load order.
    MissingMaster { plugin: String, master: String },
}

impl std::fmt::Display for PluginWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginWarning::Form43 { plugin, version } => {
                write!(
                    f,
                    "{plugin}: uses Form 43 (v{version:.2}) — this is the Oldrim format, \
                     which can cause CTDs in SSE. Resave in Creation Kit."
                )
            }
            PluginWarning::MissingMaster { plugin, master } => {
                write!(
                    f,
                    "{plugin}: missing master '{master}' — the game will crash on load."
                )
            }
        }
    }
}

/// Parse a plugin header from a file path.
///
/// Only reads the first ~8KB to extract the TES4 record and MAST sub-records.
pub fn parse_plugin_header(path: &Path) -> Result<PluginHeader> {
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open plugin: {}", path.display()))?;

    // Read TES4 record header (24 bytes)
    let mut sig = [0u8; 4];
    file.read_exact(&mut sig)?;
    if &sig != TES4_SIGNATURE {
        bail!("not a valid Bethesda plugin: expected TES4, got {sig:?}");
    }

    let data_size = read_u32_le(&mut file)?;
    let record_flags = read_u32_le(&mut file)?;
    let _form_id = read_u32_le(&mut file)?;
    let _revision = read_u32_le(&mut file)?;
    let version = read_u16_le(&mut file)?;
    let _unknown = read_u16_le(&mut file)?;

    // Read the HEDR sub-record (inside TES4 data, first sub-record)
    let mut plugin_version: f32 = 0.0;
    let mut num_records: u32 = 0;
    let mut masters: Vec<String> = Vec::new();

    // Read TES4 sub-records up to data_size bytes
    let data_start = file.stream_position()?;
    let data_end = data_start + u64::from(data_size);

    while file.stream_position()? < data_end {
        let mut sub_sig = [0u8; 4];
        if file.read_exact(&mut sub_sig).is_err() {
            break;
        }
        let sub_size = u64::from(read_u16_le(&mut file)?);
        let sub_start = file.stream_position()?;

        match &sub_sig {
            b"HEDR" => {
                // HEDR: version (f32), numRecords (u32), nextObjectId (u32)
                plugin_version = read_f32_le(&mut file)?;
                num_records = read_u32_le(&mut file)?;
            }
            sub if sub == MAST_SIGNATURE => {
                // MAST: null-terminated string
                let mut buf = vec![0u8; sub_size as usize];
                file.read_exact(&mut buf)?;
                // Trim trailing null
                if buf.last() == Some(&0) {
                    buf.pop();
                }
                if let Ok(name) = String::from_utf8(buf) {
                    masters.push(name);
                }
            }
            _ => {
                // Skip unknown sub-record
            }
        }

        // Seek to the end of this sub-record
        file.seek(SeekFrom::Start(sub_start + sub_size))?;
    }

    Ok(PluginHeader {
        filename,
        record_flags,
        version: if version >= 1 {
            plugin_version
        } else {
            plugin_version
        },
        num_records,
        masters,
    })
}

/// Validate plugins against the active load order.
///
/// Returns warnings for Form 43 plugins and missing masters.
/// `active_plugins` should be all plugin filenames in the load order (case-insensitive matching).
/// `plugin_dir` is the game's Data directory containing the plugins.
pub fn validate_plugins(
    plugin_dir: &Path,
    active_plugins: &[&str],
    check_form_43: bool,
) -> Vec<PluginWarning> {
    let active_lower: std::collections::HashSet<String> =
        active_plugins.iter().map(|p| p.to_lowercase()).collect();

    let mut warnings = Vec::new();

    for plugin_name in active_plugins {
        let path = plugin_dir.join(plugin_name);
        if !path.exists() {
            continue;
        }

        let header = match parse_plugin_header(&path) {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(plugin = *plugin_name, error = %e, "failed to parse plugin header");
                continue;
            }
        };

        // Check Form 43
        if check_form_43 && header.is_form_43() {
            warnings.push(PluginWarning::Form43 {
                plugin: plugin_name.to_string(),
                version: header.version,
            });
        }

        // Check missing masters
        for master in &header.masters {
            if !active_lower.contains(&master.to_lowercase()) {
                warnings.push(PluginWarning::MissingMaster {
                    plugin: plugin_name.to_string(),
                    master: master.clone(),
                });
            }
        }
    }

    warnings
}

fn read_u32_le(r: &mut impl Read) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u16_le(r: &mut impl Read) -> io::Result<u16> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf)?;
    Ok(u16::from_le_bytes(buf))
}

fn read_f32_le(r: &mut impl Read) -> io::Result<f32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(f32::from_le_bytes(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal TES4 record in memory for testing.
    fn build_test_plugin(version: f32, masters: &[&str], record_flags: u32) -> Vec<u8> {
        let mut data = Vec::new();

        // Build TES4 sub-records
        let mut sub_records = Vec::new();

        // HEDR sub-record: version(f32) + numRecords(u32) + nextObjectId(u32) = 12 bytes
        sub_records.extend_from_slice(b"HEDR");
        sub_records.extend_from_slice(&12u16.to_le_bytes());
        sub_records.extend_from_slice(&version.to_le_bytes());
        sub_records.extend_from_slice(&100u32.to_le_bytes()); // numRecords
        sub_records.extend_from_slice(&0x800u32.to_le_bytes()); // nextObjectId

        // MAST sub-records for each master
        for master in masters {
            let name_bytes = master.as_bytes();
            let sub_size = (name_bytes.len() + 1) as u16; // +1 for null terminator
            sub_records.extend_from_slice(b"MAST");
            sub_records.extend_from_slice(&sub_size.to_le_bytes());
            sub_records.extend_from_slice(name_bytes);
            sub_records.push(0); // null terminator

            // DATA sub-record (8 bytes, file size — required after each MAST)
            sub_records.extend_from_slice(b"DATA");
            sub_records.extend_from_slice(&8u16.to_le_bytes());
            sub_records.extend_from_slice(&0u64.to_le_bytes());
        }

        // TES4 record header
        data.extend_from_slice(b"TES4");
        data.extend_from_slice(&(sub_records.len() as u32).to_le_bytes()); // data size
        data.extend_from_slice(&record_flags.to_le_bytes()); // flags
        data.extend_from_slice(&0u32.to_le_bytes()); // form ID
        data.extend_from_slice(&0u32.to_le_bytes()); // revision
        data.extend_from_slice(&44u16.to_le_bytes()); // version field
        data.extend_from_slice(&0u16.to_le_bytes()); // unknown

        data.extend_from_slice(&sub_records);
        data
    }

    #[test]
    fn test_parse_form_44_plugin() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let plugin_data = build_test_plugin(1.70, &["Skyrim.esm", "Update.esm"], 0);
        std::fs::write(tmp.path(), &plugin_data).unwrap();

        let header = parse_plugin_header(tmp.path()).unwrap();
        assert!(!header.is_form_43());
        assert_eq!(header.masters.len(), 2);
        assert_eq!(header.masters[0], "Skyrim.esm");
        assert_eq!(header.masters[1], "Update.esm");
        assert_eq!(header.num_records, 100);
    }

    #[test]
    fn test_parse_form_43_plugin() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let plugin_data = build_test_plugin(0.94, &["Skyrim.esm"], 0);
        std::fs::write(tmp.path(), &plugin_data).unwrap();

        let header = parse_plugin_header(tmp.path()).unwrap();
        assert!(header.is_form_43());
    }

    #[test]
    fn test_esm_flag() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let plugin_data = build_test_plugin(1.70, &[], flags::ESM);
        std::fs::write(tmp.path(), &plugin_data).unwrap();

        let header = parse_plugin_header(tmp.path()).unwrap();
        assert!(header.is_esm());
        assert!(!header.is_esl());
    }

    #[test]
    fn test_esl_flag() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let plugin_data = build_test_plugin(1.70, &[], flags::ESL);
        std::fs::write(tmp.path(), &plugin_data).unwrap();

        let header = parse_plugin_header(tmp.path()).unwrap();
        assert!(!header.is_esm());
        assert!(header.is_esl());
    }

    #[test]
    fn test_validate_missing_master() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_data = build_test_plugin(1.70, &["Skyrim.esm", "MissingMod.esp"], 0);
        std::fs::write(tmp.path().join("MyMod.esp"), &plugin_data).unwrap();

        let active = vec!["Skyrim.esm", "MyMod.esp"];
        let warnings = validate_plugins(tmp.path(), &active, true);

        assert_eq!(warnings.len(), 1);
        assert!(
            matches!(&warnings[0], PluginWarning::MissingMaster { master, .. } if master == "MissingMod.esp")
        );
    }

    #[test]
    fn test_validate_form_43_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_data = build_test_plugin(0.94, &["Skyrim.esm"], 0);
        std::fs::write(tmp.path().join("OldMod.esp"), &plugin_data).unwrap();

        // Also create a dummy Skyrim.esm so it doesn't show missing master
        let esm_data = build_test_plugin(1.70, &[], flags::ESM);
        std::fs::write(tmp.path().join("Skyrim.esm"), &esm_data).unwrap();

        let active = vec!["Skyrim.esm", "OldMod.esp"];
        let warnings = validate_plugins(tmp.path(), &active, true);

        assert!(
            warnings.iter().any(
                |w| matches!(w, PluginWarning::Form43 { plugin, .. } if plugin == "OldMod.esp")
            )
        );
    }

    #[test]
    fn test_invalid_file_header() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"NOT_A_PLUGIN_FILE").unwrap();

        let result = parse_plugin_header(tmp.path());
        assert!(result.is_err());
    }
}
