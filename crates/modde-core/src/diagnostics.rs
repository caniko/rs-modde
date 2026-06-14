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
    pub missing_store_mods: Vec<ModId>,
}

/// Build real conflict and collision state for a profile.
pub fn analyze_profile_state(
    profile: &Profile,
    store_dir: &Path,
    hidden: &std::collections::HashSet<(String, String)>,
    classifier: Option<&dyn crate::collision::CollisionClassifier>,
) -> Result<ProfileAnalysis> {
    let resolved_order = crate::resolver::resolve(profile)?.order;
    let missing_store_mods = resolved_order
        .iter()
        .filter(|mod_id| !store_dir.join(mod_id.as_str()).exists())
        .cloned()
        .collect::<Vec<_>>();

    let Some(classifier) = classifier else {
        return Ok(ProfileAnalysis {
            resolved_order,
            conflict_map: ConflictMap::default(),
            collision_report: None,
            missing_store_mods,
        });
    };

    let full_conflict_map =
        crate::collision::build_full_conflict_map(store_dir, &resolved_order, classifier)?;
    let collision_report = crate::collision::analyze_collisions(
        &full_conflict_map.conflict_map,
        &resolved_order,
        hidden,
        &full_conflict_map.origins,
        classifier,
    );

    Ok(ProfileAnalysis {
        resolved_order,
        conflict_map: full_conflict_map.conflict_map,
        collision_report: Some(collision_report),
        missing_store_mods: full_conflict_map.missing_mods,
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

// ── Shared diagnostic rules ─────────────────────────────────────────

/// Warn about enabled mods whose store directory is missing or empty.
pub struct StorePresenceRule;

impl DiagnosticRule for StorePresenceRule {
    fn name(&self) -> &'static str {
        "store-presence"
    }

    fn check(&self, ctx: &DiagContext) -> Vec<Diagnostic> {
        ctx.profile
            .mods
            .iter()
            .filter(|m| m.enabled)
            .filter_map(|m| {
                let mod_dir = ctx.store_dir.join(&m.mod_id);
                let is_empty = if mod_dir.exists() {
                    match std::fs::read_dir(&mod_dir) {
                        Ok(mut entries) => entries.next().is_none(),
                        Err(_) => true,
                    }
                } else {
                    true
                };

                if is_empty {
                    Some(Diagnostic {
                        severity: Severity::Warning,
                        title: format!("Empty mod: {}", m.mod_id),
                        detail: format!(
                            "Mod '{}' is enabled but has no files in the store directory. \
                             It may not have been downloaded or extracted correctly.",
                            m.mod_id
                        ),
                        affected_mod: Some(m.mod_id.clone()),
                        affected_file: Some(mod_dir),
                        fix: Some(DiagFix {
                            label: "Re-install mod".to_string(),
                            description: format!(
                                "Re-download and install '{}', or disable it if it is no longer needed.",
                                m.mod_id
                            ),
                        }),
                    })
                } else {
                    None
                }
            })
            .collect()
    }
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

/// Create diagnostics shared by every supported game.
#[must_use]
pub fn base_diagnostics() -> DiagnosticEngine {
    let mut engine = DiagnosticEngine::new();
    engine.add_rule(Box::new(StorePresenceRule));
    engine
}

#[cfg(test)]
mod tests;
