//! 出站尝试的健康门禁和结果记录；仅在响应交给客户端之前执行。

use super::*;
use crate::gateway::health::{self, FailureKind};

#[derive(Debug, Clone, Copy)]
struct ObservedFailure(FailureKind);

pub(crate) fn should_try_explicit_response(provider: &Provider, response: &reqwest::Response) -> bool {
    should_try_explicit_fallback(provider, response.status())
        || response.extensions().get::<ObservedFailure>()
            .is_some_and(|failure| failure.0 == FailureKind::ModelUnsupported)
}

pub(crate) async fn send_observed_upstream(
    builder: reqwest::RequestBuilder,
    provider: &Provider,
) -> Result<reqwest::Response, reqwest::Error> {
    // 短冷却在提交客户端响应前等待；长冷却直接交给备用候选或客户端。
    if !health::is_available(&provider.id, Some(&provider.model)) {
        if let Some(wait) = health::min_cooldown_remaining(&provider.id, Some(&provider.model)) {
            if wait <= Duration::from_secs(10) {
                tokio::time::sleep(wait).await;
            }
        }
    }
    let Some(_permit) = health::acquire_permit(&provider.id, Some(&provider.model)) else {
        let retry_after = health::min_cooldown_remaining_secs(&provider.id, Some(&provider.model))
            .unwrap_or(1).max(1);
        return Ok(http::Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::RETRY_AFTER, retry_after.to_string())
            .body(reqwest::Body::from(r#"{"error":{"type":"rate_limit_error","message":"上游正在冷却，请稍后重试"}}"#))
            .expect("固定的冷却响应有效")
            .into());
    };
    let started = Instant::now();
    let mut response = match builder.send().await {
        Ok(response) => response,
        Err(error) => {
            // 本机反代管理自己的账号池，网络连接失败不应给账号或整个反代冷却。
            if !provider.is_antigravity() && !provider.is_kiro() {
                health::record_failure(&provider.id);
            }
            return Err(error);
        }
    };
    if response.status().is_success() {
        health::record_model_success(&provider.id, &provider.model, Some(started.elapsed().as_millis() as i64));
        return Ok(response);
    }
    let status = response.status();
    let headers = response.headers().clone();
    let version = response.version();
    let mut body = Vec::new();
    // 只缓冲有界错误正文；成功流不缓冲，也不重复消费客户端流。
    while let Some(chunk) = response.chunk().await? {
        let remaining = MAX_UPSTREAM_ERROR_BYTES.saturating_sub(body.len());
        body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        if body.len() >= MAX_UPSTREAM_ERROR_BYTES { break; }
    }
    let text = String::from_utf8_lossy(&body);
    let kind = if provider.is_antigravity() || provider.is_kiro() {
        // 429/504/5xx 已在反代内部处理，不能在入口再封掉整个账号池。
        let classified = health::classify_failure(status.as_u16(), Some(&headers), Some(&text));
        if matches!(classified.kind, FailureKind::Auth | FailureKind::ModelUnsupported) {
            health::record_http_failure(&provider.id, Some(&provider.model), status.as_u16(), Some(&headers), Some(&text));
        }
        classified.kind
    } else {
        health::record_http_failure(&provider.id, Some(&provider.model), status.as_u16(), Some(&headers), Some(&text))
    };
    let mut rebuilt = http::Response::builder().status(status).version(version);
    *rebuilt.headers_mut().expect("响应头存在") = headers;
    rebuilt.headers_mut().expect("响应头存在").remove(header::CONTENT_LENGTH);
    rebuilt.headers_mut().expect("响应头存在").remove(header::TRANSFER_ENCODING);
    let mut response: reqwest::Response = rebuilt.body(reqwest::Body::from(body))
        .expect("上游响应状态与头有效").into();
    response.extensions_mut().insert(ObservedFailure(kind));
    Ok(response)
}
