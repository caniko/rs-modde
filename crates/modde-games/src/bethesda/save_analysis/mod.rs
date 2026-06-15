//! Read-only Bethesda save dependency analysis for pre-removal gates.
#![allow(clippy::wildcard_imports)]
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

mod scan;
mod source;

#[cfg(test)]
mod tests;

use scan::*;
use source::*;

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
