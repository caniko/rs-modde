use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::PluginEntry;
use crate::installer::StagedFile;
use crate::profile::{EnabledMod, Profile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CrashLogFormat {
    Auto,
    CrashLoggerSse,
    NetScriptFramework,
    Trainwreck,
    Generic,
}

impl CrashLogFormat {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::CrashLoggerSse => "crash-logger-sse",
            Self::NetScriptFramework => "net-script-framework",
            Self::Trainwreck => "trainwreck",
            Self::Generic => "generic",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "crash-logger-sse" | "crashlogger" | "crashloggersse" => Some(Self::CrashLoggerSse),
            "net-script-framework" | "netscriptframework" | ".net-script-framework" => {
                Some(Self::NetScriptFramework)
            }
            "trainwreck" => Some(Self::Trainwreck),
            "generic" => Some(Self::Generic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashToken {
    pub value: String,
    pub kind: CrashTokenKind,
    pub section: String,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CrashTokenKind {
    Plugin,
    Dll,
    AssetPath,
    FormId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashSignature {
    pub detected_format: CrashLogFormat,
    pub exception_line: Option<String>,
    pub tokens: Vec<CrashToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashCorrelationReport {
    pub game_id: String,
    pub profile_name: String,
    pub source_path: PathBuf,
    pub raw_sha256: String,
    pub format: CrashLogFormat,
    pub signature: CrashSignature,
    pub suspects: Vec<CrashSuspect>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashSuspect {
    pub mod_id: Option<String>,
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub nexus_mod_id: Option<i64>,
    pub nexus_file_id: Option<i64>,
    pub nexus_game_domain: Option<String>,
    pub installed_timestamp: Option<i64>,
    pub plugin_name: Option<String>,
    pub plugin_load_index: Option<i64>,
    pub evidence: Vec<CrashEvidence>,
    pub confidence: CrashConfidence,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashEvidence {
    pub token: String,
    pub kind: CrashTokenKind,
    pub section: String,
    pub reason: String,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CrashConfidence {
    Low,
    Medium,
    High,
    Highest,
}

pub trait CrashLogParser {
    fn format(&self) -> CrashLogFormat;
    fn parse(&self, text: &str) -> CrashSignature;
}

pub struct CrashLoggerSseParser;
pub struct NetScriptFrameworkParser;
pub struct TrainwreckParser;
pub struct GenericCrashParser;

impl CrashLogParser for CrashLoggerSseParser {
    fn format(&self) -> CrashLogFormat {
        CrashLogFormat::CrashLoggerSse
    }

    fn parse(&self, text: &str) -> CrashSignature {
        parse_with_sections(text, self.format(), crash_logger_section)
    }
}

impl CrashLogParser for TrainwreckParser {
    fn format(&self) -> CrashLogFormat {
        CrashLogFormat::Trainwreck
    }

    fn parse(&self, text: &str) -> CrashSignature {
        parse_with_sections(text, self.format(), trainwreck_section)
    }
}

impl CrashLogParser for NetScriptFrameworkParser {
    fn format(&self) -> CrashLogFormat {
        CrashLogFormat::NetScriptFramework
    }

    fn parse(&self, text: &str) -> CrashSignature {
        parse_with_sections(text, self.format(), net_script_framework_section)
    }
}

impl CrashLogParser for GenericCrashParser {
    fn format(&self) -> CrashLogFormat {
        CrashLogFormat::Generic
    }

    fn parse(&self, text: &str) -> CrashSignature {
        parse_with_sections(text, self.format(), |_| None)
    }
}

#[derive(Debug, Clone)]
pub struct CrashCorrelationInput {
    pub game_id: String,
    pub profile: Profile,
    pub active_plugins: Vec<PluginEntry>,
    pub installed_files: Vec<(String, StagedFile)>,
    pub tool_files: Vec<String>,
}

#[must_use]
pub fn detect_format(text: &str) -> CrashLogFormat {
    let mut lines = text.lines();
    let head = (0..8)
        .filter_map(|_| lines.next())
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    if head.contains(".net script framework") || head.contains("net script framework") {
        CrashLogFormat::NetScriptFramework
    } else if head.contains("crashloggersse") {
        CrashLogFormat::CrashLoggerSse
    } else if head.contains("trainwreck") {
        CrashLogFormat::Trainwreck
    } else {
        CrashLogFormat::Generic
    }
}

#[must_use]
pub fn parse_crash_log(text: &str, requested: CrashLogFormat) -> CrashSignature {
    let format = match requested {
        CrashLogFormat::Auto => detect_format(text),
        other => other,
    };
    match format {
        CrashLogFormat::Auto => unreachable!("auto is resolved before parser dispatch"),
        CrashLogFormat::CrashLoggerSse => CrashLoggerSseParser.parse(text),
        CrashLogFormat::NetScriptFramework => NetScriptFrameworkParser.parse(text),
        CrashLogFormat::Trainwreck => TrainwreckParser.parse(text),
        CrashLogFormat::Generic => GenericCrashParser.parse(text),
    }
}

#[must_use]
pub fn correlate_crash_log(
    source_path: &Path,
    raw_text: &str,
    requested_format: CrashLogFormat,
    input: CrashCorrelationInput,
) -> CrashCorrelationReport {
    let signature = parse_crash_log(raw_text, requested_format);
    let raw_sha256 = sha256_hex(raw_text.as_bytes());
    let suspects = correlate_tokens(&signature.tokens, &input);
    CrashCorrelationReport {
        game_id: input.game_id,
        profile_name: input.profile.name,
        source_path: source_path.to_path_buf(),
        raw_sha256,
        format: signature.detected_format,
        signature,
        suspects,
    }
}

fn correlate_tokens(tokens: &[CrashToken], input: &CrashCorrelationInput) -> Vec<CrashSuspect> {
    let mut by_key: HashMap<String, CrashSuspect> = HashMap::new();
    let mods_by_id = input
        .profile
        .mods
        .iter()
        .map(|m| (m.mod_id.to_lowercase(), m))
        .collect::<HashMap<_, _>>();
    let active_plugins = input
        .active_plugins
        .iter()
        .filter(|p| p.enabled)
        .map(|p| (p.plugin_name.to_lowercase(), p))
        .collect::<HashMap<_, _>>();

    for token in tokens {
        match token.kind {
            CrashTokenKind::Plugin => {
                let key = token.value.to_lowercase();
                let plugin = active_plugins.get(&key);
                let matched = input.profile.mods.iter().find(|m| {
                    m.enabled
                        && (m.mod_id.eq_ignore_ascii_case(&token.value)
                            || m.display_name
                                .as_deref()
                                .is_some_and(|name| name.eq_ignore_ascii_case(&token.value)))
                });
                upsert_suspect(
                    &mut by_key,
                    matched,
                    Some(token.value.clone()),
                    plugin.map(|p| p.sort_index),
                    token,
                    CrashConfidence::Highest,
                    "exact active plugin name mentioned by log",
                );
            }
            CrashTokenKind::Dll => {
                let needle = token.value.to_lowercase();
                let matched = input.installed_files.iter().find_map(|(mod_id, file)| {
                    file.rel_path
                        .rsplit(['/', '\\'])
                        .next()
                        .is_some_and(|name| name.eq_ignore_ascii_case(&needle))
                        .then(|| mods_by_id.get(&mod_id.to_lowercase()).copied())
                        .flatten()
                });
                let is_tool_file = input.tool_files.iter().any(|path| {
                    path.rsplit(['/', '\\'])
                        .next()
                        .is_some_and(|name| name.eq_ignore_ascii_case(&needle))
                });
                upsert_suspect(
                    &mut by_key,
                    matched,
                    None,
                    None,
                    token,
                    CrashConfidence::High,
                    if is_tool_file {
                        "DLL basename mentioned by log and also tracked as tool-applied file"
                    } else {
                        "DLL basename mentioned by log and matched to installed file manifest"
                    },
                );
            }
            CrashTokenKind::AssetPath => {
                let needle = normalize_path(&token.value);
                let matched = input.installed_files.iter().find_map(|(mod_id, file)| {
                    let rel = normalize_path(&file.rel_path);
                    (rel == needle || rel.ends_with(&needle) || needle.ends_with(&rel))
                        .then(|| mods_by_id.get(&mod_id.to_lowercase()).copied())
                        .flatten()
                });
                upsert_suspect(
                    &mut by_key,
                    matched,
                    None,
                    None,
                    token,
                    CrashConfidence::Medium,
                    "asset path mentioned by log and matched to installed file manifest",
                );
            }
            CrashTokenKind::FormId => {
                upsert_suspect(
                    &mut by_key,
                    None,
                    None,
                    None,
                    token,
                    CrashConfidence::Low,
                    "form ID mentioned by log; no local plugin ownership is inferred",
                );
            }
        }
    }

    let mut suspects = by_key.into_values().collect::<Vec<_>>();
    for suspect in &mut suspects {
        suspect.confidence = suspect
            .evidence
            .iter()
            .map(|e| match e.reason.as_str() {
                "exact active plugin name mentioned by log" => CrashConfidence::Highest,
                reason if reason.contains("DLL basename") => CrashConfidence::High,
                reason if reason.contains("asset path") => CrashConfidence::Medium,
                _ => CrashConfidence::Low,
            })
            .max()
            .unwrap_or(CrashConfidence::Low);
        let name = suspect
            .display_name
            .as_deref()
            .or(suspect.mod_id.as_deref())
            .or(suspect.plugin_name.as_deref())
            .unwrap_or("unmatched crash evidence");
        suspect.summary =
            format!("{name} is mentioned by the crash log; review the evidence rows.");
    }
    suspects.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.summary.cmp(&b.summary))
    });
    suspects
}

fn upsert_suspect(
    by_key: &mut HashMap<String, CrashSuspect>,
    matched_mod: Option<&EnabledMod>,
    plugin_name: Option<String>,
    plugin_load_index: Option<i64>,
    token: &CrashToken,
    confidence: CrashConfidence,
    reason: &str,
) {
    let key = matched_mod
        .map(|m| format!("mod:{}", m.mod_id.to_lowercase()))
        .or_else(|| {
            plugin_name
                .as_ref()
                .map(|p| format!("plugin:{}", p.to_lowercase()))
        })
        .unwrap_or_else(|| format!("token:{}:{}", token.kind as u8, token.value.to_lowercase()));

    let suspect = by_key.entry(key).or_insert_with(|| CrashSuspect {
        mod_id: matched_mod.map(|m| m.mod_id.clone()),
        display_name: matched_mod.and_then(|m| m.display_name.clone()),
        version: matched_mod.and_then(|m| m.version.clone()),
        nexus_mod_id: matched_mod
            .and_then(|m| m.nexus_mod_id)
            .and_then(|id| id.to_i64().ok()),
        nexus_file_id: matched_mod
            .and_then(|m| m.nexus_file_id)
            .and_then(|id| id.to_i64().ok()),
        nexus_game_domain: matched_mod.and_then(|m| m.nexus_game_domain.clone()),
        installed_timestamp: matched_mod.and_then(|m| m.installed_timestamp),
        plugin_name,
        plugin_load_index,
        evidence: Vec::new(),
        confidence,
        summary: String::new(),
    });

    if confidence > suspect.confidence {
        suspect.confidence = confidence;
    }
    suspect.evidence.push(CrashEvidence {
        token: token.value.clone(),
        kind: token.kind,
        section: token.section.clone(),
        reason: reason.to_string(),
        line: token.line,
    });
}

fn parse_with_sections(
    text: &str,
    format: CrashLogFormat,
    classify_section: fn(&str) -> Option<&'static str>,
) -> CrashSignature {
    let mut tokens = Vec::new();
    let mut exception_line = None;
    let mut section = "header".to_string();
    for (idx, line) in text.lines().enumerate() {
        let line_no = idx + 1;
        if exception_line.is_none() && line.to_lowercase().contains("exception") {
            exception_line = Some(line.trim().to_string());
        }
        if let Some(next) = classify_section(line.trim()) {
            section = next.to_string();
            continue;
        }
        for token in extract_tokens(line, &section, line_no) {
            tokens.push(token);
        }
    }
    dedupe_tokens(&mut tokens);
    CrashSignature {
        detected_format: format,
        exception_line,
        tokens,
    }
}

fn crash_logger_section(line: &str) -> Option<&'static str> {
    match line {
        "PROBABLE CALL STACK:" | "CALL STACK ([P]robable / [S]tack scan):" => Some("call-stack"),
        "REGISTERS:" => Some("registers"),
        "STACK:" => Some("stack"),
        "MODULES:" => Some("modules"),
        "SKSE PLUGINS:" => Some("script-extender-plugins"),
        "PLUGINS:" => Some("plugins"),
        "Possible relevant objects" | "POSSIBLE RELEVANT OBJECTS:" => Some("relevant-objects"),
        _ => None,
    }
}

fn trainwreck_section(line: &str) -> Option<&'static str> {
    match line {
        "PROBABLE CALL STACK:" => Some("call-stack"),
        "REGISTERS:" => Some("registers"),
        "STACK:" => Some("stack"),
        "MODULES:" => Some("modules"),
        "SCRIPT EXTENDER PLUGINS:" => Some("script-extender-plugins"),
        _ => None,
    }
}

fn net_script_framework_section(line: &str) -> Option<&'static str> {
    let normalized = line
        .trim_matches(|c: char| c == '=' || c == '-' || c.is_whitespace())
        .to_ascii_lowercase();
    match normalized.as_str() {
        "possible relevant objects" => Some("relevant-objects"),
        "probable callstack" | "probable call stack" => Some("call-stack"),
        "registers" => Some("registers"),
        "stack" => Some("stack"),
        "modules" => Some("modules"),
        "plugins" => Some("plugins"),
        _ => None,
    }
}

fn extract_tokens(line: &str, section: &str, line_no: usize) -> Vec<CrashToken> {
    let mut out = Vec::new();
    let cleaned = line.trim_matches(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ';' | ':' | '(' | ')' | '[' | ']')
    });
    for raw in cleaned.split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>')) {
        let token = raw.trim_matches(|c: char| {
            matches!(
                c,
                '"' | '\'' | '`' | ',' | ';' | ':' | '(' | ')' | '[' | ']' | '{' | '}'
            )
        });
        if token.len() < 4 {
            continue;
        }
        let token = token.split_once('+').map_or(token, |(module, _)| module);
        let lower = token.to_lowercase();
        let kind = if lower.ends_with(".esp") || lower.ends_with(".esm") || lower.ends_with(".esl")
        {
            Some(CrashTokenKind::Plugin)
        } else if lower.ends_with(".dll") {
            Some(CrashTokenKind::Dll)
        } else if looks_like_asset_path(&lower) {
            Some(CrashTokenKind::AssetPath)
        } else if looks_like_form_id(token) {
            Some(CrashTokenKind::FormId)
        } else {
            None
        };
        if let Some(kind) = kind {
            out.push(CrashToken {
                value: token.to_string(),
                kind,
                section: section.to_string(),
                line: line_no,
            });
        }
    }
    out
}

fn looks_like_asset_path(lower: &str) -> bool {
    (lower.contains('/') || lower.contains('\\'))
        && [
            ".nif", ".dds", ".hkx", ".pex", ".psc", ".bsa", ".ba2", ".ini", ".json", ".toml",
            ".yaml", ".xml",
        ]
        .iter()
        .any(|suffix| lower.ends_with(suffix))
}

fn looks_like_form_id(token: &str) -> bool {
    let trimmed = token.strip_prefix("0x").unwrap_or(token);
    trimmed.len() == 8 && trimmed.chars().all(|c| c.is_ascii_hexdigit())
}

fn dedupe_tokens(tokens: &mut Vec<CrashToken>) {
    let mut seen = BTreeSet::new();
    tokens.retain(|token| {
        seen.insert((
            token.value.to_lowercase(),
            token.kind,
            token.section.clone(),
            token.line,
        ))
    });
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().fold(String::with_capacity(64), |mut out, b| {
        write!(&mut out, "{b:02x}").expect("writing to String cannot fail");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::ProfileSource;
    use crate::resolver::GameId;

    fn enabled_mod(mod_id: &str, display_name: Option<&str>) -> EnabledMod {
        EnabledMod {
            mod_id: mod_id.to_string(),
            display_name: display_name.map(str::to_string),
            enabled: true,
            ..EnabledMod::default()
        }
    }

    fn staged_file(rel_path: &str) -> StagedFile {
        StagedFile {
            rel_path: rel_path.to_string(),
            origin_rel_path: rel_path.to_string(),
            size: 1024,
            merge_group: None,
        }
    }

    /// Synthetic install state matching the fixture logs below:
    /// - `broken-mod` ships the active plugin `BrokenMod.esp`
    /// - `bad-skse-plugin` ships the native DLL `badplugin.dll`
    fn correlation_input() -> CrashCorrelationInput {
        let profile = Profile {
            id: Some(1),
            name: "default".to_string(),
            game_id: GameId::from("skyrim-se"),
            source: ProfileSource::Manual,
            mods: vec![
                enabled_mod("broken-mod", Some("BrokenMod.esp")),
                enabled_mod("bad-skse-plugin", Some("Bad SKSE Plugin")),
                enabled_mod("innocent-mod", Some("Innocent Mod")),
            ],
            overrides: PathBuf::from("/tmp/overrides"),
            load_order_rules: Default::default(),
            load_order_lock: None,
        };
        CrashCorrelationInput {
            game_id: "skyrim-se".to_string(),
            profile,
            active_plugins: vec![
                PluginEntry {
                    plugin_name: "Skyrim.esm".to_string(),
                    sort_index: 0,
                    enabled: true,
                },
                PluginEntry {
                    plugin_name: "BrokenMod.esp".to_string(),
                    sort_index: 1,
                    enabled: true,
                },
            ],
            installed_files: vec![
                (
                    "bad-skse-plugin".to_string(),
                    staged_file("SKSE/Plugins/badplugin.dll"),
                ),
                ("broken-mod".to_string(), staged_file("BrokenMod.esp")),
            ],
            tool_files: Vec::new(),
        }
    }

    fn crash_logger_sse_fixture() -> &'static str {
        "Skyrim SSE v1.6.1170\n\
         CrashLoggerSSE v1-14-1-0\n\
         Unhandled exception \"EXCEPTION_ACCESS_VIOLATION\" at 0x7FF61D2A4B30\n\
         \n\
         PROBABLE CALL STACK:\n\
         \t[0] badplugin.dll+0012F30\n\
         \t[1] SkyrimSE.exe+05B11F0\n\
         \n\
         REGISTERS:\n\
         \tRAX 0x0\n\
         \n\
         STACK:\n\
         \t[RSP+0] 0x7FF61D2A4B30\n\
         \n\
         MODULES:\n\
         \tSkyrimSE.exe\n\
         \tbadplugin.dll\n\
         \n\
         SKSE PLUGINS:\n\
         \tbadplugin.dll v1.2.3\n\
         \n\
         PLUGINS:\n\
         \t[00] Skyrim.esm\n\
         \t[FE:000] BrokenMod.esp\n"
    }

    fn net_script_framework_fixture() -> &'static str {
        ".NET Script Framework version 18\n\
         Unhandled native exception occurred at 0x7FF61D2A4B30\n\
         \n\
         Possible relevant objects\n\
         [ 1] TESObjectREFR(FormId: 0x00012345, File: `BrokenMod.esp`)\n\
         \n\
         Probable callstack\n\
         [0] badplugin.dll+0012F30\n\
         [1] SkyrimSE.exe+05B11F0\n\
         \n\
         Modules\n\
         badplugin.dll\n\
         SkyrimSE.exe\n\
         \n\
         Plugins\n\
         [00] Skyrim.esm\n\
         [FE:000] BrokenMod.esp\n"
    }

    fn assert_correlation(report: &CrashCorrelationReport) {
        // The .esp suspect must be matched with highest-confidence evidence.
        let esp = report
            .suspects
            .iter()
            .find(|s| s.mod_id.as_deref() == Some("broken-mod"))
            .expect("BrokenMod.esp should be correlated to mod 'broken-mod'");
        assert_eq!(esp.confidence, CrashConfidence::Highest);
        assert_eq!(esp.plugin_name.as_deref(), Some("BrokenMod.esp"));
        assert!(
            esp.evidence.iter().any(|e| e.kind == CrashTokenKind::Plugin
                && e.reason == "exact active plugin name mentioned by log"),
            "expected exact-active-plugin evidence, got: {:?}",
            esp.evidence
        );

        // The DLL must be matched against the installed-files manifest.
        let dll = report
            .suspects
            .iter()
            .find(|s| s.mod_id.as_deref() == Some("bad-skse-plugin"))
            .expect("badplugin.dll should be correlated to mod 'bad-skse-plugin'");
        assert_eq!(dll.confidence, CrashConfidence::High);
        assert!(
            dll.evidence.iter().any(|e| e.kind == CrashTokenKind::Dll
                && e.token.eq_ignore_ascii_case("badplugin.dll")
                && e.reason.contains("installed file manifest")),
            "expected DLL manifest-match evidence, got: {:?}",
            dll.evidence
        );

        // Suspects are ranked: confidence is non-increasing down the list,
        // and the plugin suspect outranks the DLL suspect.
        assert!(
            report
                .suspects
                .windows(2)
                .all(|w| w[0].confidence >= w[1].confidence),
            "suspects must be sorted by descending confidence"
        );
        let esp_rank = report
            .suspects
            .iter()
            .position(|s| s.mod_id.as_deref() == Some("broken-mod"))
            .expect("esp suspect present");
        let dll_rank = report
            .suspects
            .iter()
            .position(|s| s.mod_id.as_deref() == Some("bad-skse-plugin"))
            .expect("dll suspect present");
        assert_eq!(esp_rank, 0, "highest-confidence suspect must rank first");
        assert!(esp_rank < dll_rank);
    }

    #[test]
    fn crash_logger_sse_log_correlates_end_to_end() {
        let report = correlate_crash_log(
            Path::new("/tmp/crash-2026-06-12.log"),
            crash_logger_sse_fixture(),
            CrashLogFormat::Auto,
            correlation_input(),
        );
        assert_eq!(report.format, CrashLogFormat::CrashLoggerSse);
        assert_eq!(report.game_id, "skyrim-se");
        assert_eq!(report.profile_name, "default");
        assert_eq!(report.raw_sha256.len(), 64);
        assert!(
            report
                .signature
                .exception_line
                .as_deref()
                .is_some_and(|line| line.contains("EXCEPTION_ACCESS_VIOLATION"))
        );
        assert_correlation(&report);
    }

    #[test]
    fn net_script_framework_log_autodetects_and_correlates() {
        let report = correlate_crash_log(
            Path::new("/tmp/Crash_2026_06_12.txt"),
            net_script_framework_fixture(),
            CrashLogFormat::Auto,
            correlation_input(),
        );
        assert_eq!(report.format, CrashLogFormat::NetScriptFramework);
        assert_correlation(&report);
    }

    #[test]
    fn crashlogger_parser_uses_upstream_section_headers() {
        let log = "Skyrim SSE v1.6.1170\nCrashLoggerSSE v1-14-1-0\nUnhandled native exception occurred\n\nPROBABLE CALL STACK:\n\tSomeMod.dll+123\n\nREGISTERS:\n\nSTACK:\n\tData\\meshes\\foo\\bar.nif\n\nMODULES:\n\tSomeMod.dll\n\nSKSE PLUGINS:\n\tSomeMod.dll\n\nPLUGINS:\n\t[FE:000] Example.esp\n";
        let signature = parse_crash_log(log, CrashLogFormat::CrashLoggerSse);
        assert_eq!(signature.detected_format, CrashLogFormat::CrashLoggerSse);
        assert!(signature.tokens.iter().any(|t| t.value == "Example.esp"));
        assert!(signature.tokens.iter().any(|t| t.value == "SomeMod.dll"));
        assert!(
            signature
                .tokens
                .iter()
                .any(|t| t.value == "Data\\meshes\\foo\\bar.nif")
        );
    }

    #[test]
    fn trainwreck_parser_uses_script_extender_plugins_header() {
        let log = "Trainwreck v1.4.0\nUnhandled native exception occurred\n\nPROBABLE CALL STACK:\n\tEngineFixes.dll+ABC\n\nREGISTERS:\n\nSTACK:\n\nMODULES:\n\tEngineFixes.dll\n\nSCRIPT EXTENDER PLUGINS:\n\tEngineFixes.dll\n";
        let signature = parse_crash_log(log, CrashLogFormat::Trainwreck);
        assert_eq!(signature.detected_format, CrashLogFormat::Trainwreck);
        assert!(
            signature
                .tokens
                .iter()
                .any(|t| t.section == "script-extender-plugins" && t.value == "EngineFixes.dll")
        );
    }

    #[test]
    fn net_script_framework_parser_detects_sections() {
        let log = ".NET Script Framework v18\nUnhandled native exception\n\nPossible relevant objects\n[ 1] TESObjectREFR(FormId: 00012345, File: `Example.esp`)\n\nProbable callstack\nSomeMod.dll+123\n\nStack\nData\\textures\\foo.dds\n";
        let signature = parse_crash_log(log, CrashLogFormat::Auto);
        assert_eq!(
            signature.detected_format,
            CrashLogFormat::NetScriptFramework
        );
        assert!(signature.tokens.iter().any(|t| t.value == "Example.esp"));
        assert!(signature.tokens.iter().any(|t| t.value == "SomeMod.dll"));
        assert!(
            signature
                .tokens
                .iter()
                .any(|t| t.value == "Data\\textures\\foo.dds")
        );
    }
}
