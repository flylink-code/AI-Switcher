// Unit tests for upstream health and circuit breaking.
// Included into gateway::health module via #[cfg(test)] include!("health/tests.rs").

use std::sync::atomic::{AtomicU64, Ordering};

static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

fn unique_id(suffix: &str) -> String {
    format!(
        "up_health_{suffix}_{}_{}",
        std::process::id(),
        TEST_SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

#[test]
fn probe_failure_does_not_open_circuit() {
    let id = unique_id("probe");
    record_probe_failure(&id);
    record_probe_failure(&id);
    let row = lookup(&id).expect("row");
    assert_eq!(row.status, "failing");
    assert_eq!(row.consecutive_failures, 2);
    assert_eq!(row.cooldown_remaining_ms, 0);
    assert_eq!(rank_for_failover(&id).0, 1);
}

#[test]
fn live_failure_opens_circuit_after_two_hits() {
    let id = unique_id("live");
    record_failure(&id);
    record_failure(&id);
    let row = lookup(&id).expect("row");
    assert_eq!(row.status, "cooling");
    assert!(row.cooldown_remaining_ms > 0);
    assert_eq!(rank_for_failover(&id).0, 2);
}

#[test]
fn success_clears_probe_failures() {
    let id = unique_id("ok");
    record_probe_failure(&id);
    record_success(&id, Some(12));
    let row = lookup(&id).expect("row");
    assert_eq!(row.status, "ok");
    assert_eq!(row.consecutive_failures, 0);
    assert_eq!(row.last_latency_ms, Some(12));
}

#[test]
fn unknown_upstream_is_not_cooling() {
    let id = unique_id("missing");
    assert!(lookup(&id).is_none());
    assert_eq!(rank_for_failover(&id).0, 0);
    assert!(is_available(&id, None));
}

#[test]
fn classify_probe_status_treats_client_errors_as_reachable() {
    assert!(classify_probe_status(reqwest::StatusCode::OK));
    assert!(classify_probe_status(reqwest::StatusCode::UNAUTHORIZED));
    assert!(classify_probe_status(reqwest::StatusCode::NOT_FOUND));
    assert!(classify_probe_status(
        reqwest::StatusCode::TOO_MANY_REQUESTS
    ));
    assert!(!classify_probe_status(reqwest::StatusCode::BAD_GATEWAY));
    assert!(!classify_probe_status(
        reqwest::StatusCode::SERVICE_UNAVAILABLE
    ));
}

#[test]
fn test_classify_failure_auth() {
    let c401 = classify_failure(401, None, Some(r#"{"error":{"message":"Invalid key"}}"#));
    assert_eq!(c401.kind, FailureKind::Auth);
    assert_eq!(c401.cooldown, Some(Duration::from_secs(COOLDOWN_AUTH_SECS)));
    assert_eq!(c401.safe_description, Some(SAFE_ERR_AUTH_FAILED.into()));

    let c403 = classify_failure(
        403,
        None,
        Some(r#"{"error":{"type":"permission_error","message":"Forbidden"}}"#),
    );
    assert_eq!(c403.kind, FailureKind::Auth);
    assert_eq!(c403.cooldown, Some(Duration::from_secs(COOLDOWN_AUTH_SECS)));
}

#[test]
fn test_classify_failure_quota() {
    let c402 = classify_failure(
        402,
        None,
        Some(r#"{"error":{"message":"Payment required"}}"#),
    );
    assert_eq!(c402.kind, FailureKind::Quota);
    assert_eq!(
        c402.cooldown,
        Some(Duration::from_secs(COOLDOWN_QUOTA_SECS))
    );
    assert_eq!(c402.safe_description, Some(SAFE_ERR_QUOTA_EXHAUSTED.into()));

    let c429_quota = classify_failure(
        429,
        None,
        Some(
            r#"{"error":{"code":"insufficient_quota","message":"You exceeded your current quota"}}"#,
        ),
    );
    assert_eq!(c429_quota.kind, FailureKind::Quota);
    assert_eq!(
        c429_quota.cooldown,
        Some(Duration::from_secs(COOLDOWN_QUOTA_SECS))
    );

    let c_chinese_quota =
        classify_failure(403, None, Some(r#"{"message":"账户余额不足，请充值"}"#));
    assert_eq!(c_chinese_quota.kind, FailureKind::Quota);
}

#[test]
fn test_classify_failure_rate_limit_preserves_large_retry_after() {
    let mut headers = http::HeaderMap::new();
    headers.insert("retry-after", http::HeaderValue::from_static("3600"));

    let res = classify_failure(
        429,
        Some(&headers),
        Some(r#"{"error":{"message":"Rate limit exceeded"}}"#),
    );
    assert_eq!(res.kind, FailureKind::RateLimit);
    // Server specified 3600s must NOT be truncated to 300s!
    assert_eq!(res.cooldown, Some(Duration::from_secs(3600)));
    assert_eq!(res.safe_description, Some(SAFE_ERR_RATE_LIMITED.into()));
}

#[test]
fn test_classify_failure_rate_limit_without_retry_after() {
    let res = classify_failure(
        429,
        None,
        Some(r#"{"error":{"message":"Rate limit exceeded"}}"#),
    );
    assert_eq!(res.kind, FailureKind::RateLimit);
    assert_eq!(res.cooldown, None);
}

#[test]
fn test_classify_failure_model_unsupported_vs_general_404() {
    let model_err = classify_failure(
        404,
        None,
        Some(r#"{"error":{"code":"model_not_found","message":"The model does not exist"}}"#),
    );
    assert_eq!(model_err.kind, FailureKind::ModelUnsupported);
    assert_eq!(
        model_err.cooldown,
        Some(Duration::from_secs(COOLDOWN_MODEL_SECS))
    );
    assert_eq!(
        model_err.safe_description,
        Some(SAFE_ERR_MODEL_NOT_FOUND.into())
    );

    let general_404 = classify_failure(
        404,
        None,
        Some(r#"{"error":"Cannot POST /v1/messages/invalid_path"}"#),
    );
    assert_eq!(general_404.kind, FailureKind::ClientErrorIgnored);
    assert_eq!(general_404.cooldown, None);
}

#[test]
fn test_no_false_positive_on_word_balance_or_model() {
    let body_model_word = r#"{"choices":[{"text":"The statistical model works well"}]}"#;
    let c404 = classify_failure(404, None, Some(body_model_word));
    assert_eq!(c404.kind, FailureKind::ClientErrorIgnored);

    let body_balance_word = r#"{"status":"active","description":"Balanced load between servers"}"#;
    let c400 = classify_failure(400, None, Some(body_balance_word));
    assert_eq!(c400.kind, FailureKind::ClientErrorIgnored);
}

#[test]
fn test_model_scope_does_not_affect_other_models_or_upstream() {
    let up_id = unique_id("model_scope");
    let model_fail = "gpt-model-x";
    let model_ok = "claude-model-y";

    let kind = record_http_failure(
        &up_id,
        Some(model_fail),
        404,
        None,
        Some(r#"{"error":{"code":"model_not_found","message":"Model not found"}}"#),
    );
    assert_eq!(kind, FailureKind::ModelUnsupported);

    assert!(!is_available(&up_id, Some(model_fail)));
    assert!(is_available(&up_id, Some(model_ok)));
    assert!(is_available(&up_id, None));

    let (rank_fail, _) = rank_for_failover_model(&up_id, Some(model_fail));
    let (rank_ok, _) = rank_for_failover_model(&up_id, Some(model_ok));
    assert_eq!(rank_fail, 2);
    assert_eq!(rank_ok, 0);

    record_model_success(&up_id, model_ok, Some(50));
    assert!(!is_available(&up_id, Some(model_fail)));
    assert!(is_available(&up_id, Some(model_ok)));

    record_model_success(&up_id, model_fail, Some(45));
    assert!(is_available(&up_id, Some(model_fail)));
}

#[test]
fn test_probe_401_sets_auth_failed() {
    let up_id = unique_id("probe_auth");
    record_probe_http_status(&up_id, 401, Some(15));
    let row = lookup(&up_id).expect("health");
    assert_eq!(row.status, "auth_failed");
    assert_eq!(row.last_error, Some(SAFE_ERR_AUTH_FAILED.to_string()));
    assert!(row.cooldown_remaining_ms > 0);
    assert_eq!(rank_for_failover(&up_id).0, 3);
    assert!(!is_available(&up_id, None));
}

#[test]
fn test_probe_404_reachable_unknown_not_inference_success() {
    let up_id = unique_id("probe_404");
    record_probe_http_status(&up_id, 404, Some(20));
    let row = lookup(&up_id).expect("health");
    assert_eq!(row.status, "unknown");
    assert_eq!(row.consecutive_failures, 0);
    assert_eq!(row.cooldown_remaining_ms, 0);

    record_failure(&up_id);
    record_failure(&up_id);
    assert_eq!(lookup(&up_id).unwrap().status, "cooling");

    record_probe_http_status(&up_id, 404, Some(10));
    assert_eq!(lookup(&up_id).unwrap().status, "cooling");
}

#[test]
fn test_probe_429_cannot_clear_auth_or_overwrite_live_quota() {
    let up_id = unique_id("probe_429_auth");
    record_probe_http_status(&up_id, 401, Some(10));
    assert_eq!(lookup(&up_id).unwrap().status, "auth_failed");

    record_probe_http_status(&up_id, 429, Some(10));
    let row = lookup(&up_id).expect("health");
    assert_eq!(row.status, "auth_failed");
    assert_eq!(row.last_error, Some(SAFE_ERR_AUTH_FAILED.to_string()));

    // Test live quota is not overwritten by probe 429
    let up_quota = unique_id("probe_429_live_quota");
    record_http_failure(
        &up_quota,
        None,
        402,
        None,
        Some(r#"{"error":{"message":"Quota exceeded"}}"#),
    );
    let before_cooldown = lookup(&up_quota).unwrap().cooldown_remaining_ms;
    assert!(before_cooldown > 3000 * 1000); // 1 hour quota cooldown

    record_probe_http_status(&up_quota, 429, Some(10));
    let after_cooldown = lookup(&up_quota).unwrap().cooldown_remaining_ms;
    // Must remain 1h quota cooldown, not replaced by 5s rate limit!
    assert!(after_cooldown > 3000 * 1000);
}

#[test]
fn test_probe_200_clears_probe_auth_but_not_live_auth_or_live_cooling() {
    // 1. Probe-originated auth failure CAN be cleared by probe 200
    let up_probe_auth = unique_id("probe_auth_clear");
    record_probe_http_status(&up_probe_auth, 401, Some(10));
    assert_eq!(lookup(&up_probe_auth).unwrap().status, "auth_failed");
    record_probe_http_status(&up_probe_auth, 200, Some(10));
    assert_eq!(lookup(&up_probe_auth).unwrap().status, "ok");

    // 2. Live-originated auth failure CANNOT be cleared by probe 200
    let up_live_auth = unique_id("live_auth_clear");
    record_http_failure(
        &up_live_auth,
        None,
        401,
        None,
        Some(r#"{"error":{"message":"Invalid key"}}"#),
    );
    assert_eq!(lookup(&up_live_auth).unwrap().status, "auth_failed");
    record_probe_http_status(&up_live_auth, 200, Some(10));
    // Live auth MUST remain auth_failed!
    assert_eq!(lookup(&up_live_auth).unwrap().status, "auth_failed");

    // 3. Live cooling is not cleared by probe 200
    let up_live = unique_id("live_cooling_clear");
    record_failure(&up_live);
    record_failure(&up_live);
    assert_eq!(lookup(&up_live).unwrap().status, "cooling");

    record_probe_http_status(&up_live, 200, Some(10));
    assert_eq!(lookup(&up_live).unwrap().status, "cooling");
}

#[test]
fn test_auth_active_does_not_get_shortened_by_concurrent_429_or_transient() {
    let up_id = unique_id("auth_no_shorten");
    record_http_failure(
        &up_id,
        None,
        401,
        None,
        Some(r#"{"error":{"message":"Unauthorized"}}"#),
    );
    let initial_cooldown = lookup(&up_id).unwrap().cooldown_remaining_ms;
    assert!(initial_cooldown > 1700 * 1000); // 30 minutes

    // Concurrent 429 arrives (would normally have 5s cooldown)
    record_http_failure(
        &up_id,
        None,
        429,
        None,
        Some(r#"{"error":{"message":"Rate limit"}}"#),
    );
    let after_429 = lookup(&up_id).unwrap().cooldown_remaining_ms;
    assert!(after_429 > 1700 * 1000); // Must NOT be shortened to 5s!
    assert_eq!(lookup(&up_id).unwrap().status, "auth_failed");

    // Concurrent transient 500 arrives (would normally have 60s cooldown)
    record_http_failure(&up_id, None, 500, None, None);
    let after_500 = lookup(&up_id).unwrap().cooldown_remaining_ms;
    assert!(after_500 > 1700 * 1000); // Must NOT be shortened to 60s!
    assert_eq!(lookup(&up_id).unwrap().status, "auth_failed");
}

#[test]
fn test_circuit_permit_and_half_open_raii() {
    let up_id = unique_id("permit");

    {
        let p = acquire_permit(&up_id, None);
        assert!(p.is_some());
        assert!(!p.unwrap().is_half_open());
    }

    {
        let mut table = lock_table();
        let entry = table.entry(up_id.clone()).or_insert_with(HealthEntry::new);
        entry.open_until = Some(Instant::now() - Duration::from_millis(10));
        entry.consecutive_failures = 2;
    }

    assert!(is_available(&up_id, None));

    let p1 = acquire_permit(&up_id, None);
    assert!(p1.is_some());
    let permit1 = p1.unwrap();
    assert!(permit1.is_half_open());

    assert!(!is_available(&up_id, None));
    let p2 = acquire_permit(&up_id, None);
    assert!(p2.is_none());

    permit1.record_success(Some(25));

    assert!(is_available(&up_id, None));
    let row = lookup(&up_id).unwrap();
    assert_eq!(row.status, "ok");
    assert_eq!(row.consecutive_failures, 0);
}

#[test]
fn test_circuit_permit_drop_releases_half_open() {
    let up_id = unique_id("permit_drop");

    {
        let mut table = lock_table();
        let entry = table.entry(up_id.clone()).or_insert_with(HealthEntry::new);
        entry.open_until = Some(Instant::now() - Duration::from_millis(10));
    }

    let p1 = acquire_permit(&up_id, None);
    assert!(p1.is_some());
    assert!(p1.as_ref().unwrap().is_half_open());

    assert!(acquire_permit(&up_id, None).is_none());

    drop(p1);

    let p2 = acquire_permit(&up_id, None);
    assert!(p2.is_some());
    assert!(p2.as_ref().unwrap().is_half_open());
}

#[test]
fn test_permit_record_http_failure_client_error_releases_model_probe() {
    let up_id = unique_id("permit_client_err");
    let model = "test-model";

    // Simulate model in half-open state
    {
        let mut table = lock_table();
        let entry = table.entry(up_id.clone()).or_insert_with(HealthEntry::new);
        let m_entry = entry
            .model_entries
            .entry(model.to_string())
            .or_insert_with(ModelHealthEntry::new);
        m_entry.open_until = Some(Instant::now() - Duration::from_millis(10));
    }

    let p1 = acquire_permit(&up_id, Some(model));
    assert!(p1.is_some());
    let permit = p1.unwrap();
    assert!(permit.is_half_open());

    // Record ClientErrorIgnored (e.g. 400 Bad Request)
    let kind = permit.record_http_failure(400, None, Some(r#"{"error":"Bad prompt"}"#));
    assert_eq!(kind, FailureKind::ClientErrorIgnored);

    // RAII must have cleanly released model probe, not locked it!
    let p2 = acquire_permit(&up_id, Some(model));
    assert!(p2.is_some());
}

#[test]
fn test_parse_retry_after() {
    let now = chrono::Utc::now();
    assert_eq!(parse_retry_after("120", now), Some(120));
    assert_eq!(parse_retry_after(" 45.2 ", now), Some(46));
    assert_eq!(parse_retry_after("invalid", now), None);

    let future = now + chrono::Duration::seconds(60);
    let rfc2822 = future.to_rfc2822();
    let parsed = parse_retry_after(&rfc2822, now);
    assert!(parsed.is_some());
    let diff = parsed.unwrap();
    assert!(diff >= 58 && diff <= 62);
}

#[test]
fn test_min_cooldown_remaining() {
    let up_id = unique_id("min_cool");
    record_http_failure(
        &up_id,
        None,
        429,
        None,
        Some(r#"{"error":{"message":"Rate limit"}}"#),
    );
    let secs = min_cooldown_remaining_secs(&up_id, None);
    assert!(secs.is_some());
    assert!(secs.unwrap() > 0 && secs.unwrap() <= 5);
}
