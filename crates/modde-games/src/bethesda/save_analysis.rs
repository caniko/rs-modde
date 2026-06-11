//! Read-only Bethesda save dependency analysis for pre-removal gates.
//!
//! This intentionally does not rewrite or clean saves. It validates the save
//! header, extracts plugin/script symbols from the binary payload, and matches
//! them against the mod being removed.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::archive_index::ArchiveIndex;
use crate::traits::{
    ModSafety, SaveDependencyAnalyzer, SaveDependencyFinding, SaveDependencyKind,
    SaveRemovalGateReport,
};

const MAGIC_SKYRIM_SE: &[u8] = b"TESV_SAVEGAME";
const MAGIC_FALLOUT4: &[u8] = b"FO4_SAVEGAME";
const MAX_DECOMPRESSED_SAVE_SCAN_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug)]
pub struct BethesdaSaveDependencyAnalyzer {
    expected_magic: &'static [u8],
    save_extension: &'static str,
}

pub static SKYRIM_SAVE_ANALYZER: BethesdaSaveDependencyAnalyzer = BethesdaSaveDependencyAnalyzer {
    expected_magic: MAGIC_SKYRIM_SE,
    save_extension: "ess",
};

pub static FALLOUT4_SAVE_ANALYZER: BethesdaSaveDependencyAnalyzer =
    BethesdaSaveDependencyAnalyzer {
        expected_magic: MAGIC_FALLOUT4,
        save_extension: "fos",
    };

#[derive(Debug, Default)]
struct ModDependencySource {
    plugins: BTreeSet<String>,
    scripts: BTreeSet<String>,
    warnings: Vec<String>,
    parse_failures: Vec<String>,
    saw_save_relevant_file: bool,
    saw_file: bool,
}

#[derive(Debug)]
struct ParsedSaveSymbols {
    plugins: BTreeSet<String>,
    scripts: BTreeSet<String>,
    active_scripts: BTreeSet<String>,
    unattached_instances: BTreeSet<String>,
    undefined_elements: BTreeSet<String>,
}

impl SaveDependencyAnalyzer for BethesdaSaveDependencyAnalyzer {
    fn analyze_mod_removal(
        &self,
        mod_id: &str,
        mod_dir: &Path,
        save_roots: &[PathBuf],
    ) -> Result<SaveRemovalGateReport> {
        let source = collect_mod_dependency_source(mod_dir)?;
        let safety = if source.parse_failures.is_empty() {
            if source.saw_save_relevant_file {
                ModSafety::SaveBreaking
            } else if source.saw_file {
                ModSafety::SaveSafe
            } else {
                ModSafety::Unknown
            }
        } else {
            ModSafety::Unknown
        };

        let mut report = SaveRemovalGateReport {
            mod_id: mod_id.to_string(),
            safety,
            analyzed_saves: 0,
            blocking_findings: Vec::new(),
            warnings: source.warnings.clone(),
        };

        for failure in &source.parse_failures {
            report.blocking_findings.push(SaveDependencyFinding {
                save_path: mod_dir.to_path_buf(),
                profile: None,
                dependency_kind: SaveDependencyKind::ParseIncomplete,
                symbol: failure.clone(),
                source_file: None,
                confidence: 1.0,
            });
        }

        if source.plugins.is_empty() && source.scripts.is_empty() {
            return Ok(report);
        }

        for save_path in save_files(save_roots, self.save_extension) {
            report.analyzed_saves += 1;
            match self.parse_save_symbols(&save_path) {
                Ok(symbols) => {
                    append_symbol_findings(&mut report, &save_path, &source, &symbols);
                }
                Err(err) => {
                    report.blocking_findings.push(SaveDependencyFinding {
                        save_path,
                        profile: None,
                        dependency_kind: SaveDependencyKind::ParseIncomplete,
                        symbol: err.to_string(),
                        source_file: None,
                        confidence: 1.0,
                    });
                }
            }
        }

        Ok(report)
    }
}

impl BethesdaSaveDependencyAnalyzer {
    fn parse_save_symbols(&self, save_path: &Path) -> Result<ParsedSaveSymbols> {
        let bytes = std::fs::read(save_path)
            .with_context(|| format!("failed to read save file {}", save_path.display()))?;
        validate_save_header(&bytes, self.expected_magic)?;
        let scan_bytes = save_scan_bytes(&bytes);
        Ok(ParsedSaveSymbols {
            plugins: extract_names_by_extension(&scan_bytes, &["esp", "esm", "esl"]),
            scripts: extract_script_names(&scan_bytes),
            active_scripts: extract_tagged_script_names(
                &scan_bytes,
                &["activescript", "active script"],
            ),
            unattached_instances: extract_tagged_script_names(
                &scan_bytes,
                &["unattached", "unattachedinstance", "unattached instance"],
            ),
            undefined_elements: extract_tagged_script_names(
                &scan_bytes,
                &["undefined", "undefinedelement", "undefined element"],
            ),
        })
    }
}

fn collect_mod_dependency_source(mod_dir: &Path) -> Result<ModDependencySource> {
    let mut source = ModDependencySource::default();
    if !mod_dir.exists() {
        source.parse_failures.push(format!(
            "mod staging directory is missing: {}",
            mod_dir.display()
        ));
        return Ok(source);
    }

    let mut stack = vec![mod_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .with_context(|| format!("failed to read mod directory {}", dir.display()))?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }

            source.saw_file = true;
            let rel = path.strip_prefix(mod_dir).unwrap_or(&path);
            let rel_normalized = normalize_path(rel);
            let ext = extension(&path);

            match ext.as_deref() {
                Some("esp" | "esm" | "esl") => {
                    source.saw_save_relevant_file = true;
                    if let Some(name) = path.file_name().and_then(OsStr::to_str) {
                        source.plugins.insert(name.to_ascii_lowercase());
                    }
                }
                Some("pex" | "psc") => {
                    source.saw_save_relevant_file = true;
                    if let Some(stem) = path.file_stem().and_then(OsStr::to_str) {
                        source.scripts.insert(stem.to_ascii_lowercase());
                    }
                }
                Some("dll") => {
                    source.saw_save_relevant_file = true;
                    source.warnings.push(format!(
                        "{} is a script extender/native plugin; save records may not name it directly",
                        rel_normalized
                    ));
                }
                Some("bsa" | "ba2") => {
                    collect_archive_dependency_source(&path, &rel_normalized, &mut source);
                }
                _ => {}
            }
        }
    }

    Ok(source)
}

fn collect_archive_dependency_source(
    path: &Path,
    rel_path: &str,
    source: &mut ModDependencySource,
) {
    match ArchiveIndex::read(path) {
        Ok(index) => {
            for file in index.files {
                let file_path = file.path.to_ascii_lowercase();
                let archive_symbol = Path::new(&file_path);
                match extension(archive_symbol).as_deref() {
                    Some("esp" | "esm" | "esl") => {
                        source.saw_save_relevant_file = true;
                        if let Some(name) = archive_symbol.file_name().and_then(OsStr::to_str) {
                            source.plugins.insert(name.to_ascii_lowercase());
                        }
                    }
                    Some("pex" | "psc") => {
                        source.saw_save_relevant_file = true;
                        if let Some(stem) = archive_symbol.file_stem().and_then(OsStr::to_str) {
                            source.scripts.insert(stem.to_ascii_lowercase());
                        }
                    }
                    _ => {}
                }
            }
        }
        Err(err) => {
            source.parse_failures.push(format!(
                "failed to index Bethesda archive {rel_path}: {err}. Regenerate the mod staging entry by reinstalling the archive, then validate with `modde mod remove --dry-run <mod_id>`."
            ));
        }
    }
}

fn append_symbol_findings(
    report: &mut SaveRemovalGateReport,
    save_path: &Path,
    source: &ModDependencySource,
    symbols: &ParsedSaveSymbols,
) {
    for plugin in source.plugins.intersection(&symbols.plugins) {
        report.blocking_findings.push(SaveDependencyFinding {
            save_path: save_path.to_path_buf(),
            profile: None,
            dependency_kind: SaveDependencyKind::PluginRecord,
            symbol: plugin.clone(),
            source_file: Some(plugin.clone()),
            confidence: 1.0,
        });
    }

    append_script_set(
        report,
        save_path,
        &source.scripts,
        &symbols.scripts,
        SaveDependencyKind::PapyrusScript,
        0.9,
    );
    append_script_set(
        report,
        save_path,
        &source.scripts,
        &symbols.active_scripts,
        SaveDependencyKind::ActiveScript,
        1.0,
    );
    append_script_set(
        report,
        save_path,
        &source.scripts,
        &symbols.unattached_instances,
        SaveDependencyKind::UnattachedInstance,
        1.0,
    );
    append_script_set(
        report,
        save_path,
        &source.scripts,
        &symbols.undefined_elements,
        SaveDependencyKind::UndefinedElement,
        1.0,
    );
}

fn append_script_set(
    report: &mut SaveRemovalGateReport,
    save_path: &Path,
    mod_scripts: &BTreeSet<String>,
    save_scripts: &BTreeSet<String>,
    dependency_kind: SaveDependencyKind,
    confidence: f32,
) {
    for script in mod_scripts.intersection(save_scripts) {
        report.blocking_findings.push(SaveDependencyFinding {
            save_path: save_path.to_path_buf(),
            profile: None,
            dependency_kind,
            symbol: script.clone(),
            source_file: Some(format!("scripts/{script}.pex")),
            confidence,
        });
    }
}

fn save_files(roots: &[PathBuf], extension: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        collect_save_files(root, extension, &mut out);
    }
    out.sort();
    out.dedup();
    out
}

fn collect_save_files(path: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.is_file() {
        if path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
        {
            out.push(path.to_path_buf());
        }
        return;
    }
    if !meta.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let child = entry.path();
        if child.file_name().and_then(OsStr::to_str) == Some(".git") {
            continue;
        }
        collect_save_files(&child, extension, out);
    }
}

fn validate_save_header(bytes: &[u8], expected_magic: &[u8]) -> Result<()> {
    anyhow::ensure!(
        bytes.len() >= expected_magic.len() + 14,
        "save is truncated before the Bethesda header is complete"
    );
    anyhow::ensure!(
        bytes.starts_with(expected_magic),
        "save magic mismatch; expected {}",
        String::from_utf8_lossy(expected_magic)
    );
    let header_size_offset = expected_magic.len();
    let header_size = u32::from_le_bytes(
        bytes[header_size_offset..header_size_offset + 4]
            .try_into()
            .expect("slice length checked above"),
    );
    anyhow::ensure!(header_size > 0, "save header size is zero");
    Ok(())
}

fn save_scan_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut scan = bytes.to_vec();
    for payload in decompress_embedded_payloads(bytes) {
        scan.push(b' ');
        scan.extend_from_slice(&payload);
    }
    scan
}

fn decompress_embedded_payloads(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut payloads = Vec::new();
    for offset in 0..bytes.len().saturating_sub(2) {
        let tail = &bytes[offset..];
        if looks_like_zlib(tail)
            && let Ok(payload) = decompress_zlib(tail)
        {
            payloads.push(payload);
        }
        if tail.starts_with(&[0x04, 0x22, 0x4d, 0x18])
            && let Ok(payload) = decompress_lz4_frame(tail)
        {
            payloads.push(payload);
        }
    }
    payloads
}

fn looks_like_zlib(bytes: &[u8]) -> bool {
    bytes.len() >= 2
        && bytes[0] == 0x78
        && matches!(bytes[1], 0x01 | 0x5e | 0x9c | 0xda)
        && u16::from_be_bytes([bytes[0], bytes[1]]) % 31 == 0
}

fn decompress_zlib(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = flate2::read::ZlibDecoder::new(bytes);
    let mut payload = Vec::new();
    decoder
        .by_ref()
        .take(MAX_DECOMPRESSED_SAVE_SCAN_BYTES)
        .read_to_end(&mut payload)?;
    anyhow::ensure!(!payload.is_empty(), "zlib payload is empty");
    Ok(payload)
}

fn decompress_lz4_frame(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = lz4_flex::frame::FrameDecoder::new(bytes);
    let mut payload = Vec::new();
    decoder
        .by_ref()
        .take(MAX_DECOMPRESSED_SAVE_SCAN_BYTES)
        .read_to_end(&mut payload)?;
    anyhow::ensure!(!payload.is_empty(), "lz4 payload is empty");
    Ok(payload)
}

fn extract_names_by_extension(bytes: &[u8], extensions: &[&str]) -> BTreeSet<String> {
    let ascii = lossy_ascii_lower(bytes);
    let mut out = BTreeSet::new();
    for token in ascii.split(|c: char| !is_symbol_char(c)) {
        let token = token.trim_matches('.');
        if token.is_empty() {
            continue;
        }
        if extensions
            .iter()
            .any(|ext| token.ends_with(&format!(".{ext}")))
        {
            if let Some(name) = token.rsplit(['/', '\\', '\0']).next() {
                out.insert(name.to_string());
            }
        }
    }
    out
}

fn extract_script_names(bytes: &[u8]) -> BTreeSet<String> {
    let ascii = lossy_ascii_lower(bytes);
    let mut out = BTreeSet::new();
    for token in ascii.split(|c: char| !is_symbol_char(c)) {
        let token = token.trim_matches('.');
        if token.ends_with(".pex") || token.ends_with(".psc") {
            if let Some(stem) = Path::new(token)
                .file_stem()
                .and_then(OsStr::to_str)
                .filter(|s| !s.is_empty())
            {
                out.insert(stem.to_string());
            }
        } else if looks_like_papyrus_script_name(token) {
            out.insert(token.to_string());
        }
    }
    out
}

fn extract_tagged_script_names(bytes: &[u8], tags: &[&str]) -> BTreeSet<String> {
    let ascii = lossy_ascii_lower(bytes);
    let mut out = BTreeSet::new();
    for tag in tags {
        let mut search = ascii.as_str();
        while let Some(idx) = search.find(tag) {
            let after = &search[idx + tag.len()..];
            for token in after
                .split(|c: char| !is_symbol_char(c))
                .filter(|s| !s.is_empty())
                .take(8)
            {
                let token = token.trim_matches('.');
                if token.ends_with(".pex") || looks_like_papyrus_script_name(token) {
                    let stem = token.strip_suffix(".pex").unwrap_or(token);
                    out.insert(stem.to_string());
                    break;
                }
            }
            search = &after[after.len().min(1)..];
        }
    }
    out
}

fn looks_like_papyrus_script_name(token: &str) -> bool {
    token.len() >= 3
        && token.len() <= 128
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        && !token.chars().all(|c| c.is_ascii_digit())
        && (token.contains("script")
            || token.starts_with("qf_")
            || token.starts_with("pf_")
            || token.starts_with("tif_"))
}

fn lossy_ascii_lower(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| {
            if byte.is_ascii_graphic() || *byte == b' ' {
                (*byte as char).to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect()
}

fn is_symbol_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '\\')
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save_bytes(magic: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(magic);
        bytes.extend_from_slice(&32u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(b"Test");
        bytes.push(0);
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn skyrim_save_symbols_include_plugins_and_scripts() {
        let tmp = tempfile::tempdir().unwrap();
        let save = tmp.path().join("Save1.ess");
        std::fs::write(
            &save,
            save_bytes(
                MAGIC_SKYRIM_SE,
                b"SomeMod.esp scripts/MyQuestScript.pex ActiveScript MyQuestScript",
            ),
        )
        .unwrap();

        let parsed = SKYRIM_SAVE_ANALYZER.parse_save_symbols(&save).unwrap();
        assert!(parsed.plugins.contains("somemod.esp"));
        assert!(parsed.scripts.contains("myquestscript"));
        assert!(parsed.active_scripts.contains("myquestscript"));
    }

    #[test]
    fn skyrim_save_symbols_include_zlib_payload() {
        let tmp = tempfile::tempdir().unwrap();
        let save = tmp.path().join("Save1.ess");
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(
            &mut encoder,
            b"CompressedMod.esp scripts/CompressedScript.pex ActiveScript CompressedScript",
        )
        .unwrap();
        let compressed = encoder.finish().unwrap();
        std::fs::write(&save, save_bytes(MAGIC_SKYRIM_SE, &compressed)).unwrap();

        let parsed = SKYRIM_SAVE_ANALYZER.parse_save_symbols(&save).unwrap();
        assert!(parsed.plugins.contains("compressedmod.esp"));
        assert!(parsed.scripts.contains("compressedscript"));
        assert!(parsed.active_scripts.contains("compressedscript"));
    }

    #[test]
    fn invalid_save_header_fails_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let save = tmp.path().join("Save1.ess");
        std::fs::write(&save, b"not a save").unwrap();
        assert!(SKYRIM_SAVE_ANALYZER.parse_save_symbols(&save).is_err());
    }

    #[test]
    fn analyzer_blocks_loose_plugin_dependency() {
        let tmp = tempfile::tempdir().unwrap();
        let mod_dir = tmp.path().join("mod");
        let saves = tmp.path().join("saves");
        std::fs::create_dir_all(&mod_dir).unwrap();
        std::fs::create_dir_all(&saves).unwrap();
        std::fs::write(mod_dir.join("SomeMod.esp"), b"TES4").unwrap();
        std::fs::write(
            saves.join("Save1.ess"),
            save_bytes(MAGIC_SKYRIM_SE, b"plugins SomeMod.esp"),
        )
        .unwrap();

        let report = SKYRIM_SAVE_ANALYZER
            .analyze_mod_removal("mod", &mod_dir, &[saves])
            .unwrap();
        assert_eq!(report.safety, ModSafety::SaveBreaking);
        assert!(report.is_blocked());
        assert_eq!(
            report.blocking_findings[0].dependency_kind,
            SaveDependencyKind::PluginRecord
        );
    }

    #[test]
    fn analyzer_allows_asset_only_mod() {
        let tmp = tempfile::tempdir().unwrap();
        let mod_dir = tmp.path().join("mod");
        let saves = tmp.path().join("saves");
        std::fs::create_dir_all(mod_dir.join("textures")).unwrap();
        std::fs::create_dir_all(&saves).unwrap();
        std::fs::write(mod_dir.join("textures/sky.dds"), b"DDS").unwrap();
        std::fs::write(
            saves.join("Save1.ess"),
            save_bytes(MAGIC_SKYRIM_SE, b"plugins SomeMod.esp"),
        )
        .unwrap();

        let report = SKYRIM_SAVE_ANALYZER
            .analyze_mod_removal("mod", &mod_dir, &[saves])
            .unwrap();
        assert_eq!(report.safety, ModSafety::SaveSafe);
        assert!(!report.is_blocked());
    }

    #[test]
    fn analyzer_blocks_loose_pex_dependency() {
        let tmp = tempfile::tempdir().unwrap();
        let mod_dir = tmp.path().join("mod");
        let saves = tmp.path().join("saves");
        std::fs::create_dir_all(mod_dir.join("scripts")).unwrap();
        std::fs::create_dir_all(&saves).unwrap();
        std::fs::write(mod_dir.join("scripts/MyQuestScript.pex"), b"pex").unwrap();
        std::fs::write(
            saves.join("Save1.ess"),
            save_bytes(MAGIC_SKYRIM_SE, b"ActiveScript MyQuestScript"),
        )
        .unwrap();

        let report = SKYRIM_SAVE_ANALYZER
            .analyze_mod_removal("mod", &mod_dir, &[saves])
            .unwrap();
        assert_eq!(report.safety, ModSafety::SaveBreaking);
        assert!(report.is_blocked());
        assert!(
            report
                .blocking_findings
                .iter()
                .any(|f| f.dependency_kind == SaveDependencyKind::ActiveScript)
        );
    }

    #[test]
    fn analyzer_fails_closed_on_invalid_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let mod_dir = tmp.path().join("mod");
        let saves = tmp.path().join("saves");
        std::fs::create_dir_all(&mod_dir).unwrap();
        std::fs::create_dir_all(&saves).unwrap();
        std::fs::write(mod_dir.join("PackedScripts.bsa"), b"not a valid bsa").unwrap();

        let report = SKYRIM_SAVE_ANALYZER
            .analyze_mod_removal("mod", &mod_dir, &[saves])
            .unwrap();
        assert_eq!(report.safety, ModSafety::Unknown);
        assert!(report.is_blocked());
        assert!(
            report
                .blocking_findings
                .iter()
                .any(|f| f.dependency_kind == SaveDependencyKind::ParseIncomplete)
        );
    }
}
