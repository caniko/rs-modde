use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Plugin manifest (TOML format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    /// What this plugin provides.
    pub provides: Vec<PluginCapability>,
}

/// What a plugin can provide.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PluginCapability {
    /// Additional game support.
    Game { game_id: String },
    /// Additional installer format.
    Installer { format: String },
    /// Additional diagnostic rules.
    Diagnostic,
}

/// Load a plugin manifest from a TOML file.
pub fn load_manifest(path: &Path) -> crate::error::Result<PluginManifest> {
    let content = std::fs::read_to_string(path)?;
    let manifest: PluginManifest = toml::from_str(&content)?;
    Ok(manifest)
}

/// Scan the plugins directory for plugin manifests.
#[must_use]
pub fn scan_plugins() -> Vec<(PathBuf, PluginManifest)> {
    let plugin_dir = crate::paths::data_dir().join("plugins");
    if !plugin_dir.exists() {
        return Vec::new();
    }

    let mut plugins = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&plugin_dir) {
        for entry in entries.flatten() {
            let manifest_path = entry.path().join("plugin.toml");
            if manifest_path.exists()
                && let Ok(manifest) = load_manifest(&manifest_path) {
                    plugins.push((entry.path(), manifest));
                }
        }
    }
    plugins
}

/// List all registered plugin capabilities.
#[must_use]
pub fn list_capabilities() -> Vec<(String, PluginCapability)> {
    scan_plugins()
        .into_iter()
        .flat_map(|(_, manifest)| {
            let name = manifest.name.clone();
            manifest
                .provides
                .into_iter()
                .map(move |cap| (name.clone(), cap))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_manifest() {
        let toml_str = r#"
name = "skyrim-support"
version = "1.0.0"
description = "Adds Skyrim SE game support"
author = "modde-contrib"

[[provides]]
Game = { game_id = "skyrim-se" }

[[provides]]
Diagnostic = {}
"#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.name, "skyrim-support");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.description, "Adds Skyrim SE game support");
        assert_eq!(manifest.author, "modde-contrib");
        assert_eq!(manifest.provides.len(), 2);
    }

    #[test]
    fn test_plugin_capabilities() {
        let game_cap = PluginCapability::Game {
            game_id: "cyberpunk2077".to_string(),
        };
        let installer_cap = PluginCapability::Installer {
            format: "fomod".to_string(),
        };
        let diag_cap = PluginCapability::Diagnostic;

        // Verify equality
        assert_eq!(
            game_cap,
            PluginCapability::Game {
                game_id: "cyberpunk2077".to_string()
            }
        );
        assert_ne!(game_cap, diag_cap);
        assert_ne!(installer_cap, diag_cap);

        // Verify round-trip serialization
        let toml_str = r#"
name = "test"
version = "0.1.0"
description = "test plugin"
author = "test"

[[provides]]
Game = { game_id = "cyberpunk2077" }

[[provides]]
Installer = { format = "fomod" }

[[provides]]
Diagnostic = {}
"#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.provides.len(), 3);
        assert_eq!(manifest.provides[0], game_cap);
        assert_eq!(manifest.provides[1], installer_cap);
        assert_eq!(manifest.provides[2], diag_cap);
    }
}
