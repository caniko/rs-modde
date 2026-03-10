use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::resolver::LoadOrderRule;

/// A mod entry within a profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnabledMod {
    pub mod_id: String,
    pub enabled: bool,
    #[serde(default)]
    pub version: Option<String>,
}

/// Source from which a profile was created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProfileSource {
    Manual,
    NexusCollection { slug: String, version: String },
    Wabbajack { manifest_hash: String },
}

/// A modding profile containing an ordered list of mods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub game_id: String,
    pub source: ProfileSource,
    pub mods: Vec<EnabledMod>,
    pub overrides: PathBuf,
    #[serde(default)]
    pub load_order_rules: Vec<LoadOrderRule>,
}

impl Profile {
    /// Load a profile from a `profile.toml` file.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let profile: Profile = toml::from_str(&content)?;
        Ok(profile)
    }

    /// Save this profile to a `profile.toml` file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
        Ok(())
    }
}

/// Manages profiles on disk.
pub struct ProfileManager {
    base_dir: PathBuf,
}

impl ProfileManager {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Default base directory: `~/.local/share/modde/profiles/`.
    pub fn default_dir() -> PathBuf {
        let data_dir = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                PathBuf::from(home).join(".local/share")
            });
        data_dir.join("modde").join("profiles")
    }

    fn profile_path(&self, name: &str) -> PathBuf {
        self.base_dir.join(name).join("profile.toml")
    }

    /// List all profile names.
    pub fn list(&self) -> Result<Vec<String>> {
        let mut profiles = Vec::new();
        if !self.base_dir.exists() {
            return Ok(profiles);
        }
        for entry in std::fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let toml_path = entry.path().join("profile.toml");
                if toml_path.exists() {
                    if let Some(name) = entry.file_name().to_str() {
                        profiles.push(name.to_string());
                    }
                }
            }
        }
        Ok(profiles)
    }

    /// Load a profile by name.
    pub fn load(&self, name: &str) -> Result<Profile> {
        let path = self.profile_path(name);
        if !path.exists() {
            return Err(CoreError::ProfileNotFound(name.to_string()));
        }
        Profile::load(&path)
    }

    /// Create a new profile.
    pub fn create(&self, profile: &Profile) -> Result<()> {
        let path = self.profile_path(&profile.name);
        if path.exists() {
            return Err(CoreError::ProfileAlreadyExists(profile.name.clone()));
        }
        profile.save(&path)
    }

    /// Delete a profile by name.
    pub fn delete(&self, name: &str) -> Result<()> {
        let dir = self.base_dir.join(name);
        if !dir.exists() {
            return Err(CoreError::ProfileNotFound(name.to_string()));
        }
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
}
