use std::collections::BTreeSet;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use super::{CrashLogFormat, CrashSignature, CrashToken, CrashTokenKind};

pub(super) fn parse_with_sections(
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

pub(super) fn crash_logger_section(line: &str) -> Option<&'static str> {
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

pub(super) fn trainwreck_section(line: &str) -> Option<&'static str> {
    match line {
        "PROBABLE CALL STACK:" => Some("call-stack"),
        "REGISTERS:" => Some("registers"),
        "STACK:" => Some("stack"),
        "MODULES:" => Some("modules"),
        "SCRIPT EXTENDER PLUGINS:" => Some("script-extender-plugins"),
        _ => None,
    }
}

pub(super) fn net_script_framework_section(line: &str) -> Option<&'static str> {
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

pub(super) fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().fold(String::with_capacity(64), |mut out, b| {
        write!(&mut out, "{b:02x}").expect("writing to String cannot fail");
        out
    })
}
