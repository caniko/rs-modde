use std::borrow::Cow;
use std::path::PathBuf;

use smallvec::SmallVec;

use crate::resolver::GameId;

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

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("hash mismatch for {path}: expected {expected}, got {actual}")]
    HashMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },

    #[error("dependency cycle detected involving mod {0}")]
    DependencyCycle(String),

    #[error("conflict: file '{path}' provided by multiple mods: {mods:?}")]
    FileConflict {
        path: String,
        mods: Box<SmallVec<[String; 4]>>,
    },

    #[error("profile '{0}' not found")]
    ProfileNotFound(String),

    #[error("profile '{0}' already exists")]
    ProfileAlreadyExists(String),

    #[error(
        "ambiguous profile name '{name}': found in games {games:?}. Use --game to disambiguate."
    )]
    AmbiguousProfile {
        name: String,
        games: SmallVec<[GameId; 4]>,
    },

    #[error("mod '{mod_id}' not found in profile '{profile}'. Available: {candidates:?}")]
    ModNotFound {
        profile: String,
        mod_id: String,
        candidates: SmallVec<[String; 5]>,
    },

    #[error("game '{0}' not detected")]
    GameNotDetected(String),

    #[error("save '{path}' is already assigned to profile '{profile}'")]
    SaveAlreadyAssigned { path: String, profile: String },

    #[error("no active profile for game '{0}'")]
    NoActiveProfile(String),

    #[error("not in experiment mode for game '{0}'")]
    NotInExperiment(String),

    #[error("save vault error: {0}")]
    SaveVaultError(String),

    #[error("game '{game_id}' has {save_count} existing saves that need adoption")]
    SaveAdoptionRequired { game_id: GameId, save_count: usize },

    #[error("unsupported filesystem operation: {0}")]
    UnsupportedFs(Cow<'static, str>),

    #[error("validation error: {0}")]
    Validation(Cow<'static, str>),

    #[error("{0}")]
    Other(Cow<'static, str>),
}

pub type Result<T> = std::result::Result<T, CoreError>;
