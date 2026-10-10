//! 将请求资源的生命周期绑定到响应体，而不是响应头返回时刻。

use axum::body::Body;
use axum::response::Response;
use http_body_util::{BodyExt, StreamBody};
use std::sync::Arc;
use crate::database::Database;

/// 每次入站请求独立分配，不使用客户端可复用的 correlation_id 去重。
#[derive(Clone, Default)]
struct RequestLogInner {
    id: Option<String>,
    inflight: Option<InflightLog>,
}

#[derive(Clone, Default)]
pub(crate) struct RequestLogSlot(Arc<std::sync::Mutex<RequestLogInner>>);

/// 响应头返回前被取消时，用来补上供应商、路由和尝试链。
#[derive(Clone)]
pub(crate) struct InflightLog {
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    pub protocol: String,
    pub is_stream: bool,
    pub profile_id: Option<String>,
    pub route_reason: Option<String>,
    pub requested_model: Option<String>,
    pub upstream_id: Option<String>,
    pub route_mode: Option<String>,
    pub attempts_json: String,
}

impl RequestLogSlot {
    pub(crate) fn record(&self, id: &str) {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).id = Some(id.to_string());
    }

    pub(crate) fn note_inflight(&self, snapshot: InflightLog) {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).inflight = Some(snapshot);
    }

    pub(crate) fn recorded_id(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).id.clone()
    }
}

pub(super) struct PendingRequestGuard {
    db: Arc<Database>,
    slot: RequestLogSlot,
    target: crate::provider::ProviderTarget,
    route: String,
    correlation: Option<crate::gateway::correlation::Correlation>,
    started: std::time::Instant,
    armed: bool,
}

impl PendingRequestGuard {
    pub(super) fn new(state: &mut super::ProxyState) -> Self {
        let slot = RequestLogSlot::default();
        state.request_log = Some(slot.clone());
        Self {
            db: state.db.clone(), slot, target: state.target,
            route: state.request_path.clone(), correlation: state.correlation.clone(),
            started: std::time::Instant::now(), armed: true,
        }
    }

    pub(super) fn disarm(&mut self) { self.armed = false; }
}

impl Drop for PendingRequestGuard {
    fn drop(&mut self) {
        if !self.armed { return; }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else { return; };
        let (db, slot, target, route, correlation) = (
            self.db.clone(), self.slot.clone(), self.target, self.route.clone(), self.correlation.clone(),
        );
        let duration = self.started.elapsed().as_millis().min(i64::MAX as u128) as i64;
        runtime.spawn_blocking(move || {
            let result = db.with_conn(|conn| {
                let inner = slot.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
                let id = match inner.id {
                    Some(id) => id,
                    None => {
                        let inflight = inner.inflight.as_ref();
                        let id = crate::database::dao::proxy_logs::insert_proxy_log(
                            conn,
                            inflight.map(|item| item.provider_id.as_str()),
                            inflight.map(|item| item.provider_name.as_str()),
                            inflight.map(|item| item.model.as_str()),
                            None,
                            duration,
                            Some(target.as_str()),
                            inflight.map(|item| item.protocol.as_str()),
                            Some(&route),
                            inflight.is_some_and(|item| item.is_stream),
                            Some("cancelled"),
                            Some("客户端在响应提交前断开"),
                        )?;
                        crate::database::dao::proxy_logs::update_proxy_log_hop(
                            conn, &id, correlation.as_ref().map(|item| item.id.as_str()),
                            correlation.as_ref().map(|item| item.hop),
                        )?;
                        if let Some(snapshot) = inflight {
                            crate::database::dao::proxy_logs::update_proxy_log_route(
                                conn, &id,
                                snapshot.profile_id.as_deref(),
                                snapshot.route_reason.as_deref(),
                                0,
                                snapshot.requested_model.as_deref(),
                                snapshot.upstream_id.as_deref(),
                                snapshot.route_mode.as_deref(),
                                Some(snapshot.attempts_json.as_str()),
                            )?;
                        }
                        id
                    }
                };
                crate::database::dao::proxy_logs::update_proxy_log_stream_outcome(
                    conn, &id, "cancelled", None, Some("cancelled"), Some("客户端在响应提交前断开"),
                )
            });
            match result {
                Ok(_) => crate::usage_events::notify_log_recorded(),
                Err(error) => log::warn!("记录响应头前取消失败: {error}"),
            }
        });
    }
}

/// 只携带数据库和日志 ID；Drop 不阻塞网络 executor，也不改上游健康。
struct StreamLogGuard {
    db: Arc<Database>,
    id: Option<String>,
}

impl StreamLogGuard {
    fn finish(&mut self, outcome: &'static str) {
        let Some(id) = self.id.take() else { return; };
        let db = Arc::clone(&self.db);
        let Ok(runtime) = tokio::runtime::Handle::try_current() else { return; };
        runtime.spawn_blocking(move || {
            let result = db.with_conn(|conn| {
                // 协议转换器已记录的错误/完成优先；取消不能覆盖已有终态。
                Ok(conn.execute(
                    "UPDATE proxy_request_logs SET stream_outcome = ?1
                     WHERE id = ?2 AND stream_outcome IS NULL",
                    rusqlite::params![outcome, id],
                )?)
            });
            match result {
                Ok(changed) if changed > 0 => crate::usage_events::notify_log_recorded(),
                Err(error) => log::warn!("记录响应体终态失败: {error}"),
                _ => {}
            }
        });
    }
}

impl Drop for StreamLogGuard {
    fn drop(&mut self) { self.finish("cancelled"); }
}

fn classify_protocol_terminal(event: &serde_json::Value) -> Option<(&'static str, Option<&'static str>)> {
    let kind = event.get("type").and_then(serde_json::Value::as_str).unwrap_or("");
    match kind {
        "response.completed" | "message_stop" => return Some(("complete", None)),
        "response.failed" | "response.incomplete" | "error" => {
            return Some(("midstream_error", Some("midstream_error")));
        }
        _ => {}
    }
    if event.get("error").is_some_and(|error| !error.is_null()) {
        return Some(("midstream_error", Some("midstream_error")));
    }
    let finish = event.pointer("/choices/0/finish_reason").and_then(serde_json::Value::as_str).unwrap_or("");
    if !finish.is_empty() {
        return Some(("complete", None));
    }
    None
}

pub(super) fn record_sse_terminal(db: &Database, id: Option<&str>, data: &str) {
    let data = data.trim();
    if data == "[DONE]" {
        let Some(id) = id else { return; };
        if let Err(error) = db.with_conn(|conn| {
            crate::database::dao::proxy_logs::update_proxy_log_stream_outcome(
                conn, id, "complete", None, None, None,
            )
        }) {
            log::warn!("记录流式协议终态失败: {error}");
        }
        return;
    }
    if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
        record_response_event(db, id, &event);
    }
}

pub(super) fn record_response_event(db: &Database, id: Option<&str>, event: &serde_json::Value) {
    let Some(id) = id else { return; };
    let Some((outcome, category)) = classify_protocol_terminal(event) else { return; };
    if let Err(error) = db.with_conn(|conn| {
        crate::database::dao::proxy_logs::update_proxy_log_stream_outcome(
            conn, id, outcome, None, category,
            category.map(|_| "上游返回未完成或错误的流式结果"),
        )
    }) {
        log::warn!("记录流式协议终态失败: {error}");
    }
}

pub(super) fn track_stream_body(body: Body, db: Arc<Database>, id: Option<String>) -> Body {
    let guard = StreamLogGuard { db, id };
    let stream = futures_util::stream::unfold((body, guard), |(mut body, mut guard)| async move {
        match body.frame().await {
            Some(frame) => {
                if frame.is_err() { guard.finish("midstream_error"); }
                Some((frame, (body, guard)))
            }
            None => {
                guard.finish("complete");
                None
            }
        }
    });
    Body::new(StreamBody::new(stream))
}


pub(super) fn hold_response_guard<G: Send + 'static>(response: Response, guard: G) -> Response {
    let (parts, body) = response.into_parts();
    Response::from_parts(parts, hold_body_guard(body, guard))
}

fn hold_body_guard<G: Send + 'static>(body: Body, guard: G) -> Body {
    // 使用 Frame 而非 data stream，不能丢失上游 trailers。
    let stream = futures_util::stream::unfold((body, guard), |(mut body, guard)| async move {
        body.frame().await.map(|frame| (frame, (body, guard)))
    });
    Body::new(StreamBody::new(stream))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use axum::{body::Bytes, routing::get, Router};
    use tokio::sync::Notify;

    fn log_database() -> Arc<Database> {
        let db = Arc::new(Database::memory().unwrap());
        db.with_conn(|conn| {
            crate::database::dao::proxy_logs::insert_proxy_log_with_source(
                conn, Some("lifecycle"), 1, None, None, None, Some(200),
                0, 0, 0, 0, false, 0, Some("claude_code"), Some("anthropic"),
                Some("/v1/messages"), true, None, None, "proxy", None,
            )?;
            Ok(())
        }).unwrap();
        db
    }

    async fn expect_outcome(db: &Database, expected: &str) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let outcome: Option<String> = db.with_conn(|conn| {
                    Ok(conn.query_row(
                        "SELECT stream_outcome FROM proxy_request_logs WHERE id = 'lifecycle'",
                        [], |row| row.get(0),
                    )?)
                }).unwrap();
                if outcome.as_deref() == Some(expected) { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("终态应异步落库");
    }

    #[tokio::test]
    async fn pending_cancel_reuses_only_its_own_log_slot() {
        let mut state = super::super::tests::circuit_test_state();
        state.correlation = Some(crate::gateway::correlation::Correlation {
            id: "reused-client-id".into(), target_app: Some("claude_code".into()),
            hop: crate::gateway::correlation::HOP_SMART_GATEWAY,
        });
        state.request_path = "/v1/messages".into();
        let first = PendingRequestGuard::new(&mut state);
        super::super::log_early_failure(&state, "/v1/messages", "request", Some(400), 1);
        let existing = state.request_log.as_ref().unwrap().recorded_id().unwrap();
        drop(first);
        expect_outcome_by_id(&state.db, &existing).await;
        let second = PendingRequestGuard::new(&mut state);
        drop(second);
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let count: i64 = state.db.with_conn(|conn| Ok(conn.query_row(
                    "SELECT count(*) FROM proxy_request_logs WHERE stream_outcome='cancelled'", [], |row| row.get(0),
                )?)).unwrap();
                if count == 2 { break; }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.unwrap();
    }

    async fn expect_outcome_by_id(db: &Database, id: &str) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let value: Option<String> = db.with_conn(|conn| Ok(conn.query_row(
                    "SELECT stream_outcome FROM proxy_request_logs WHERE id=?1", [id], |row| row.get(0),
                )?)).unwrap();
                if value.as_deref() == Some("cancelled") { break; }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.unwrap();
    }

    #[tokio::test]
    async fn stream_log_records_complete_cancel_and_error_once() {
        let db = log_database();
        let body = track_stream_body(Body::from("payload"), db.clone(), Some("lifecycle".into()));
        assert_eq!(axum::body::to_bytes(body, 1024).await.unwrap(), "payload");
        expect_outcome(&db, "complete").await;
        db.with_conn(|conn| {
            crate::database::dao::proxy_logs::update_proxy_log_stream_outcome(
                conn, "lifecycle", "cancelled", None, None, None,
            )
        }).unwrap();
        expect_outcome(&db, "complete").await;

        let db = log_database();
        let body = track_stream_body(Body::empty(), db.clone(), Some("lifecycle".into()));
        drop(body);
        expect_outcome(&db, "cancelled").await;

        let db = log_database();
        let stream = futures_util::stream::once(async {
            Err::<Bytes, _>(std::io::Error::other("测试断流"))
        });
        let body = track_stream_body(Body::from_stream(stream), db.clone(), Some("lifecycle".into()));
        assert!(axum::body::to_bytes(body, 1024).await.is_err());
        expect_outcome(&db, "midstream_error").await;
    }

    #[tokio::test]
    async fn stream_log_preserves_existing_protocol_error_on_cancel() {
        let db = log_database();
        db.with_conn(|conn| {
            crate::database::dao::proxy_logs::update_proxy_log_stream_outcome(
                conn, "lifecycle", "midstream_error", None, Some("timeout"), None,
            )
        }).unwrap();
        drop(track_stream_body(Body::empty(), db.clone(), Some("lifecycle".into())));
        tokio::time::sleep(Duration::from_millis(30)).await;
        expect_outcome(&db, "midstream_error").await;
    }

    #[tokio::test]
    async fn response_protocol_failure_cannot_become_complete_or_cancelled() {
        let db = log_database();
        record_response_event(&db, Some("lifecycle"), &serde_json::json!({
            "type": "response.failed", "error": {"message": "不应记录的原始正文"}
        }));
        record_response_event(&db, Some("lifecycle"), &serde_json::json!({"type":"response.completed"}));
        let body = track_stream_body(Body::from("done"), db.clone(), Some("lifecycle".into()));
        axum::body::to_bytes(body, 1024).await.unwrap();
        expect_outcome(&db, "midstream_error").await;
        let diagnostic: String = db.with_conn(|conn| Ok(conn.query_row(
            "SELECT diagnostic FROM proxy_request_logs WHERE id = 'lifecycle'",
            [], |row| row.get(0),
        )?)).unwrap();
        assert!(!diagnostic.contains("原始正文"));
    }

    #[tokio::test]
    async fn anthropic_and_chat_terminals_are_written_once() {
        let db = log_database();
        record_response_event(&db, Some("lifecycle"), &serde_json::json!({"type":"message_stop"}));
        record_response_event(&db, Some("lifecycle"), &serde_json::json!({"type":"error","error":{"message":"late"}}));
        drop(track_stream_body(Body::empty(), db.clone(), Some("lifecycle".into())));
        tokio::time::sleep(Duration::from_millis(30)).await;
        expect_outcome(&db, "complete").await;

        let db = log_database();
        record_sse_terminal(&db, Some("lifecycle"), r#"{"choices":[{"finish_reason":"stop","delta":{}}]}"#);
        record_sse_terminal(&db, Some("lifecycle"), "[DONE]");
        drop(track_stream_body(Body::empty(), db.clone(), Some("lifecycle".into())));
        tokio::time::sleep(Duration::from_millis(30)).await;
        expect_outcome(&db, "complete").await;

        let db = log_database();
        record_response_event(&db, Some("lifecycle"), &serde_json::json!({"type":"error"}));
        record_response_event(&db, Some("lifecycle"), &serde_json::json!({"type":"message_stop"}));
        record_sse_terminal(&db, Some("lifecycle"), "[DONE]");
        drop(track_stream_body(Body::empty(), db.clone(), Some("lifecycle".into())));
        tokio::time::sleep(Duration::from_millis(30)).await;
        expect_outcome(&db, "midstream_error").await;
    }

    struct Guard(Arc<AtomicUsize>);
    impl Drop for Guard {
        fn drop(&mut self) { self.0.fetch_add(1, Ordering::SeqCst); }
    }

    #[tokio::test]
    async fn response_guard_survives_headers_and_releases_on_body_drop() {
        let drops = Arc::new(AtomicUsize::new(0));
        let response = hold_response_guard(Response::new(Body::from("payload")), Guard(drops.clone()));
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        let body = response.into_body();
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(body);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn response_guard_releases_on_complete() {
        let drops = Arc::new(AtomicUsize::new(0));
        let body = hold_body_guard(Body::from("payload"), Guard(drops.clone()));
        assert_eq!(axum::body::to_bytes(body, 1024).await.unwrap(), "payload");
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    async fn wait_for_drop(drops: &AtomicUsize) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while drops.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("真实 TCP 断连后请求资源必须释放");
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn tcp_disconnect_before_response_headers_drops_waiting_handler() {
        let drops = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(Notify::new());
        let (count, signal) = (drops.clone(), entered.clone());
        let app = Router::new().route("/", get(move || {
            let (count, signal) = (count.clone(), signal.clone());
            async move {
                let _guard = Guard(count);
                signal.notify_one();
                std::future::pending::<()>().await;
                "unreachable"
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let socket = tokio::task::spawn_blocking(move || {
            let mut socket = std::net::TcpStream::connect(address).unwrap();
            socket.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
            socket
        }).await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), entered.notified()).await.unwrap();
        socket.shutdown(std::net::Shutdown::Both).unwrap();
        drop(socket);
        // 在断言前注册 abort guard，即使断言失败也不留下测试服务器。
        let result = tokio::time::timeout(Duration::from_secs(4), wait_for_drop(&drops)).await;
        server.abort();
        result.unwrap();
    }

    #[tokio::test]
    async fn tcp_disconnect_after_headers_drops_stream_guard() {
        let drops = Arc::new(AtomicUsize::new(0));
        let count = drops.clone();
        let db = log_database();
        let handler_db = db.clone();
        let app = Router::new().route("/", get(move || {
            let count = count.clone();
            let db = handler_db.clone();
            async move {
                let stream = futures_util::stream::once(async { Ok::<_, Infallible>("data: ready\n\n") });
                let stream = futures_util::StreamExt::chain(stream, futures_util::stream::pending());
                let body = track_stream_body(Body::from_stream(stream), db, Some("lifecycle".into()));
                hold_response_guard(Response::new(body), Guard(count))
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        tokio::task::spawn_blocking(move || {
            let mut socket = std::net::TcpStream::connect(address).unwrap();
            socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            socket.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
            let mut response = Vec::new();
            while !response.windows(5).any(|value| value == b"ready") {
                let mut buffer = [0; 1024];
                let n = socket.read(&mut buffer).unwrap();
                assert!(n > 0);
                response.extend_from_slice(&buffer[..n]);
            }
            socket.shutdown(std::net::Shutdown::Both).unwrap();
        }).await.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(4), wait_for_drop(&drops)).await;
        server.abort();
        result.unwrap();
        expect_outcome(&db, "cancelled").await;
    }
}
