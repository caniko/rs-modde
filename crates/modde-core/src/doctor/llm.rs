use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::evidence::build_evidence;
use super::{
    DoctorContext, DoctorExplanation, DoctorLlmProvider, DoctorLlmRequest, validate_explanation,
};
use crate::settings::DoctorLlmSettings;

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

pub(in crate::doctor) fn trim_doctor_context(
    mut context: DoctorContext,
    max_bytes: usize,
) -> Result<DoctorContext> {
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
        let keep = context.installed_files.len() / 2;
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
