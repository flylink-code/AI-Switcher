//! Built-in Kiro account pool and local API gateway.
//!
//! Turns Kiro / Amazon Q accounts into Anthropic and OpenAI endpoints.
//! Protocol behavior follows the public Kiro IDE wire format. Google
//! Antigravity mapping and thought-signature storage are not reused.

pub mod account;
pub mod account_test;
pub mod event_stream;
pub mod gateway;
pub mod map;
pub mod models;
pub mod oauth;
pub mod outbound;
pub mod pool;
pub mod quota;
pub mod token;
pub mod upstream;
pub mod usage_log;

pub use account::{import_accounts_json, list_accounts, remove_account, KiroAccountPublic};
pub use gateway::{
    gateway_status, set_gateway_api_key, set_gateway_port, set_outbound_proxy, start_gateway,
    stop_gateway, KiroGatewayStatus,
};
pub use models::preferred_default_model;
pub use oauth::{login_builder_id, login_social};
