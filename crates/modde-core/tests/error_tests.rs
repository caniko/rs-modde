use std::path::PathBuf;

use modde_core::CoreError;

// ── Display messages ────────────────────────────────────────────────

#[test]
fn test_hash_mismatch_display() {
    let err = CoreError::HashMismatch {
        path: PathBuf::from("/game/data/mod.esp"),
        expected: "abcdef".to_string(),
        actual: "123456".to_string(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("/game/data/mod.esp"));
    assert!(msg.contains("abcdef"));
    assert!(msg.contains("123456"));
}

#[test]
fn test_dependency_cycle_display() {
    let err = CoreError::DependencyCycle("problematic_mod".to_string());
    let msg = format!("{err}");
    assert!(msg.contains("problematic_mod"));
    assert!(msg.contains("cycle"));
}

#[test]
fn test_file_conflict_display() {
    let err = CoreError::FileConflict {
        path: "textures/sky.dds".to_string(),
        mods: Box::new(smallvec::smallvec!["mod_a".to_string(), "mod_b".to_string()]),
    };
    let msg = format!("{err}");
    assert!(msg.contains("textures/sky.dds"));
    assert!(msg.contains("mod_a"));
    assert!(msg.contains("mod_b"));
}

#[test]
fn test_profile_not_found_display() {
    let err = CoreError::ProfileNotFound("my-profile".to_string());
    let msg = format!("{err}");
    assert!(msg.contains("my-profile"));
    assert!(msg.contains("not found"));
}

#[test]
fn test_profile_already_exists_display() {
    let err = CoreError::ProfileAlreadyExists("duplicate".to_string());
    let msg = format!("{err}");
    assert!(msg.contains("duplicate"));
    assert!(msg.contains("already exists"));
}

#[test]
fn test_game_not_detected_display() {
    let err = CoreError::GameNotDetected("skyrim-se".to_string());
    let msg = format!("{err}");
    assert!(msg.contains("skyrim-se"));
    assert!(msg.contains("not detected"));
}

#[test]
fn test_unsupported_fs_display() {
    let err = CoreError::UnsupportedFs("NTFS junctions".into());
    let msg = format!("{err}");
    assert!(msg.contains("NTFS junctions"));
}

#[test]
fn test_other_error_display() {
    let err = CoreError::Other("something went wrong".into());
    let msg = format!("{err}");
    assert_eq!(msg, "something went wrong");
}

// ── From conversions ────────────────────────────────────────────────

#[test]
fn test_io_error_converts() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    let core_err: CoreError = io_err.into();
    assert!(matches!(core_err, CoreError::Io(_)));
    let msg = format!("{core_err}");
    assert!(msg.contains("file missing"));
}

#[test]
fn test_json_error_converts() {
    let json_err = serde_json::from_str::<serde_json::Value>("{{bad json").unwrap_err();
    let core_err: CoreError = json_err.into();
    assert!(matches!(core_err, CoreError::Json(_)));
}

#[test]
fn test_toml_de_error_converts() {
    let toml_err = toml::from_str::<toml::Value>("[[[[invalid").unwrap_err();
    let core_err: CoreError = toml_err.into();
    assert!(matches!(core_err, CoreError::TomlDe(_)));
}

// ── Result type alias ───────────────────────────────────────────────

#[test]
fn test_result_alias_ok() {
    let result: modde_core::Result<i32> = Ok(42);
    assert_eq!(result.unwrap(), 42);
}

#[test]
fn test_result_alias_err() {
    let result: modde_core::Result<i32> = Err(CoreError::Other("test".into()));
    assert!(result.is_err());
}
