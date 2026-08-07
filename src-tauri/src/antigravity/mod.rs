//! Built-in Antigravity account pool and local API gateway.
//!
//! Independently implemented personal gateway that turns Google/Antigravity
//! OAuth accounts into Anthropic/OpenAI-compatible endpoints for agent tools.
//! Architecture is inspired by public Antigravity tooling; no third-party
//! CC-BY-NC-SA source is vendored here.

pub mod account;
pub mod gateway;
pub mod map;
pub mod oauth;
pub mod pool;
pub mod quota;
pub mod upstream;
pub mod usage_log;

pub use account::{
    import_accounts_json, list_accounts, remove_account, set_active_account, AntigravityAccount,
    AntigravityAccountPublic,
};
pub use gateway::{
    gateway_status, set_gateway_api_key, set_gateway_port, start_gateway, stop_gateway,
    AntigravityGatewayStatus, DEFAULT_GATEWAY_PORT,
};
pub use oauth::login_with_browser;
pub use pool::AccountPool;
pub use quota::QuotaSnapshot;
