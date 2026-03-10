use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML serialization error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("TOML deserialization error: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("hash mismatch for {path}: expected {expected}, got {actual}")]
    HashMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },

    #[error("dependency cycle detected involving mod {0}")]
    DependencyCycle(String),

    #[error("conflict: file '{path}' provided by multiple mods: {mods:?}")]
    FileConflict { path: String, mods: Vec<String> },

    #[error("profile '{0}' not found")]
    ProfileNotFound(String),

    #[error("profile '{0}' already exists")]
    ProfileAlreadyExists(String),

    #[error("game '{0}' not detected")]
    GameNotDetected(String),

    #[error("unsupported filesystem operation: {0}")]
    UnsupportedFs(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
