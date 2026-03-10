use serde::Deserialize;

/// REDmod mod manifest (`info.json`).
#[derive(Debug, Clone, Deserialize)]
pub struct RedModdeifest {
    pub name: String,
    pub version: Option<String>,
    #[serde(default)]
    pub custom_sounds: Vec<CustomSound>,
    #[serde(default)]
    pub scripts: Vec<ScriptEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CustomSound {
    pub name: String,
    #[serde(rename = "type")]
    pub sound_type: Option<String>,
    pub file: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScriptEntry {
    pub name: String,
    pub path: Option<String>,
}

impl RedModdeifest {
    /// Parse a REDmod `info.json` file.
    pub fn parse(json: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}
