use std::path::Path;

use anyhow::{Context, Result};

/// Safe INI patching: modify a key in a section while preserving comments and formatting.
///
/// If the key doesn't exist in the section, it is appended. If the section
/// doesn't exist, it is created at the end of the file.
pub fn patch_ini(path: &Path, section: &str, key: &str, value: &str) -> Result<()> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read INI: {}", path.display()))?;

    let patched = patch_ini_content(&content, section, key, value);

    std::fs::write(path, &patched)
        .with_context(|| format!("failed to write INI: {}", path.display()))?;

    Ok(())
}

/// Patch INI content in-memory (for testing).
pub fn patch_ini_content(content: &str, section: &str, key: &str, value: &str) -> String {
    let section_header = format!("[{section}]");
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let mut in_section = false;
    let mut key_found = false;
    let mut section_found = false;
    let mut insert_at = None;
    // Track the position right after the last non-empty content line in the
    // target section, so that when the target section is the last section we
    // insert before any trailing blank lines rather than at the very end.
    let mut last_content_line = None;

    for (i, line) in lines.iter_mut().enumerate() {
        let trimmed = line.trim();

        // Check for section headers
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_section && !key_found {
                // We left the target section without finding the key
                insert_at = Some(i);
                break;
            }
            in_section = trimmed == section_header;
            if in_section {
                section_found = true;
            }
            continue;
        }

        if in_section {
            if !trimmed.is_empty() {
                last_content_line = Some(i);
            }
            if !trimmed.is_empty() && !trimmed.starts_with(';') && !trimmed.starts_with('#') {
                if let Some((k, _)) = trimmed.split_once('=') {
                    if k.trim() == key.trim() {
                        *line = format!("{key}={value}");
                        key_found = true;
                    }
                }
            }
        }
    }

    if key_found {
        return lines.join("\n");
    }

    let new_line = format!("{key}={value}");

    if section_found {
        if let Some(pos) = insert_at {
            lines.insert(pos, new_line);
        } else if let Some(last) = last_content_line {
            // Target section is the last section; insert right after the last
            // non-empty line instead of appending to the very end of the file.
            lines.insert(last + 1, new_line);
        } else {
            // Section header exists but has no content lines; append at end.
            lines.push(new_line);
        }
    } else {
        // Section doesn't exist; create it
        lines.push(String::new());
        lines.push(section_header);
        lines.push(new_line);
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_patch_existing_key() {
        let content = "[General]\nbLanguage=ENGLISH\niStoryManagerLogging=0\n";
        let result = patch_ini_content(content, "General", "bLanguage", "FRENCH");
        assert!(result.contains("bLanguage=FRENCH"));
        assert!(!result.contains("ENGLISH"));
    }

    #[test]
    fn test_patch_new_key_existing_section() {
        let content = "[General]\nbLanguage=ENGLISH\n";
        let result = patch_ini_content(content, "General", "sNewKey", "value");
        assert!(result.contains("sNewKey=value"));
        assert!(result.contains("bLanguage=ENGLISH"));
    }

    #[test]
    fn test_patch_new_section() {
        let content = "[General]\nbLanguage=ENGLISH\n";
        let result = patch_ini_content(content, "Display", "iSize", "1920");
        assert!(result.contains("[Display]"));
        assert!(result.contains("iSize=1920"));
    }

    #[test]
    fn test_preserves_comments() {
        let content = "[General]\n; This is a comment\nbLanguage=ENGLISH\n";
        let result = patch_ini_content(content, "General", "bLanguage", "FRENCH");
        assert!(result.contains("; This is a comment"));
    }
}
