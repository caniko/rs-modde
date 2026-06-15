use serde::{Deserialize, Serialize};

/// Per-game tool configuration stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    pub tool_id: String,
    pub enabled: bool,
    pub settings: serde_json::Value,
}

impl ToolConfig {
    pub fn new(tool_id: impl Into<String>) -> Self {
        Self {
            tool_id: tool_id.into(),
            enabled: false,
            settings: serde_json::Value::Object(serde_json::Map::new()),
        }
    }

    /// Get a string setting.
    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.settings.get(key).and_then(|v| v.as_str())
    }

    /// Get a bool setting, defaulting to `false`.
    #[must_use]
    pub fn get_bool(&self, key: &str) -> bool {
        self.settings
            .get(key)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    }

    /// Get an integer setting.
    #[must_use]
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.settings.get(key).and_then(serde_json::Value::as_i64)
    }

    /// Set a setting value.
    pub fn set(&mut self, key: impl Into<String>, value: serde_json::Value) {
        if let serde_json::Value::Object(ref mut map) = self.settings {
            map.insert(key.into(), value);
        }
    }
}
