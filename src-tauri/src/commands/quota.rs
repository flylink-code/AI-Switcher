//! Tauri command handlers for provider quota and balance queries.

use crate::database::dao;
use crate::error::{AppError, AppResult};
use crate::provider::{Provider, ProviderTarget};
use crate::quota::types::ProviderQuotaResult;
use crate::quota::{query_official_quota, query_provider_quota};
use crate::store::AppState;
use rusqlite::Connection;

/// Load a provider card, falling back to the smart-gateway upstream pool.
pub(crate) fn load_quota_provider(conn: &Connection, provider_id: &str) -> AppResult<Provider> {
    if let Some(provider) = dao::get_provider(conn, provider_id)? {
        return Ok(provider);
    }
    dao::gateway::get_upstream_provider(conn, provider_id)?
        .ok_or_else(|| AppError::Config(format!("供应商不存在: {provider_id}")))
}

#[tauri::command]
pub async fn get_provider_quota(
    provider_id: String,
    state: tauri::State<'_, AppState>,
) -> AppResult<ProviderQuotaResult> {
    let mut provider = state
        .db
        .with_read_conn(|conn| load_quota_provider(conn, &provider_id))?;
    provider.api_key = dao::materialize_api_key(&provider.api_key)?.unwrap_or_default();
    Ok(query_provider_quota(&provider).await)
}

#[tauri::command]
pub async fn get_official_quota(
    target: ProviderTarget,
) -> AppResult<ProviderQuotaResult> {
    Ok(query_official_quota(target).await)
}

#[cfg(test)]
mod tests {
    use super::load_quota_provider;
    use crate::database::dao;
    use crate::database::Database;
    use crate::error::AppError;
    use crate::provider::{
        ClaudeModelMapping, ProviderInput, ProviderKind, ProviderTarget, ProtocolType,
    };

    fn upstream_input(id: &str, name: &str, base_url: &str) -> ProviderInput {
        ProviderInput {
            id: Some(id.to_string()),
            name: name.to_string(),
            base_url: base_url.to_string(),
            api_key: String::new(),
            clear_api_key: false,
            model: "test-model".to_string(),
            model_context_window: None,
            auto_review_model_override: None,
            web_search_enabled: None,
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: ProtocolType::Anthropic,
            provider_kind: ProviderKind::Standard,
            auth_binding: String::new(),
            target_app: ProviderTarget::ClaudeCode,
            notes: String::new(),
            failover_group: 0,
            failover_models: Vec::new(),
            hidden_models: Vec::new(),
            thinking_config: None,
            custom_headers: None,
        }
    }

    #[test]
    fn load_quota_provider_falls_back_to_upstream_pool() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let created = dao::gateway::upsert_upstream(
                conn,
                &upstream_input("up_quota_only", "DeepSeek pool", "https://api.deepseek.com"),
            )?;
            assert!(dao::get_provider(conn, &created.id)?.is_none());
            let loaded = load_quota_provider(conn, &created.id)?;
            assert_eq!(loaded.id, created.id);
            assert_eq!(loaded.name, "DeepSeek pool");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn load_quota_provider_prefers_provider_row() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let mut input =
                upstream_input("shared_quota_id", "Provider card", "https://api.deepseek.com");
            input.name = "Provider card".into();
            let provider = dao::upsert_provider(conn, &input)?;
            let mut pool = upstream_input(&provider.id, "Upstream snapshot", "https://api.deepseek.com");
            pool.name = "Upstream snapshot".into();
            dao::gateway::upsert_upstream(conn, &pool)?;
            let loaded = load_quota_provider(conn, &provider.id)?;
            assert_eq!(loaded.name, "Provider card");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn load_quota_provider_missing_id_is_config_error() {
        let db = Database::memory().unwrap();
        db.with_conn(|conn| {
            let err = load_quota_provider(conn, "missing_quota_id").unwrap_err();
            match err {
                AppError::Config(message) => {
                    assert!(message.contains("missing_quota_id"), "{message}");
                }
                other => panic!("expected Config, got {other:?}"),
            }
            Ok(())
        })
        .unwrap();
    }
}
