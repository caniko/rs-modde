//! Edge case tests for `CoreError` variants and error handling.

use std::path::PathBuf;

use modde_core::error::CoreError;

// ── HashMismatch with empty path ─────────────────────────────────────

#[test]
fn error_hash_mismatch_empty_path() {
    let err = CoreError::HashMismatch {
        path: PathBuf::new(),
        expected: "aaa".to_string(),
        actual: "bbb".to_string(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("hash mismatch"));
    assert!(msg.contains("aaa"));
    assert!(msg.contains("bbb"));
}

#[test]
fn error_hash_mismatch_with_unicode_path() {
    let err = CoreError::HashMismatch {
        path: PathBuf::from("/tmp/\u{1F60A}/mod.esp"),
        expected: "expected_hash".to_string(),
        actual: "actual_hash".to_string(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("hash mismatch"));
    assert!(msg.contains("expected_hash"));
}

// ── DependencyCycle with very long description ───────────────────────

#[test]
fn error_dependency_cycle_long_description() {
    let long_name = "a".repeat(10_000);
    let err = CoreError::DependencyCycle(long_name.clone());
    let msg = format!("{err}");
    assert!(msg.contains("dependency cycle"));
    assert!(msg.contains(&long_name));
    assert!(msg.len() > 10_000);
}

#[test]
fn error_dependency_cycle_empty_name() {
    let err = CoreError::DependencyCycle(String::new());
    let msg = format!("{err}");
    assert!(msg.contains("dependency cycle"));
}

#[test]
fn error_dependency_cycle_special_chars() {
    let err = CoreError::DependencyCycle("mod<>&\"'with\\special/chars".to_string());
    let msg = format!("{err}");
    assert!(msg.contains("mod<>&\"'with\\special/chars"));
}

// ── FileConflict with many mods ──────────────────────────────────────

#[test]
fn error_file_conflict_many_mods() {
    let mods: smallvec::SmallVec<[String; 4]> = (0..100).map(|i| format!("mod_{i:03}")).collect();
    let err = CoreError::FileConflict {
        path: "shared/texture.dds".to_string(),
        mods: Box::new(mods.clone()),
    };
    let msg = format!("{err}");
    assert!(msg.contains("conflict"));
    assert!(msg.contains("shared/texture.dds"));
    // All 100 mods should appear in the debug output of the Vec
    assert!(msg.contains("mod_000"));
    assert!(msg.contains("mod_099"));
}

#[test]
fn error_file_conflict_empty_mods() {
    let err = CoreError::FileConflict {
        path: "file.esp".to_string(),
        mods: Box::new(smallvec::smallvec![]),
    };
    let msg = format!("{err}");
    assert!(msg.contains("conflict"));
    assert!(msg.contains("file.esp"));
}

#[test]
fn error_file_conflict_empty_path() {
    let err = CoreError::FileConflict {
        path: String::new(),
        mods: Box::new(smallvec::smallvec!["mod_a".into()]),
    };
    let msg = format!("{err}");
    assert!(msg.contains("conflict"));
}

// ── Error conversion chain ───────────────────────────────────────────

#[test]
fn error_io_to_core_error_conversion() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let core_err: CoreError = io_err.into();
    let msg = format!("{core_err}");
    assert!(msg.contains("IO error"));
    assert!(msg.contains("file not found"));
}

#[test]
fn error_io_permission_denied_conversion() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
    let core_err: CoreError = io_err.into();
    let msg = format!("{core_err}");
    assert!(msg.contains("access denied"));
}

#[test]
fn error_json_to_core_error_conversion() {
    let json_err = serde_json::from_str::<serde_json::Value>("not valid json").unwrap_err();
    let core_err: CoreError = json_err.into();
    let msg = format!("{core_err}");
    assert!(msg.contains("JSON"));
}

#[test]
fn error_toml_de_to_core_error_conversion() {
    let toml_err = toml::from_str::<toml::Value>("not = [valid toml").unwrap_err();
    let core_err: CoreError = toml_err.into();
    let msg = format!("{core_err}");
    assert!(msg.contains("TOML"));
}

#[test]
fn error_toml_ser_to_core_error_conversion() {
    // toml::ser::Error can be triggered by serializing types that toml does not support.
    // An enum variant (not a struct/map) at the top level triggers an error.
    let result = toml::to_string("bare string");
    assert!(
        result.is_err(),
        "bare string should fail toml serialization"
    );
    let core_err: CoreError = result.unwrap_err().into();
    let msg = format!("{core_err}");
    assert!(msg.contains("TOML"));
}

// ── All error variants implement Send + Sync ─────────────────────────

#[test]
fn error_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CoreError>();
}

// ── Error Debug output ───────────────────────────────────────────────

#[test]
fn error_debug_output_reasonable() {
    let errors: Vec<CoreError> = vec![
        CoreError::Io(std::io::Error::other("test")),
        CoreError::HashMismatch {
            path: PathBuf::from("/test"),
            expected: "aaa".into(),
            actual: "bbb".into(),
        },
        CoreError::DependencyCycle("mod_x".into()),
        CoreError::FileConflict {
            path: "f.esp".into(),
            mods: Box::new(smallvec::smallvec!["a".into(), "b".into()]),
        },
        CoreError::ProfileNotFound("gone".into()),
        CoreError::ProfileAlreadyExists("dup".into()),
        CoreError::GameNotDetected("nope".into()),
        CoreError::UnsupportedFs("btrfs reflink".into()),
        CoreError::Other("misc".into()),
    ];

    for err in &errors {
        let debug = format!("{err:?}");
        assert!(!debug.is_empty(), "Debug output should not be empty");
        // Debug output should contain the variant name
        let display = format!("{err}");
        assert!(!display.is_empty(), "Display output should not be empty");
        // Debug and Display should differ (Debug includes variant name / struct)
        // This is generally true for thiserror-derived types
        assert_ne!(
            debug, display,
            "Debug and Display should differ for: {debug}"
        );
    }
}

// ── Error source chain ───────────────────────────────────────────────

#[test]
fn error_io_source_preserved() {
    use std::error::Error;

    let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broke");
    let core_err: CoreError = io_err.into();

    // The source should be the original io::Error
    let source = core_err.source();
    assert!(source.is_some(), "Io variant should have a source");
    let source_msg = format!("{}", source.unwrap());
    assert!(source_msg.contains("pipe broke"));
}

#[test]
fn error_hash_mismatch_no_source() {
    use std::error::Error;

    let err = CoreError::HashMismatch {
        path: PathBuf::from("/test"),
        expected: "a".into(),
        actual: "b".into(),
    };
    // HashMismatch doesn't wrap another error, so source should be None
    assert!(err.source().is_none());
}

#[test]
fn error_other_no_source() {
    use std::error::Error;
    let err = CoreError::Other("something".into());
    assert!(err.source().is_none());
}

// ── ProfileNotFound and ProfileAlreadyExists ─────────────────────────

#[test]
fn error_profile_not_found_display() {
    let err = CoreError::ProfileNotFound("my_profile".to_string());
    assert_eq!(format!("{err}"), "profile 'my_profile' not found");
}

#[test]
fn error_profile_already_exists_display() {
    let err = CoreError::ProfileAlreadyExists("dup_profile".to_string());
    assert_eq!(format!("{err}"), "profile 'dup_profile' already exists");
}

#[test]
fn error_game_not_detected_display() {
    let err = CoreError::GameNotDetected("morrowind".to_string());
    assert_eq!(format!("{err}"), "game 'morrowind' not detected");
}

#[test]
fn error_unsupported_fs_display() {
    let err = CoreError::UnsupportedFs("reflink on ext4".into());
    assert_eq!(
        format!("{err}"),
        "unsupported filesystem operation: reflink on ext4"
    );
}

// ── CoreError is 'static ─────────────────────────────────────────────

#[test]
fn error_is_static() {
    fn assert_static<T: 'static>() {}
    assert_static::<CoreError>();
}

// ── Result type alias works ──────────────────────────────────────────

#[test]
fn error_result_type_alias() {
    fn returns_ok() -> modde_core::Result<i32> {
        Ok(42)
    }
    fn returns_err() -> modde_core::Result<i32> {
        Err(CoreError::Other("fail".into()))
    }

    assert_eq!(returns_ok().unwrap(), 42);
    assert!(returns_err().is_err());
}
