use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::collision::CollisionReport;
use crate::profile::Profile;
use crate::resolver::{ConflictMap, ModId};

/// Severity level for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// A suggested fix for a diagnostic.
#[derive(Debug, Clone)]
pub struct DiagFix {
    pub label: String,
    pub description: String,
}

/// A single diagnostic finding.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub affected_mod: Option<String>,
    pub affected_file: Option<PathBuf>,
    pub fix: Option<DiagFix>,
}

/// Context passed to diagnostic rules for analysis.
pub struct DiagContext<'a> {
    pub game_id: &'a str,
    pub profile: &'a Profile,
    pub active_plugins: &'a [String],
    pub conflict_map: &'a ConflictMap,
    pub collision_report: Option<&'a CollisionReport>,
    pub store_dir: &'a Path,
    pub staging_dir: &'a Path,
}

/// Shared analysis result used by the CLI, UI, and tests.
pub struct ProfileAnalysis {
    pub resolved_order: Vec<ModId>,
    pub conflict_map: ConflictMap,
    pub collision_report: Option<CollisionReport>,
}

/// Build real conflict and collision state for a profile.
pub fn analyze_profile_state(
    profile: &Profile,
    store_dir: &Path,
    hidden: &std::collections::HashSet<(String, String)>,
    classifier: Option<&dyn crate::collision::CollisionClassifier>,
) -> Result<ProfileAnalysis> {
    let resolved_order = crate::resolver::resolve(profile)?.order;

    let Some(classifier) = classifier else {
        return Ok(ProfileAnalysis {
            resolved_order,
            conflict_map: ConflictMap::default(),
            collision_report: None,
        });
    };

    let (conflict_map, origins) =
        crate::collision::build_full_conflict_map(store_dir, &resolved_order, classifier)?;
    let collision_report = crate::collision::analyze_collisions(
        &conflict_map,
        &resolved_order,
        hidden,
        &origins,
        classifier,
    );

    Ok(ProfileAnalysis {
        resolved_order,
        conflict_map,
        collision_report: Some(collision_report),
    })
}

/// Run a diagnostic engine against a fully analyzed profile.
pub fn run_profile_diagnostics(
    game_id: &str,
    profile: &Profile,
    active_plugins: &[String],
    store_dir: &Path,
    staging_dir: &Path,
    hidden: &std::collections::HashSet<(String, String)>,
    classifier: Option<&dyn crate::collision::CollisionClassifier>,
    engine: &DiagnosticEngine,
) -> Result<(Vec<Diagnostic>, ProfileAnalysis)> {
    let analysis = analyze_profile_state(profile, store_dir, hidden, classifier)?;
    let ctx = DiagContext {
        game_id,
        profile,
        active_plugins,
        conflict_map: &analysis.conflict_map,
        collision_report: analysis.collision_report.as_ref(),
        store_dir,
        staging_dir,
    };

    Ok((engine.run_all(&ctx), analysis))
}

// ── Collision-aware diagnostic rules ────────────────────────────────

/// Warn about mods whose files are all overridden by higher-priority mods.
pub struct ShadowedModRule;

impl DiagnosticRule for ShadowedModRule {
    fn name(&self) -> &'static str {
        "shadowed-mod"
    }

    fn check(&self, ctx: &DiagContext) -> Vec<Diagnostic> {
        let Some(report) = ctx.collision_report else {
            return Vec::new();
        };
        report
            .shadowed_mods
            .iter()
            .map(|sm| {
                let by: Vec<&str> = sm
                    .shadowed_by
                    .iter()
                    .map(super::resolver::ModId::as_str)
                    .collect();
                Diagnostic {
                    severity: Severity::Warning,
                    title: format!("Mod \"{}\" is completely shadowed", sm.mod_id),
                    detail: format!(
                        "All {} files are overridden by: {}. Consider disabling this mod.",
                        sm.file_count,
                        by.join(", ")
                    ),
                    affected_mod: Some(sm.mod_id.to_string()),
                    affected_file: None,
                    fix: Some(DiagFix {
                        label: "Disable mod".to_string(),
                        description: format!("Disable \"{}\" to reduce deployment size", sm.mod_id),
                    }),
                }
            })
            .collect()
    }
}

/// Warn about dangerous script/plugin/DLL collisions.
pub struct DangerousCollisionRule;

impl DiagnosticRule for DangerousCollisionRule {
    fn name(&self) -> &'static str {
        "dangerous-collision"
    }

    fn check(&self, ctx: &DiagContext) -> Vec<Diagnostic> {
        let Some(report) = ctx.collision_report else {
            return Vec::new();
        };
        report
            .pairs
            .iter()
            .filter(|p| p.max_severity == crate::collision::CollisionSeverity::Dangerous)
            .map(|pair| {
                let dangerous_files: Vec<&str> = pair
                    .files
                    .iter()
                    .filter(|f| f.severity == crate::collision::CollisionSeverity::Dangerous)
                    .map(|f| f.file_path.as_str())
                    .collect();
                Diagnostic {
                    severity: Severity::Warning,
                    title: format!("Dangerous collision: {} vs {}", pair.loser, pair.winner),
                    detail: format!(
                        "{} script/plugin/DLL files conflict: {}",
                        dangerous_files.len(),
                        dangerous_files.join(", ")
                    ),
                    affected_mod: Some(pair.loser.to_string()),
                    affected_file: None,
                    fix: Some(DiagFix {
                        label: "Review load order".to_string(),
                        description: format!(
                            "Check that \"{}\" winning over \"{}\" is intentional for these files",
                            pair.winner, pair.loser
                        ),
                    }),
                }
            })
            .collect()
    }
}

/// A diagnostic rule that checks for specific issues.
pub trait DiagnosticRule: Send + Sync {
    fn name(&self) -> &str;
    fn check(&self, ctx: &DiagContext) -> Vec<Diagnostic>;
}

/// Engine that runs all registered rules.
pub struct DiagnosticEngine {
    rules: Vec<Box<dyn DiagnosticRule>>,
}

impl DiagnosticEngine {
    #[must_use]
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn add_rule(&mut self, rule: Box<dyn DiagnosticRule>) {
        self.rules.push(rule);
    }

    #[must_use]
    pub fn run_all(&self, ctx: &DiagContext) -> Vec<Diagnostic> {
        let mut results: Vec<Diagnostic> = self.rules.iter().flat_map(|r| r.check(ctx)).collect();
        results.sort_by_key(|d| d.severity);
        results
    }
}

impl Default for DiagnosticEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{Profile, ProfileSource};
    use crate::resolver::{ConflictMap, GameId};
    use smallvec::smallvec;
    use std::path::PathBuf;

    /// A mock rule that always returns a fixed set of diagnostics.
    struct MockRule {
        name: &'static str,
        diagnostics: Vec<Diagnostic>,
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
}
