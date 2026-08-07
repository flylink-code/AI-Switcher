//! Built-in Antigravity account pool and local API gateway.
//!
//! Independently implemented personal gateway that turns Google/Antigravity
//! OAuth accounts into Anthropic/OpenAI-compatible endpoints for agent tools.
//! Architecture is inspired by public Antigravity tooling; no third-party
//! CC-BY-NC-SA source is vendored here.

pub mod account;
pub mod gateway;
pub mod map;
pub mod model_catalog;
pub mod oauth;
pub mod outbound;
pub mod pool;
pub mod quota;
pub mod upstream;
pub mod usage_log;

pub use account::{
    import_accounts_json, list_accounts, remove_account, set_active_account, AntigravityAccount,
    AntigravityAccountPublic,
};
pub use gateway::{
    gateway_status, restore_gateway_if_enabled, set_gateway_api_key, set_gateway_port,
    set_outbound_proxy, start_gateway, stop_gateway, AntigravityGatewayStatus, DEFAULT_GATEWAY_PORT,
};
pub use oauth::login_with_browser;
pub use model_catalog::{list_catalog_models, list_model_ids, CatalogModel};
pub use outbound::DEFAULT_CLASH_PROXY_URL;
pub use pool::AccountPool;
pub use quota::QuotaSnapshot;
