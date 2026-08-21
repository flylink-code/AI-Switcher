//! Built-in Antigravity account pool and local API gateway.
//!
//! Independently implemented personal gateway that turns Google/Antigravity
//! OAuth accounts into Anthropic/OpenAI-compatible endpoints for agent tools.
//! Architecture is inspired by public Antigravity tooling; no third-party
//! CC-BY-NC-SA source is vendored here.

pub mod account;
pub mod fast_path;
pub mod gateway;
pub mod limiter;
pub mod map;
pub mod model_catalog;
pub mod oauth;
pub mod outbound;
pub mod pool;
pub mod quota;
pub mod quota_sync;
pub mod retry_info;
pub mod session_effort;
pub mod thinking;
pub mod thought_sig;
pub mod upstream;
pub mod usage_log;

pub use account::{
    import_accounts_json, list_accounts, remove_account, set_active_account, AntigravityAccount,
    AntigravityAccountPublic,
};
pub use gateway::{
    clear_sticky_sessions, gateway_status, get_fast_path_settings, get_limiter_settings, pool_instance,
    restore_gateway_if_enabled, set_fast_path_settings, set_gateway_api_key, set_gateway_port, set_limiter_settings,
    set_outbound_proxy, start_gateway, stop_gateway, AntigravityGatewayStatus,
    DEFAULT_GATEWAY_PORT,
};
pub use limiter::LimiterSettings;
pub use fast_path::FastPathSettings;
pub use oauth::login_with_browser;
pub use model_catalog::{list_catalog_models, list_model_ids, CatalogModel};
pub use outbound::DEFAULT_CLASH_PROXY_URL;
pub use pool::AccountPool;
pub use quota::QuotaSnapshot;
pub use quota_sync::{
    refresh_all_account_quotas, refresh_one_account_quota, try_refresh_all_quotas,
    QuotaRefreshSummary, QUOTA_REFRESH_EVENT, QUOTA_REFRESH_INTERVAL_SECS,
};
