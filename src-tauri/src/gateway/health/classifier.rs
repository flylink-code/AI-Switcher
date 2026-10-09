// HTTP error classifier and retry-after parser for upstream health.
// Included into gateway::health module via include!("health/classifier.rs").

const COOLDOWN_AUTH_SECS: u64 = 30 * 60; // 30 minutes
const COOLDOWN_QUOTA_SECS: u64 = 60 * 60; // 1 hour
const COOLDOWN_MODEL_SECS: u64 = 60 * 60; // 1 hour
const COOLDOWN_RATE_LIMIT_BASE_SECS: u64 = 5;
const COOLDOWN_RATE_LIMIT_MAX_SECS: u64 = 300; // 5 minutes
const TRANSIENT_FAILURE_THRESHOLD: u32 = 2;

const SAFE_ERR_AUTH_FAILED: &str = "身份验证失败 (HTTP 401/403)";
const SAFE_ERR_QUOTA_EXHAUSTED: &str = "额度不足或已用尽";
const SAFE_ERR_RATE_LIMITED: &str = "请求频次超限 (HTTP 429)";
const SAFE_ERR_MODEL_NOT_FOUND: &str = "模型不支持或不存在";
const SAFE_ERR_TRANSIENT: &str = "上游服务暂时不可用 (5xx)";

fn parse_retry_after(value: &str, now: chrono::DateTime<chrono::Utc>) -> Option<u64> {
    let trimmed = value.trim();
    if let Ok(secs) = trimmed.parse::<u64>() {
        return Some(secs);
    }
    if let Ok(secs_f) = trimmed.parse::<f64>() {
        if secs_f >= 0.0 && secs_f.is_finite() {
            return Some(secs_f.ceil() as u64);
        }
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(trimmed) {
        let diff_ms = dt.signed_duration_since(now).num_milliseconds();
        if diff_ms <= 0 {
            return Some(0);
        }
        let diff_secs = ((diff_ms as f64) / 1000.0).ceil() as u64;
        return Some(diff_secs);
    }
    for fmt in &[
        "%a, %d %b %Y %H:%M:%S GMT",
        "%a, %d %b %Y %H:%M:%S %z",
        "%A, %d-%b-%y %H:%M:%S GMT",
        "%c",
    ] {
        if let Ok(dt) = chrono::DateTime::parse_from_str(trimmed, fmt) {
            let diff_ms = dt.signed_duration_since(now).num_milliseconds();
            if diff_ms <= 0 {
                return Some(0);
            }
            let diff_secs = ((diff_ms as f64) / 1000.0).ceil() as u64;
            return Some(diff_secs);
        }
        if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(trimmed, fmt) {
            let dt = ndt.and_utc();
            let diff_ms = dt.signed_duration_since(now).num_milliseconds();
            if diff_ms <= 0 {
                return Some(0);
            }
            let diff_secs = ((diff_ms as f64) / 1000.0).ceil() as u64;
            return Some(diff_secs);
        }
    }
    None
}

fn calculate_rate_limit_backoff(consecutive_rate_limits: u32) -> Duration {
    let shift = consecutive_rate_limits.saturating_sub(1).min(6);
    let secs = (COOLDOWN_RATE_LIMIT_BASE_SECS.saturating_mul(1 << shift))
        .min(COOLDOWN_RATE_LIMIT_MAX_SECS);
    Duration::from_secs(secs)
}

fn extract_error_fields(
    body: &str,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    if body.is_empty() {
        return (None, None, None, None);
    }
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(body) {
        let err = val.get("error").unwrap_or(&val);
        let code = err.get("code").and_then(|c| {
            if let Some(s) = c.as_str() {
                Some(s.to_string())
            } else {
                c.as_i64().map(|n| n.to_string())
            }
        });
        let err_type = err
            .get("type")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                val.get("type")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string())
            });
        let err_status = err
            .get("status")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                err.get("msg")
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
            })
            .or_else(|| {
                val.get("message")
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
            });
        return (code, err_type, err_status, msg);
    }
    (None, None, None, None)
}

fn matches_quota_phrase(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("insufficient_quota")
        || lower.contains("insufficient quota")
        || lower.contains("quota exceeded")
        || lower.contains("exceeded your current quota")
        || lower.contains("exceeded quota")
        || lower.contains("out of credit")
        || lower.contains("run out of credit")
        || lower.contains("lack of balance")
        || lower.contains("balance is insufficient")
        || lower.contains("insufficient balance")
        || lower.contains("account overdue")
        || text.contains("余额不足")
        || text.contains("额度不足")
        || text.contains("额度已用尽")
        || text.contains("已欠费")
        || text.contains("账户欠费")
        || text.contains("无可用额度")
}

fn is_quota_error(
    status: u16,
    code: Option<&str>,
    err_type: Option<&str>,
    err_status: Option<&str>,
    msg: Option<&str>,
    body: &str,
) -> bool {
    if let Some(c) = code {
        let c_lower = c.to_ascii_lowercase();
        if c_lower == "insufficient_quota"
            || c_lower == "quota_exceeded"
            || c_lower == "exceeded_quota"
            || c_lower == "insufficient_user_quota"
            || c_lower == "out_of_credit"
            || c_lower == "balance_exhausted"
            || c_lower == "account_overdue"
            || c_lower == "credit_expired"
        {
            return true;
        }
    }
    if let Some(t) = err_type {
        let t_lower = t.to_ascii_lowercase();
        if t_lower == "insufficient_quota" || t_lower == "quota_exceeded" {
            return true;
        }
    }
    if err_status == Some("RESOURCE_EXHAUSTED") {
        if let Some(m) = msg {
            if matches_quota_phrase(m) {
                return true;
            }
        }
    }
    if status == 402 || status == 403 || status == 429 || status == 400 {
        if let Some(m) = msg {
            if matches_quota_phrase(m) {
                return true;
            }
        }
        if matches_quota_phrase(body) {
            return true;
        }
    }
    false
}

fn is_auth_error(code: Option<&str>, err_type: Option<&str>, msg: Option<&str>) -> bool {
    if let Some(c) = code {
        let c_lower = c.to_ascii_lowercase();
        if c_lower == "invalid_api_key"
            || c_lower == "invalid_token"
            || c_lower == "unauthorized"
            || c_lower == "authentication_error"
            || c_lower == "account_deactivated"
            || c_lower == "revoked"
        {
            return true;
        }
    }
    if let Some(t) = err_type {
        let t_lower = t.to_ascii_lowercase();
        if t_lower == "authentication_error" || t_lower == "permission_error" {
            return true;
        }
    }
    if let Some(m) = msg {
        let lower = m.to_ascii_lowercase();
        if lower.contains("invalid api key")
            || lower.contains("incorrect api key")
            || lower.contains("authentication failed")
            || lower.contains("token expired")
            || m.contains("密钥无效")
            || m.contains("凭据无效")
        {
            return true;
        }
    }
    false
}

fn is_rate_limit_error(code: Option<&str>, err_type: Option<&str>, msg: Option<&str>) -> bool {
    if let Some(c) = code {
        let c_lower = c.to_ascii_lowercase();
        if c_lower == "rate_limit_exceeded"
            || c_lower == "rate_limit_error"
            || c_lower == "requests"
            || c_lower == "tokens"
        {
            return true;
        }
    }
    if let Some(t) = err_type {
        let t_lower = t.to_ascii_lowercase();
        if t_lower == "rate_limit_error" || t_lower == "overloaded_error" {
            return true;
        }
    }
    if let Some(m) = msg {
        let lower = m.to_ascii_lowercase();
        if lower.contains("rate limit")
            || lower.contains("too many requests")
            || lower.contains("overloaded")
            || m.contains("频次超限")
            || m.contains("请求过于频繁")
        {
            return true;
        }
    }
    false
}

fn matches_model_not_found(code: Option<&str>, message: Option<&str>, body: &str) -> bool {
    if let Some(c) = code {
        let c_lower = c.to_ascii_lowercase();
        if c_lower == "model_not_found"
            || c_lower == "invalid_model"
            || c_lower == "model_does_not_exist"
            || c_lower == "unknown_model"
            || c_lower == "unsupported_model"
        {
            return true;
        }
    }

    let check_text = |text: &str| -> bool {
        if text.contains("模型不存在")
            || text.contains("不支持的模型")
            || text.contains("模型未找到")
            || text.contains("找不到该模型")
            || text.contains("模型不可用")
        {
            return true;
        }
        let lower = text.to_ascii_lowercase();
        if lower.contains("model_not_found")
            || lower.contains("model not found")
            || lower.contains("unknown model")
            || lower.contains("unsupported model")
            || lower.contains("invalid model")
            || lower.contains("does not have access to model")
            || lower.contains("no such model")
        {
            return true;
        }
        if lower.contains("model")
            && (lower.contains("does not exist")
                || lower.contains("not found")
                || lower.contains("could not be found"))
        {
            if !lower.contains("route not found") && !lower.contains("endpoint not found") {
                return true;
            }
        }
        false
    };

    if let Some(msg) = message {
        if check_text(msg) {
            return true;
        }
    }
    check_text(body)
}

pub fn classify_failure(
    status: u16,
    headers: Option<&http::HeaderMap>,
    body: Option<&str>,
) -> FailureClassification {
    let body_str = body.unwrap_or("").trim();
    let (code, err_type, err_status, msg) = extract_error_fields(body_str);

    // 1. Quota check (402 or explicit quota error)
    if status == 402
        || is_quota_error(
            status,
            code.as_deref(),
            err_type.as_deref(),
            err_status.as_deref(),
            msg.as_deref(),
            body_str,
        )
    {
        return FailureClassification {
            kind: FailureKind::Quota,
            cooldown: Some(Duration::from_secs(COOLDOWN_QUOTA_SECS)),
            safe_description: Some(SAFE_ERR_QUOTA_EXHAUSTED.to_string()),
        };
    }

    // 2. Auth check (401 or 403 not quota)
    if status == 401
        || status == 403
        || is_auth_error(code.as_deref(), err_type.as_deref(), msg.as_deref())
    {
        return FailureClassification {
            kind: FailureKind::Auth,
            cooldown: Some(Duration::from_secs(COOLDOWN_AUTH_SECS)),
            safe_description: Some(SAFE_ERR_AUTH_FAILED.to_string()),
        };
    }

    // 3. RateLimit check (429 or 529 overloaded)
    if status == 429
        || status == 529
        || is_rate_limit_error(code.as_deref(), err_type.as_deref(), msg.as_deref())
    {
        // Server Retry-After takes precedence directly (do NOT truncate 3600s to 300s)
        let retry_cooldown = headers
            .and_then(|h| {
                h.get("retry-after")
                    .and_then(|val| val.to_str().ok())
                    .and_then(|val| parse_retry_after(val, chrono::Utc::now()))
            })
            .map(Duration::from_secs);

        return FailureClassification {
            kind: FailureKind::RateLimit,
            cooldown: retry_cooldown,
            safe_description: Some(SAFE_ERR_RATE_LIMITED.to_string()),
        };
    }

    // 4. Model unsupported check (404/400 only when explicitly model not found)
    if status == 404 || status == 400 {
        if matches_model_not_found(code.as_deref(), msg.as_deref(), body_str) {
            return FailureClassification {
                kind: FailureKind::ModelUnsupported,
                cooldown: Some(Duration::from_secs(COOLDOWN_MODEL_SECS)),
                safe_description: Some(SAFE_ERR_MODEL_NOT_FOUND.to_string()),
            };
        }
        return FailureClassification {
            kind: FailureKind::ClientErrorIgnored,
            cooldown: None,
            safe_description: None,
        };
    }

    // 5. Transient server errors (5xx)
    if status >= 500 {
        return FailureClassification {
            kind: FailureKind::Transient,
            cooldown: Some(Duration::from_secs(CIRCUIT_OPEN_SECONDS)),
            safe_description: Some(SAFE_ERR_TRANSIENT.to_string()),
        };
    }

    // 6. Other client errors (405, 422, etc.)
    FailureClassification {
        kind: FailureKind::ClientErrorIgnored,
        cooldown: None,
        safe_description: None,
    }
}
