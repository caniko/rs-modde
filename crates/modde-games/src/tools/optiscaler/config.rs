use super::*;

pub(super) fn optiscaler_config_reset_reason(
    existing: &OptiScalerInstallState,
    new_ini: &Path,
    config: &ToolConfig,
) -> Option<String> {
    if config.get_bool("force_config_reset") {
        return Some("forced by setting".to_string());
    }
    let Ok(new_content) = std::fs::read_to_string(new_ini) else {
        return None;
    };
    let new_keys: BTreeSet<String> = parse_ini_keys(&new_content).into_iter().collect();
    let old_unknown = existing
        .ini_settings
        .keys()
        .any(|key| !new_keys.contains(key));
    old_unknown.then_some("schema mismatch".to_string())
}

pub(super) fn flatten_ini_overrides(
    overrides: &serde_json::Map<String, serde_json::Value>,
) -> Vec<(String, serde_json::Value)> {
    fn walk(prefix: &str, value: &serde_json::Value, out: &mut Vec<(String, serde_json::Value)>) {
        if let serde_json::Value::Object(map) = value {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                walk(&path, child, out);
            }
        } else {
            out.push((prefix.to_string(), value.clone()));
        }
    }

    let mut out = Vec::new();
    for (key, value) in overrides {
        walk(key, value, &mut out);
    }
    out
}

pub(super) fn set_ini_value(content: &str, path: &str, value: &str) -> String {
    let Some((target_section, target_key)) = path.rsplit_once('.') else {
        tracing::warn!(
            ini_key = path,
            "optiscaler: ini override key has no section prefix; expected 'section.key'"
        );
        return content.to_string();
    };
    let mut current_section = "";
    let mut updated = false;
    let mut lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if !updated && !target_section.is_empty() && current_section == target_section {
                lines.push(format!("{target_key}={value}"));
                updated = true;
            }
            current_section = trimmed[1..trimmed.len() - 1].trim();
        }

        if current_section == target_section
            && let Some((key, _)) = trimmed.split_once('=')
            && key.trim() == target_key
        {
            lines.push(format!("{target_key}={value}"));
            updated = true;
            continue;
        }
        lines.push(line.to_string());
    }

    if !updated && !target_section.is_empty() && current_section == target_section {
        lines.push(format!("{target_key}={value}"));
        updated = true;
    }

    if !updated {
        if !target_section.is_empty() {
            lines.push(format!("[{target_section}]"));
        }
        lines.push(format!("{target_key}={value}"));
    }

    lines.join("\n") + "\n"
}
