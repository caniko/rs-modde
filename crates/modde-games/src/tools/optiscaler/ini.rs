use super::config::{flatten_ini_overrides, set_ini_value};
use super::*;

pub fn parse_optiscaler_ini(content: &str) -> BTreeMap<String, String> {
    let mut section = String::new();
    let mut values = BTreeMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed[1..trimmed.len() - 1].trim().to_string();
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            if !key.is_empty() {
                let path = if section.is_empty() {
                    key.to_string()
                } else {
                    format!("{section}.{key}")
                };
                values.insert(path, value.trim().to_string());
            }
        }
    }
    values
}

pub(super) fn parse_ini_keys(content: &str) -> Vec<String> {
    let mut section = String::new();
    let mut keys = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed[1..trimmed.len() - 1].trim().to_string();
            continue;
        }
        if let Some((key, _)) = trimmed.split_once('=') {
            let key = key.trim();
            if !key.is_empty() {
                if section.is_empty() {
                    keys.push(key.to_string());
                } else {
                    keys.push(format!("{section}.{key}"));
                }
            }
        }
    }
    keys
}

pub(super) fn apply_ini_overrides_with_existing(
    src: &Path,
    existing: Option<&Path>,
    dest: &Path,
    config: &ToolConfig,
) -> Result<()> {
    let content = build_ini_with_overrides(src, existing, config)?;
    std::fs::write(dest, content).with_context(|| format!("failed to write {}", dest.display()))?;
    Ok(())
}

pub(super) fn build_ini_with_overrides(
    src: &Path,
    existing: Option<&Path>,
    config: &ToolConfig,
) -> Result<String> {
    let mut content = std::fs::read_to_string(src)
        .with_context(|| format!("failed to read {}", src.display()))?;
    if let Some(existing) = existing
        && let Ok(existing_content) = std::fs::read_to_string(existing)
    {
        let source_keys: BTreeSet<String> = parse_ini_keys(&content).into_iter().collect();
        for (key, value) in parse_optiscaler_ini(&existing_content) {
            if source_keys.contains(&key) {
                content = set_ini_value(&content, &key, &value);
            }
        }
    }
    if let Some(overrides) = config
        .settings
        .get("ini_overrides")
        .and_then(serde_json::Value::as_object)
    {
        for (path, value) in flatten_ini_overrides(overrides) {
            let value = match value {
                serde_json::Value::String(value) => value,
                other => other.to_string(),
            };
            content = set_ini_value(&content, path.as_str(), value.as_str());
        }
    }
    for (path, value) in effective_optiscaler_ini_overrides(config) {
        content = set_ini_value(&content, path, value);
    }
    Ok(content)
}

pub(super) fn effective_optiscaler_ini_overrides(
    config: &ToolConfig,
) -> Vec<(&'static str, &'static str)> {
    let mut overrides = Vec::new();
    overrides.push((
        "FSR.Fsr4Update",
        match fsr4_variant(config) {
            FSR4_VARIANT_INT8_402 => "auto",
            _ => "True",
        },
    ));
    overrides.push((
        "Spoofing.Dxgi",
        if config.get_bool("spoof_dlss") {
            "auto"
        } else {
            "false"
        },
    ));
    overrides.push((
        "Plugins.LoadAsiPlugins",
        if config.get_bool("enable_optipatcher") {
            "true"
        } else {
            "auto"
        },
    ));
    overrides
}
