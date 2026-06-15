use super::{DoctorCollisionSummary, DoctorContext, DoctorEvidence};
use crate::crash::{CrashEvidence, CrashTokenKind};
use crate::{CollisionReport, CollisionSeverity};

pub(in crate::doctor) const MAX_EVIDENCE_STRING_CHARS: usize = 100;

pub(in crate::doctor) fn collision_summaries(
    report: &CollisionReport,
) -> Vec<DoctorCollisionSummary> {
    report
        .pairs
        .iter()
        .take(50)
        .map(|pair| DoctorCollisionSummary {
            loser: pair.loser.to_string(),
            winner: pair.winner.to_string(),
            severity: severity_label(pair.max_severity).to_string(),
            files: pair
                .files
                .iter()
                .take(20)
                .map(|file| file.file_path.clone())
                .collect(),
        })
        .collect()
}

fn severity_label(severity: CollisionSeverity) -> &'static str {
    match severity {
        CollisionSeverity::Cosmetic => "cosmetic",
        CollisionSeverity::Config => "config",
        CollisionSeverity::Dangerous => "dangerous",
        CollisionSeverity::Unknown => "unknown",
    }
}

pub(in crate::doctor) fn build_evidence(ctx: &DoctorContext) -> Vec<DoctorEvidence> {
    let mut evidence = Vec::new();
    for (idx, item) in ctx.mod_set.iter().enumerate() {
        evidence.push(DoctorEvidence {
            id: format!("MOD-{:04}", idx + 1),
            kind: "mod".to_string(),
            title: cap_evidence_string(item.display_name.as_deref().unwrap_or(&item.mod_id)),
            detail: format!(
                "mod_id={}, version={}, enabled={}",
                cap_evidence_string(&item.mod_id),
                cap_evidence_string(item.version.as_deref().unwrap_or("<none>")),
                item.enabled
            ),
        });
    }
    for (idx, plugin) in ctx.active_plugins.iter().enumerate() {
        evidence.push(DoctorEvidence {
            id: format!("PLUGIN-{:04}", idx + 1),
            kind: "plugin".to_string(),
            title: cap_evidence_string(&plugin.plugin_name),
            detail: format!(
                "load_order={}, enabled={}",
                plugin.sort_index, plugin.enabled
            ),
        });
    }
    for (idx, file) in ctx.installed_files.iter().take(200).enumerate() {
        evidence.push(DoctorEvidence {
            id: format!("FILE-{:04}", idx + 1),
            kind: "installed-file".to_string(),
            title: cap_evidence_string(&file.rel_path),
            detail: format!(
                "owned by {} ({} bytes)",
                cap_evidence_string(&file.mod_id),
                file.size
            ),
        });
    }
    for (idx, file) in ctx.tool_files.iter().take(100).enumerate() {
        evidence.push(DoctorEvidence {
            id: format!("TOOL-{:04}", idx + 1),
            kind: "tool-file".to_string(),
            title: cap_evidence_string(file),
            detail: "tracked as a tool-applied file".to_string(),
        });
    }
    for (idx, diag) in ctx.diagnostics.iter().enumerate() {
        evidence.push(DoctorEvidence {
            id: format!("DIAG-{:04}", idx + 1),
            kind: "diagnostic".to_string(),
            title: cap_evidence_string(&diag.title),
            detail: cap_evidence_string(&format!("{}: {}", diag.severity, diag.detail)),
        });
    }
    for (idx, collision) in ctx.collisions.iter().enumerate() {
        evidence.push(DoctorEvidence {
            id: format!("COLLISION-{:04}", idx + 1),
            kind: "collision".to_string(),
            title: cap_evidence_string(&format!("{} vs {}", collision.loser, collision.winner)),
            detail: cap_evidence_string(&format!(
                "{} collision over {} file(s): {}",
                collision.severity,
                collision.files.len(),
                collision.files.join(", ")
            )),
        });
    }
    for (idx, diff) in ctx.recent_profile_diffs.iter().enumerate() {
        evidence.push(DoctorEvidence {
            id: format!("DIFF-{:04}", idx + 1),
            kind: "profile-diff".to_string(),
            title: cap_evidence_string(&format!("{:?}: {}", diff.kind, diff.mod_id)),
            detail: cap_evidence_string(&format!(
                "previous_version={}, current_version={}, snapshot={}",
                diff.previous_version.as_deref().unwrap_or("<none>"),
                diff.current_version.as_deref().unwrap_or("<none>"),
                diff.snapshot_created_at
            )),
        });
    }
    if let Some(crash) = &ctx.crash {
        for (idx, suspect) in crash.suspects.iter().enumerate() {
            evidence.push(DoctorEvidence {
                id: format!("CRASH-{:04}", idx + 1),
                kind: "crash-suspect".to_string(),
                title: cap_evidence_string(&suspect.summary),
                detail: cap_evidence_string(
                    &suspect
                        .evidence
                        .iter()
                        .map(format_crash_evidence)
                        .collect::<Vec<_>>()
                        .join("; "),
                ),
            });
        }
    }
    evidence
}

pub(in crate::doctor) fn cap_evidence_string(value: &str) -> String {
    let mut out = value
        .chars()
        .take(MAX_EVIDENCE_STRING_CHARS)
        .collect::<String>();
    if value.chars().count() > MAX_EVIDENCE_STRING_CHARS {
        out.push_str("...");
    }
    out
}

fn format_crash_evidence(evidence: &CrashEvidence) -> String {
    let kind = match evidence.kind {
        CrashTokenKind::Plugin => "plugin",
        CrashTokenKind::Dll => "dll",
        CrashTokenKind::AssetPath => "asset",
        CrashTokenKind::FormId => "form-id",
    };
    format!(
        "line {} {} {} in {} ({})",
        evidence.line, kind, evidence.token, evidence.section, evidence.reason
    )
}
