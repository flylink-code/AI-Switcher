//! Local HTTP proxy that exposes an Anthropic-compatible `/v1/messages` endpoint.
//!
//! Claude Desktop is pointed at `http://127.0.0.1:<port>`; the proxy forwards
//! requests to the active third-party provider after mapping the model name and
//! injecting the real API key. Request summaries are written to the SQLite log
//! table for the usage dashboard (P4).

pub(crate) mod convert;
mod codex;
mod codex_anthropic;
mod codex_auto_review;
mod codex_chat;
mod codex_compact;
mod codex_history;
mod codex_moonshot_schema;
mod web_tools;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::convert::Infallible;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use bytes::Bytes;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::{mpsc::UnboundedSender, oneshot};
use tokio::task::JoinHandle;
use tower_http::cors::CorsLayer;

use crate::database::dao::proxy_logs::{
    insert_proxy_log, maintain_proxy_logs as maintain_logs, update_proxy_log_diagnostic,
    update_proxy_log_hop, update_proxy_log_route, update_proxy_log_stream_outcome, update_proxy_log_usage_idempotent, extract_usage_envelope_id,
};
use crate::database::dao::providers::{get_current_provider, list_providers, resolve_api_key};
use crate::database::dao::settings::get_setting;
use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::catalog::{claude_discovery_payload, openai_models_payload, rewrite_json_model, CatalogStyle};
use crate::provider::{
    api_endpoint_url, protocol_endpoint_path_for_provider, resolve_upstream_model, ProtocolType,
    Provider, ProviderTarget,
};

const DEFAULT_PORT: u16 = 15821;
const LOG_RETENTION_DAYS_KEY: &str = "proxy_log_retention_days";
const LOG_MAX_ROWS_KEY: &str = "proxy_log_max_rows";
const LOG_AUTO_MAINTAIN_KEY: &str = "proxy_log_auto_maintain";
pub const PROXY_FAILOVER_ENABLED_KEY: &str = "proxy_failover_enabled";
pub const PROXY_RETRYABLE_STATUS_CODES_KEY: &str = "proxy_retryable_status_codes";
pub const PROXY_STREAMING_IDLE_TIMEOUT_KEY: &str = "proxy_streaming_idle_timeout_secs";
const DEFAULT_STREAMING_IDLE_TIMEOUT_SECS: u64 = 180;
const MAX_UPSTREAM_ERROR_BYTES: usize = 16 * 1024;
const CIRCUIT_FAILURE_THRESHOLD: u8 = 2;
const CIRCUIT_OPEN_SECONDS: u64 = 60;
/// Max alternate upstreams tried on a single failing request chain.
pub(crate) const FAILOVER_MAX_HOPS: usize = 3;

include!("state.rs");
include!("route_select.rs");
include!("upstream_request.rs");
include!("failover.rs");
include!("logging.rs");
