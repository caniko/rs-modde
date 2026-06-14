use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::crash::CrashCorrelationReport;
use crate::diagnostics::{Diagnostic, Severity};
use crate::installer::StagedFile;
use crate::profile::{EnabledMod, Profile};
use crate::{CollisionReport, PluginEntry};
mod evidence;
mod llm;
pub use llm::explain_with_openai_compatible_chat;

use evidence::{build_evidence, collision_summaries};

#[cfg(test)]
use evidence::{MAX_EVIDENCE_STRING_CHARS, cap_evidence_string};
#[cfg(test)]
use llm::trim_doctor_context;
#[cfg(test)]
use crate::crash::{CrashEvidence, CrashTokenKind};
#[cfg(test)]
use crate::settings::DoctorLlmSettings;



#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorProfileModSnapshot {
    pub mod_id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub enabled: bool,
    pub version: Option<String>,
    pub source_archive_hash: Option<String>,
}

impl DoctorProfileModSnapshot {
    #[must_use]
    pub fn from_profile(profile: &Profile) -> Vec<Self> {
        profile
            .mods
            .iter()
            .map(|enabled_mod| Self {
                mod_id: enabled_mod.mod_id.clone(),
                display_name: enabled_mod.display_name.clone(),
                enabled: enabled_mod.enabled,
                version: enabled_mod.version.clone(),
                source_archive_hash: enabled_mod.source_archive_hash.clone(),
            })
            .collect()
    }
}

impl From<&EnabledMod> for DoctorProfileModSnapshot {
    fn from(value: &EnabledMod) -> Self {
        Self {
            mod_id: value.mod_id.clone(),
            display_name: value.display_name.clone(),
            enabled: value.enabled,
            version: value.version.clone(),
            source_archive_hash: value.source_archive_hash.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSnapshotRow {
    pub id: i64,
    pub snapshot: Vec<DoctorProfileModSnapshot>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DoctorProfileDiffKind {
    Added,
    Removed,
    Enabled,
    Disabled,
    VersionChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorProfileDiff {
    pub kind: DoctorProfileDiffKind,
    pub mod_id: String,
    pub display_name: Option<String>,
    pub previous_version: Option<String>,
    pub current_version: Option<String>,
    pub snapshot_id: i64,
    pub snapshot_created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorEvidence {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorContext {
    pub game_id: String,
    pub profile_name: String,
    pub crash: Option<CrashCorrelationReport>,
    pub mod_set: Vec<DoctorProfileModSnapshot>,
    pub active_plugins: Vec<DoctorPluginSummary>,
    pub installed_files: Vec<DoctorInstalledFileSummary>,
    pub tool_files: Vec<String>,
    pub diagnostics: Vec<DoctorDiagnosticSummary>,
    pub collisions: Vec<DoctorCollisionSummary>,
    pub recent_profile_diffs: Vec<DoctorProfileDiff>,
    pub evidence: Vec<DoctorEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorPluginSummary {
    pub plugin_name: String,
    pub sort_index: i64,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorInstalledFileSummary {
    pub mod_id: String,
    pub rel_path: String,
    pub origin_rel_path: String,
    pub size: u64,
    pub merge_group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorDiagnosticSummary {
    pub severity: String,
    pub title: String,
    pub detail: String,
    pub affected_mod: Option<String>,
    pub affected_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCollisionSummary {
    pub loser: String,
    pub winner: String,
    pub severity: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DoctorContextInput {
    pub game_id: String,
    pub profile: Profile,
    pub crash: Option<CrashCorrelationReport>,
    pub active_plugins: Vec<PluginEntry>,
    pub installed_files: Vec<(String, StagedFile)>,
    pub tool_files: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub collision_report: Option<CollisionReport>,
    pub recent_profile_diffs: Vec<DoctorProfileDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorExplanation {
    pub hypotheses: Vec<DoctorHypothesis>,
    #[serde(default)]
    pub unsupported: Vec<UnsupportedHypothesis>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorHypothesis {
    pub rank: usize,
    pub title: String,
    pub confidence: String,
    pub summary: String,
    pub evidence_ids: Vec<String>,
    pub recommended_checks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsupportedHypothesis {
    pub title: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorLlmProvider {
    Local,
    Remote,
}

#[derive(Debug, Clone)]
pub struct DoctorLlmRequest {
    pub provider: DoctorLlmProvider,
    pub context: DoctorContext,
}

#[must_use]
pub fn build_doctor_context(input: DoctorContextInput) -> DoctorContext {
    let mod_set = DoctorProfileModSnapshot::from_profile(&input.profile);
    let active_plugins = input
        .active_plugins
        .iter()
        .map(|plugin| DoctorPluginSummary {
            plugin_name: plugin.plugin_name.clone(),
            sort_index: plugin.sort_index,
            enabled: plugin.enabled,
        })
        .collect::<Vec<_>>();
    let installed_files = input
        .installed_files
        .iter()
        .map(|(mod_id, file)| DoctorInstalledFileSummary {
            mod_id: mod_id.clone(),
            rel_path: file.rel_path.clone(),
            origin_rel_path: file.origin_rel_path.clone(),
            size: file.size,
            merge_group: file.merge_group.clone(),
        })
        .collect::<Vec<_>>();
    let diagnostics = input
        .diagnostics
        .iter()
        .map(|diag| DoctorDiagnosticSummary {
            severity: match diag.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Info => "info",
            }
            .to_string(),
            title: diag.title.clone(),
            detail: diag.detail.clone(),
            affected_mod: diag.affected_mod.clone(),
            affected_file: diag
                .affected_file
                .as_ref()
                .map(|path| path.display().to_string()),
        })
        .collect::<Vec<_>>();
    let collisions = input
        .collision_report
        .as_ref()
        .map(collision_summaries)
        .unwrap_or_default();

    let mut ctx = DoctorContext {
        game_id: input.game_id,
        profile_name: input.profile.name,
        crash: input.crash,
        mod_set,
        active_plugins,
        installed_files,
        tool_files: input.tool_files,
        diagnostics,
        collisions,
        recent_profile_diffs: input.recent_profile_diffs,
        evidence: Vec::new(),
    };
    ctx.evidence = build_evidence(&ctx);
    ctx
}

#[must_use]
pub fn diff_profile_snapshots(
    current: &[DoctorProfileModSnapshot],
    previous: &ProfileSnapshotRow,
) -> Vec<DoctorProfileDiff> {
    let current_by_id = current
        .iter()
        .map(|m| (m.mod_id.as_str(), m))
        .collect::<BTreeMap<_, _>>();
    let previous_by_id = previous
        .snapshot
        .iter()
        .map(|m| (m.mod_id.as_str(), m))
        .collect::<BTreeMap<_, _>>();
    let ids = current_by_id
        .keys()
        .chain(previous_by_id.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let mut diffs = Vec::new();

    for id in ids {
        match (previous_by_id.get(id), current_by_id.get(id)) {
            (None, Some(current_mod)) => diffs.push(diff(
                DoctorProfileDiffKind::Added,
                current_mod,
                None,
                current_mod.version.clone(),
                previous,
            )),
            (Some(previous_mod), None) => diffs.push(diff(
                DoctorProfileDiffKind::Removed,
                previous_mod,
                previous_mod.version.clone(),
                None,
                previous,
            )),
            (Some(previous_mod), Some(current_mod)) => {
                if previous_mod.enabled != current_mod.enabled {
                    diffs.push(diff(
                        if current_mod.enabled {
                            DoctorProfileDiffKind::Enabled
                        } else {
                            DoctorProfileDiffKind::Disabled
                        },
                        current_mod,
                        previous_mod.version.clone(),
                        current_mod.version.clone(),
                        previous,
                    ));
                }
                if previous_mod.version != current_mod.version {
                    diffs.push(diff(
                        DoctorProfileDiffKind::VersionChanged,
                        current_mod,
                        previous_mod.version.clone(),
                        current_mod.version.clone(),
                        previous,
                    ));
                }
            }
            (None, None) => {}
        }
    }
    diffs
}

#[must_use]
pub fn validate_explanation(
    explanation: DoctorExplanation,
    evidence: &[DoctorEvidence],
) -> DoctorExplanation {
    let valid = evidence
        .iter()
        .map(|item| item.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut supported = Vec::new();
    let mut unsupported = explanation.unsupported;

    for hypothesis in explanation.hypotheses {
        let missing = hypothesis
            .evidence_ids
            .iter()
            .filter(|id| !valid.contains(id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if missing.is_empty() && !hypothesis.evidence_ids.is_empty() {
            supported.push(hypothesis);
        } else {
            unsupported.push(UnsupportedHypothesis {
                title: hypothesis.title,
                reason: if missing.is_empty() {
                    "hypothesis did not cite any modde evidence".to_string()
                } else {
                    format!(
                        "hypothesis cited unknown evidence IDs: {}",
                        missing.join(", ")
                    )
                },
            });
        }
    }

    DoctorExplanation {
        hypotheses: supported,
        unsupported,
    }
}

fn diff(
    kind: DoctorProfileDiffKind,
    source: &DoctorProfileModSnapshot,
    previous_version: Option<String>,
    current_version: Option<String>,
    row: &ProfileSnapshotRow,
) -> DoctorProfileDiff {
    DoctorProfileDiff {
        kind,
        mod_id: source.mod_id.clone(),
        display_name: source.display_name.clone(),
        previous_version,
        current_version,
        snapshot_id: row.id,
        snapshot_created_at: row.created_at.clone(),
    }
}
#[cfg(test)]
mod tests;
