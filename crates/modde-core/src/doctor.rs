use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::crash::{CrashCorrelationReport, CrashEvidence, CrashTokenKind};
use crate::diagnostics::{Diagnostic, Severity};
use crate::installer::StagedFile;
use crate::profile::{EnabledMod, Profile};
use crate::settings::DoctorLlmSettings;
use crate::{CollisionReport, CollisionSeverity, PluginEntry};

const MAX_EVIDENCE_STRING_CHARS: usize = 100;

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

pub async fn explain_with_openai_compatible_chat(
    settings: &DoctorLlmSettings,
    request: DoctorLlmRequest,
) -> Result<DoctorExplanation> {
    let (endpoint, model, api_key) = match request.provider {
        DoctorLlmProvider::Local => (
            settings.local_endpoint.clone(),
            settings.local_model.clone(),
            None,
        ),
        DoctorLlmProvider::Remote => {
            let endpoint = settings
                .remote_endpoint
                .clone()
                .context("doctor remote LLM endpoint is not configured")?;
            let model = settings
                .remote_model
                .clone()
                .context("doctor remote LLM model is not configured")?;
            let key = std::env::var(&settings.remote_api_key_env).with_context(|| {
                format!(
                    "doctor remote LLM API key env var {} is not set",
                    settings.remote_api_key_env
                )
            })?;
            (endpoint, model, Some(key))
        }
    };

    let context = trim_doctor_context(request.context, settings.max_context_bytes)?;
    let context_json = serde_json::to_string_pretty(&context)?;
    if context_json.len() > settings.max_context_bytes {
        bail!(
            "doctor context is {} bytes, above configured max_context_bytes {}",
            context_json.len(),
            settings.max_context_bytes
        );
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(settings.timeout_seconds.max(1)))
        .build()?;
    let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "temperature": 0.1,
        "messages": [
            {
                "role": "system",
                "content": "You are modde doctor. Return only JSON matching {\"hypotheses\":[{\"rank\":1,\"title\":\"...\",\"confidence\":\"low|medium|high\",\"summary\":\"...\",\"evidence_ids\":[\"EVIDENCE-ID\"],\"recommended_checks\":[\"...\"]}],\"unsupported\":[]}. Every hypothesis must cite evidence_ids from the provided context. Do not cite facts not present in the context."
            },
            {
                "role": "user",
                "content": context_json
            }
        ]
    });

    let mut req = client.post(url).json(&body);
    if let Some(key) = api_key {
        req = req.bearer_auth(key);
    }
    let resp = req.send().await?.error_for_status()?;
    let payload: ChatCompletionResponse = resp.json().await?;
    let content = payload
        .choices
        .first()
        .map(|choice| choice.message.content.as_str())
        .context("LLM response did not contain a first message")?;
    let parsed: DoctorExplanation =
        serde_json::from_str(content).context("LLM response content was not valid doctor JSON")?;
    Ok(validate_explanation(parsed, &context.evidence))
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

fn collision_summaries(report: &CollisionReport) -> Vec<DoctorCollisionSummary> {
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

fn build_evidence(ctx: &DoctorContext) -> Vec<DoctorEvidence> {
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

fn trim_doctor_context(mut context: DoctorContext, max_bytes: usize) -> Result<DoctorContext> {
    if context.evidence.is_empty() {
        context.evidence = build_evidence(&context);
    }
    if serde_json::to_string_pretty(&context)?.len() <= max_bytes {
        return Ok(context);
    }

    context.tool_files.clear();
    context.evidence = build_evidence(&context);
    if serde_json::to_string_pretty(&context)?.len() <= max_bytes {
        return Ok(context);
    }

    while !context.installed_files.is_empty()
        && serde_json::to_string_pretty(&context)?.len() > max_bytes
    {
        let keep = (context.installed_files.len() / 2).max(0);
        context.installed_files.truncate(keep);
        context.evidence = build_evidence(&context);
    }
    if serde_json::to_string_pretty(&context)?.len() <= max_bytes {
        return Ok(context);
    }

    context.collisions.clear();
    context.evidence = build_evidence(&context);
    if serde_json::to_string_pretty(&context)?.len() <= max_bytes {
        return Ok(context);
    }

    context.evidence.retain(|e| {
        matches!(
            e.kind.as_str(),
            "crash-suspect" | "profile-diff" | "diagnostic"
        )
    });
    let size = serde_json::to_string_pretty(&context)?.len();
    if size > max_bytes {
        bail!(
            "doctor context is {size} bytes after priority trimming, above configured max_context_bytes {max_bytes}"
        );
    }
    Ok(context)
}

fn cap_evidence_string(value: &str) -> String {
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

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn profile_snapshot_diff_detects_common_changes() {
        let previous = ProfileSnapshotRow {
            id: 7,
            created_at: "2026-06-11 10:00:00".into(),
            snapshot: vec![
                DoctorProfileModSnapshot {
                    mod_id: "a".into(),
                    display_name: None,
                    version: Some("1".into()),
                    source_archive_hash: None,
                    enabled: true,
                },
                DoctorProfileModSnapshot {
                    mod_id: "b".into(),
                    display_name: None,
                    version: Some("1".into()),
                    source_archive_hash: None,
                    enabled: true,
                },
            ],
        };
        let current = vec![
            DoctorProfileModSnapshot {
                mod_id: "a".into(),
                display_name: None,
                version: Some("2".into()),
                source_archive_hash: None,
                enabled: false,
            },
            DoctorProfileModSnapshot {
                mod_id: "c".into(),
                display_name: None,
                version: None,
                source_archive_hash: None,
                enabled: true,
            },
        ];

        let kinds = diff_profile_snapshots(&current, &previous)
            .into_iter()
            .map(|diff| diff.kind)
            .collect::<Vec<_>>();
        assert!(kinds.contains(&DoctorProfileDiffKind::VersionChanged));
        assert!(kinds.contains(&DoctorProfileDiffKind::Disabled));
        assert!(kinds.contains(&DoctorProfileDiffKind::Removed));
        assert!(kinds.contains(&DoctorProfileDiffKind::Added));
    }

    #[test]
    fn citation_validation_rejects_unsupported_hypotheses() {
        let evidence = vec![DoctorEvidence {
            id: "MOD-0001".into(),
            kind: "mod".into(),
            title: "A".into(),
            detail: "detail".into(),
        }];
        let explanation = DoctorExplanation {
            hypotheses: vec![
                DoctorHypothesis {
                    rank: 1,
                    title: "ok".into(),
                    confidence: "high".into(),
                    summary: "summary".into(),
                    evidence_ids: vec!["MOD-0001".into()],
                    recommended_checks: vec![],
                },
                DoctorHypothesis {
                    rank: 2,
                    title: "bad".into(),
                    confidence: "high".into(),
                    summary: "summary".into(),
                    evidence_ids: vec!["NOPE".into()],
                    recommended_checks: vec![],
                },
            ],
            unsupported: vec![],
        };
        let validated = validate_explanation(explanation, &evidence);
        assert_eq!(validated.hypotheses.len(), 1);
        assert_eq!(validated.unsupported.len(), 1);
    }

    #[tokio::test]
    async fn openai_compatible_client_supports_local_and_remote() {
        let server = MockServer::start().await;
        let response = ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "content": "{\"hypotheses\":[{\"rank\":1,\"title\":\"Grounded\",\"confidence\":\"high\",\"summary\":\"Uses provided evidence\",\"evidence_ids\":[\"MOD-0001\"],\"recommended_checks\":[\"Disable test mod\"]}],\"unsupported\":[]}"
                }
            }]
        }));
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(response.clone())
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/remote/chat/completions"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;

        let context = DoctorContext {
            game_id: "skyrim-se".into(),
            profile_name: "test".into(),
            crash: None,
            mod_set: vec![],
            active_plugins: vec![],
            installed_files: vec![],
            tool_files: vec![],
            diagnostics: vec![],
            collisions: vec![],
            recent_profile_diffs: vec![],
            evidence: vec![DoctorEvidence {
                id: "MOD-0001".into(),
                kind: "mod".into(),
                title: "Test Mod".into(),
                detail: "enabled".into(),
            }],
        };
        let settings = DoctorLlmSettings {
            local_endpoint: format!("{}/v1", server.uri()),
            local_model: "local".into(),
            remote_endpoint: Some(format!("{}/remote", server.uri())),
            remote_model: Some("remote".into()),
            remote_api_key_env: "MODDE_TEST_DOCTOR_REMOTE_KEY".into(),
            timeout_seconds: 5,
            max_context_bytes: 64 * 1024,
        };

        let local = explain_with_openai_compatible_chat(
            &settings,
            DoctorLlmRequest {
                provider: DoctorLlmProvider::Local,
                context: context.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(local.hypotheses.len(), 1);

        // SAFETY: this test uses a unique env var name and no other test in
        // this crate reads it.
        unsafe {
            std::env::set_var("MODDE_TEST_DOCTOR_REMOTE_KEY", "secret");
        }
        let remote = explain_with_openai_compatible_chat(
            &settings,
            DoctorLlmRequest {
                provider: DoctorLlmProvider::Remote,
                context,
            },
        )
        .await
        .unwrap();
        assert_eq!(remote.hypotheses.len(), 1);
        // SAFETY: this cleans up the unique env var set above.
        unsafe {
            std::env::remove_var("MODDE_TEST_DOCTOR_REMOTE_KEY");
        }
    }
}
