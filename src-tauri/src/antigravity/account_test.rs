//! Pin one Antigravity account and send a tiny Cloud Code chat probe.
//!
//! Does not go through the local :15830 pool, does not rotate accounts, and
//! does not write usage logs. Used by the reverse-proxy account card to tell
//! auth-dead from rate-limited from still-chatty.

use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::antigravity::account::{is_auth_failure, store as account_store, REAUTH_REASON};
use crate::antigravity::map::anthropic::anthropic_to_gemini_request;
use crate::antigravity::map::models::map_model_id;
use crate::antigravity::model_catalog;
use crate::antigravity::upstream::{unwrap_v1internal, wrap_v1internal, UpstreamClient};
use crate::error::{AppError, AppResult};

pub const DEFAULT_PROMPT: &str = "hello";
const MAX_REPLY_CHARS: usize = 500;
const MAX_ERROR_CHARS: usize = 400;
const MAX_TOKENS: u64 = 64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityAccountTestResult {
    pub ok: bool,
    pub category: String,
    pub status: Option<u16>,
    pub model: String,
    pub latency_ms: u64,
    pub reply: Option<String>,
    pub error: Option<String>,
}

pub fn resolve_probe_model(requested: Option<&str>) -> String {
    let trimmed = requested.map(str::trim).unwrap_or("");
    if trimmed.is_empty() {
        model_catalog::preferred_gemini_flash().unwrap_or_else(|| "gemini-3.8-flash-high".into())
    } else {
        map_model_id(trimmed)
    }
}

pub fn resolve_probe_prompt(prompt: Option<&str>) -> String {
    let trimmed = prompt.map(str::trim).unwrap_or("");
    if trimmed.is_empty() {
        DEFAULT_PROMPT.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Build the Gemini-style inner request for a hello probe.
pub fn build_probe_gemini_request(model: &str, prompt: &str) -> Result<(String, Value), String> {
    let body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "stream": false,
        "thinking": { "type": "disabled" },
        "messages": [{ "role": "user", "content": prompt }],
    });
    let parts = anthropic_to_gemini_request(&body, None, None)?;
    let mut request = parts.request;
    sanitize_probe_generation(&parts.model, &mut request);
    Ok((parts.model, request))
}

fn sanitize_probe_generation(model: &str, request: &mut Value) {
    let Some(budget) = model_catalog::thinking_budget_for(model) else {
        return;
    };
    let generation = request.as_object_mut().map(|root| {
        root.entry("generationConfig".to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()))
    });
    let Some(generation) = generation else {
        return;
    };
    if let Some(obj) = generation.as_object_mut() {
        obj.remove("temperature");
        obj.remove("topP");
    }
    let max = generation.get("maxOutputTokens").and_then(Value::as_u64);
    generation["maxOutputTokens"] =
        json!(crate::antigravity::thinking::pad_max_tokens(max, budget));
    crate::antigravity::thinking::apply_thinking_budget(generation, budget, true);
}

pub fn extract_reply_text(unwrapped: &Value) -> Option<String> {
    let parts = unwrapped
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)?;
    let mut texts = Vec::new();
    for part in parts {
        if part.get("thought").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                texts.push(trimmed);
            }
        }
    }
    if texts.is_empty() {
        None
    } else {
        Some(truncate(&texts.join("\n"), MAX_REPLY_CHARS))
    }
}

pub fn classify_probe_outcome(status: Option<u16>, body: &str, network: bool) -> &'static str {
    if network {
        return "network";
    }
    if is_auth_failure(body) || status == Some(401) {
        return "auth";
    }
    let lower = body.to_ascii_lowercase();
    if status == Some(429)
        || lower.contains("resource_exhausted")
        || lower.contains("resource exhausted")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
    {
        return "rate_limit";
    }
    if status == Some(403)
        && (lower.contains("forbidden")
            || lower.contains("quota")
            || lower.contains("service_disabled")
            || lower.contains("permission_denied")
            || lower.contains("access_denied"))
    {
        return "quota";
    }
    if status.is_some_and(|code| (200..300).contains(&code)) {
        return "ok";
    }
    "error"
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let clipped: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{clipped}…")
    } else {
        clipped
    }
}

fn result_for(
    ok: bool,
    category: &str,
    status: Option<u16>,
    model: String,
    latency_ms: u64,
    reply: Option<String>,
    error: Option<String>,
) -> AntigravityAccountTestResult {
    AntigravityAccountTestResult {
        ok,
        category: category.to_string(),
        status,
        model,
        latency_ms,
        reply,
        error: error.map(|text| truncate(&text, MAX_ERROR_CHARS)),
    }
}

async fn load_probe_token(
    account_id: &str,
) -> AppResult<(String, crate::antigravity::account::AntigravityAccount)> {
    let id = account_id.to_string();
    tokio::task::spawn_blocking(move || account_store().ensure_access_token_for_probe(&id))
        .await
        .map_err(|error| AppError::Other(format!("账号探测任务失败: {error}")))?
}

async fn refresh_probe_token(
    account_id: &str,
) -> AppResult<(String, crate::antigravity::account::AntigravityAccount)> {
    let id = account_id.to_string();
    tokio::task::spawn_blocking(move || account_store().force_refresh_access_token(&id))
        .await
        .map_err(|error| AppError::Other(format!("Token 刷新任务失败: {error}")))?
}

pub async fn test_account(
    account_id: &str,
    model: Option<String>,
    prompt: Option<String>,
) -> AppResult<AntigravityAccountTestResult> {
    let started = Instant::now();
    let mapped = resolve_probe_model(model.as_deref());
    let prompt = resolve_probe_prompt(prompt.as_deref());
    let (upstream_model, request) = match build_probe_gemini_request(&mapped, &prompt) {
        Ok(value) => value,
        Err(error) => {
            return Ok(result_for(
                false,
                "error",
                None,
                mapped,
                started.elapsed().as_millis() as u64,
                None,
                Some(error),
            ));
        }
    };

    let (mut access_token, account) = match load_probe_token(account_id).await {
        Ok(value) => value,
        Err(error) => {
            let message = error.to_string();
            let category = if matches!(error, AppError::Network(_)) {
                "network"
            } else if is_auth_failure(&message) {
                "auth"
            } else {
                "error"
            };
            if category == "auth" && crate::antigravity::account::requires_reauthorization(&message)
            {
                let _ = account_store().mark_reauthorization_required(account_id, REAUTH_REASON);
            }
            return Ok(result_for(
                false,
                category,
                if category == "auth" { Some(401) } else { None },
                upstream_model,
                started.elapsed().as_millis() as u64,
                None,
                Some(message),
            ));
        }
    };

    let project_id = match account
        .token
        .project_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
    {
        Some(id) => id.to_string(),
        None => {
            let upstream = UpstreamClient::new();
            match upstream.fetch_project_id(&access_token).await {
                Ok(id) => {
                    let _ = account_store().update_project_id(account_id, &id);
                    id
                }
                Err(error) => {
                    let message = error.to_string();
                    let category = if matches!(error, AppError::Network(_)) {
                        "network"
                    } else if is_auth_failure(&message) || message.contains("401") {
                        "auth"
                    } else {
                        "error"
                    };
                    return Ok(result_for(
                        false,
                        category,
                        None,
                        upstream_model,
                        started.elapsed().as_millis() as u64,
                        None,
                        Some(message),
                    ));
                }
            }
        }
    };

    let wrapped = wrap_v1internal(&project_id, &upstream_model, request);
    let upstream = UpstreamClient::new();
    let mut refreshed = false;
    loop {
        match upstream.generate(&access_token, &wrapped, false).await {
            Ok(response) => {
                let status = response.status().as_u16();
                if status == 401 && !refreshed {
                    refreshed = true;
                    log::info!(
                        "Antigravity chat probe 401 for {}; forcing token renewal and retrying",
                        account.email
                    );
                    match refresh_probe_token(account_id).await {
                        Ok((token, _)) => {
                            access_token = token;
                            continue;
                        }
                        Err(refresh_error) => {
                            let message = refresh_error.to_string();
                            if crate::antigravity::account::requires_reauthorization(&message) {
                                let _ = account_store()
                                    .mark_reauthorization_required(account_id, REAUTH_REASON);
                            }
                            return Ok(result_for(
                                false,
                                "auth",
                                Some(401),
                                upstream_model,
                                started.elapsed().as_millis() as u64,
                                None,
                                Some(message),
                            ));
                        }
                    }
                }
                let body = response.text().await.unwrap_or_default();
                let category = classify_probe_outcome(Some(status), &body, false);
                let reply = if category == "ok" {
                    serde_json::from_str::<Value>(&body)
                        .ok()
                        .and_then(|value| extract_reply_text(&unwrap_v1internal(&value)))
                } else {
                    None
                };
                return Ok(result_for(
                    category == "ok",
                    category,
                    Some(status),
                    upstream_model,
                    started.elapsed().as_millis() as u64,
                    reply,
                    if category == "ok" { None } else { Some(body) },
                ));
            }
            Err(AppError::Network(message)) => {
                return Ok(result_for(
                    false,
                    "network",
                    None,
                    upstream_model,
                    started.elapsed().as_millis() as u64,
                    None,
                    Some(message),
                ));
            }
            Err(error) => {
                let message = error.to_string();
                let category = classify_probe_outcome(None, &message, false);
                if category == "auth"
                    && crate::antigravity::account::requires_reauthorization(&message)
                {
                    let _ =
                        account_store().mark_reauthorization_required(account_id, REAUTH_REASON);
                }
                return Ok(result_for(
                    false,
                    category,
                    None,
                    upstream_model,
                    started.elapsed().as_millis() as u64,
                    None,
                    Some(message),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_model_and_prompt_use_defaults() {
        assert_eq!(resolve_probe_prompt(None), "hello");
        assert_eq!(resolve_probe_prompt(Some("  ")), "hello");
        assert_eq!(resolve_probe_prompt(Some("ping")), "ping");
        let model = resolve_probe_model(None);
        assert!(
            model.contains("gemini") && model.contains("flash"),
            "default probe model should be Gemini Flash, got {model}"
        );
    }

    #[test]
    fn probe_request_contains_hello() {
        let (model, request) =
            build_probe_gemini_request("gemini-3.8-flash-high", "hello").expect("build");
        assert_eq!(model, "gemini-3.8-flash-high");
        let text = request
            .pointer("/contents/0/parts/0/text")
            .and_then(Value::as_str);
        assert_eq!(text, Some("hello"));
        assert_eq!(
            request
                .pointer("/generationConfig/maxOutputTokens")
                .and_then(Value::as_u64),
            Some(64)
        );
    }

    #[test]
    fn gemini_31_pro_probe_uses_thinking_budget_not_level() {
        let (model, request) =
            build_probe_gemini_request("gemini-3.1-pro-high", "hello").expect("build");
        assert_eq!(model, "gemini-3.1-pro-high");
        let gen = request.get("generationConfig").expect("generationConfig");
        assert!(gen.get("temperature").is_none());
        assert!(gen.get("topP").is_none());
        let thinking = gen.get("thinkingConfig").expect("thinkingConfig");
        assert_eq!(
            thinking.get("thinkingBudget").and_then(Value::as_u64),
            Some(10001)
        );
        assert!(thinking.get("thinkingLevel").is_none());
        assert_eq!(
            gen.get("maxOutputTokens").and_then(Value::as_u64),
            Some(10002)
        );
    }

    #[test]
    fn classify_auth_rate_limit_and_network() {
        assert_eq!(
            classify_probe_outcome(Some(401), "unauthenticated", false),
            "auth"
        );
        assert_eq!(
            classify_probe_outcome(None, "auth/invalid_grant: revoked", false),
            "auth"
        );
        assert_eq!(
            classify_probe_outcome(Some(429), "RESOURCE_EXHAUSTED", false),
            "rate_limit"
        );
        assert_eq!(
            classify_probe_outcome(Some(403), "quota forbidden", false),
            "quota"
        );
        assert_eq!(classify_probe_outcome(None, "timeout", true), "network");
        assert_eq!(classify_probe_outcome(Some(200), "", false), "ok");
        assert_eq!(
            classify_probe_outcome(Some(500), "backend unavailable", false),
            "error"
        );
    }

    #[test]
    fn extract_reply_skips_thoughts_and_truncates() {
        let value = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        { "text": "thinking", "thought": true },
                        { "text": "  hi there  " }
                    ]
                }
            }]
        });
        assert_eq!(extract_reply_text(&value).as_deref(), Some("hi there"));
    }
}
