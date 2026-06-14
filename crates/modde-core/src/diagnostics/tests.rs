use super::*;
use crate::collision::{CollisionClassifier, CollisionSeverity};
use crate::profile::{EnabledMod, Profile, ProfileSource};
use crate::resolver::{ConflictMap, GameId, ModId};
use smallvec::smallvec;
use std::path::PathBuf;

/// A mock rule that always returns a fixed set of diagnostics.
struct MockRule {
    name: &'static str,
    diagnostics: Vec<Diagnostic>,
}

struct TestClassifier;

impl CollisionClassifier for TestClassifier {
    fn index_archive(&self, _archive_path: &Path) -> Result<Vec<(String, u64)>> {
        Ok(Vec::new())
    }

    fn classify_severity(&self, _file_path: &str) -> CollisionSeverity {
        CollisionSeverity::Unknown
    }

    fn archive_extensions(&self) -> &[&str] {
        &[]
    }
}

impl DiagnosticRule for MockRule {
    fn name(&self) -> &str {
        self.name
    }

    fn check(&self, _ctx: &DiagContext) -> Vec<Diagnostic> {
        self.diagnostics.clone()
    }
}

fn make_context() -> (Profile, ConflictMap, tempfile::TempDir, tempfile::TempDir) {
    let profile = Profile {
        id: None,
        name: "test".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![],
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };
    let conflict_map = ConflictMap::default();
    let store = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    (profile, conflict_map, store, staging)
}

fn enabled_mod(id: &str) -> EnabledMod {
    EnabledMod {
        mod_id: id.to_string(),
        enabled: true,
        version: None,
        fomod_config: None,
        ..Default::default()
    }
}

fn disabled_mod(id: &str) -> EnabledMod {
    EnabledMod {
        enabled: false,
        ..enabled_mod(id)
    }
}

#[test]
fn store_presence_rule_reports_missing_and_empty_enabled_mods_only() {
    let store = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();

    let with_files = store.path().join("mod-with-files");
    std::fs::create_dir_all(with_files.join("textures")).unwrap();
    std::fs::write(with_files.join("textures/sky.dds"), b"sky").unwrap();
    std::fs::create_dir_all(store.path().join("mod-empty")).unwrap();

    let profile = Profile {
        id: None,
        name: "test".to_string(),
        game_id: GameId::from("cyberpunk2077"),
        source: ProfileSource::Manual,
        mods: vec![
            enabled_mod("mod-with-files"),
            enabled_mod("mod-empty"),
            enabled_mod("mod-missing"),
            disabled_mod("mod-disabled-missing"),
        ],
        overrides: overrides.path().to_path_buf(),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };
    let conflict_map = ConflictMap::default();
    let ctx = DiagContext {
        game_id: "cyberpunk2077",
        profile: &profile,
        active_plugins: &[],
        conflict_map: &conflict_map,
        collision_report: None,
        store_dir: store.path(),
        staging_dir: staging.path(),
    };

    let diagnostics = StorePresenceRule.check(&ctx);

    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics.iter().all(|d| d.severity == Severity::Warning));
    assert!(
        diagnostics
            .iter()
            .any(|d| d.affected_mod.as_deref() == Some("mod-empty"))
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.affected_mod.as_deref() == Some("mod-missing"))
    );
    assert!(!diagnostics.iter().any(|d| {
        matches!(
            d.affected_mod.as_deref(),
            Some("mod-with-files" | "mod-disabled-missing")
        )
    }));
}

#[test]
fn base_diagnostics_includes_store_presence_rule() {
    let store = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let profile = Profile {
        id: None,
        name: "test".to_string(),
        game_id: GameId::from("cyberpunk2077"),
        source: ProfileSource::Manual,
        mods: vec![enabled_mod("mod-missing")],
        overrides: overrides.path().to_path_buf(),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };
    let conflict_map = ConflictMap::default();
    let ctx = DiagContext {
        game_id: "cyberpunk2077",
        profile: &profile,
        active_plugins: &[],
        conflict_map: &conflict_map,
        collision_report: None,
        store_dir: store.path(),
        staging_dir: staging.path(),
    };

    let results = base_diagnostics().run_all(&ctx);

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].affected_mod.as_deref(), Some("mod-missing"));
}

#[test]
fn analyze_profile_state_reports_missing_store_mods() {
    let store = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();
    let with_files = store.path().join("mod-with-files");
    std::fs::create_dir_all(with_files.join("textures")).unwrap();
    std::fs::write(with_files.join("textures/sky.dds"), b"sky").unwrap();

    let profile = Profile {
        id: None,
        name: "test".to_string(),
        game_id: GameId::from("cyberpunk2077"),
        source: ProfileSource::Manual,
        mods: vec![enabled_mod("mod-with-files"), enabled_mod("mod-missing")],
        overrides: overrides.path().to_path_buf(),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };
    let hidden = std::collections::HashSet::new();

    let analysis =
        analyze_profile_state(&profile, store.path(), &hidden, Some(&TestClassifier)).unwrap();

    assert_eq!(
        analysis.missing_store_mods,
        vec![ModId::from("mod-missing")]
    );
    assert!(analysis.conflict_map.files.contains_key("textures/sky.dds"));
}

#[test]
fn test_engine_runs_all_rules() {
    let mut engine = DiagnosticEngine::new();

    engine.add_rule(Box::new(MockRule {
        name: "rule-a",
        diagnostics: vec![Diagnostic {
            severity: Severity::Warning,
            title: "Warning A".to_string(),
            detail: "detail".to_string(),
            affected_mod: None,
            affected_file: None,
            fix: None,
        }],
    }));

    engine.add_rule(Box::new(MockRule {
        name: "rule-b",
        diagnostics: vec![Diagnostic {
            severity: Severity::Error,
            title: "Error B".to_string(),
            detail: "detail".to_string(),
            affected_mod: None,
            affected_file: None,
            fix: None,
        }],
    }));

    let (profile, conflict_map, store, staging) = make_context();
    let ctx = DiagContext {
        game_id: "skyrim-se",
        profile: &profile,
        active_plugins: &[],
        conflict_map: &conflict_map,
        collision_report: None,
        store_dir: store.path(),
        staging_dir: staging.path(),
    };

    let results = engine.run_all(&ctx);
    assert_eq!(results.len(), 2);
}

#[test]
fn test_diagnostics_sorted_by_severity() {
    let mut engine = DiagnosticEngine::new();

    engine.add_rule(Box::new(MockRule {
        name: "mixed",
        diagnostics: vec![
            Diagnostic {
                severity: Severity::Info,
                title: "Info".to_string(),
                detail: "detail".to_string(),
                affected_mod: None,
                affected_file: None,
                fix: None,
            },
            Diagnostic {
                severity: Severity::Error,
                title: "Error".to_string(),
                detail: "detail".to_string(),
                affected_mod: None,
                affected_file: None,
                fix: None,
            },
            Diagnostic {
                severity: Severity::Warning,
                title: "Warning".to_string(),
                detail: "detail".to_string(),
                affected_mod: None,
                affected_file: None,
                fix: None,
            },
        ],
    }));

    let (profile, conflict_map, store, staging) = make_context();
    let ctx = DiagContext {
        game_id: "skyrim-se",
        profile: &profile,
        active_plugins: &[],
        conflict_map: &conflict_map,
        collision_report: None,
        store_dir: store.path(),
        staging_dir: staging.path(),
    };

    let results = engine.run_all(&ctx);
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].severity, Severity::Error);
    assert_eq!(results[1].severity, Severity::Warning);
    assert_eq!(results[2].severity, Severity::Info);
}

#[test]
fn test_empty_engine() {
    let engine = DiagnosticEngine::new();

    let (profile, conflict_map, store, staging) = make_context();
    let ctx = DiagContext {
        game_id: "skyrim-se",
        profile: &profile,
        active_plugins: &[],
        conflict_map: &conflict_map,
        collision_report: None,
        store_dir: store.path(),
        staging_dir: staging.path(),
    };

    let results = engine.run_all(&ctx);
    assert!(results.is_empty());
}
