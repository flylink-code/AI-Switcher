//! Provider quota and balance detector.
//!
//! Automatically inspects a Provider's `base_url` to determine the suitable
//! quota or balance inquiry strategy.

use crate::gateway::{is_self_referential_upstream, reserved_listener_ports};
use crate::provider::{Provider, ProviderKind, ProviderTarget};
use crate::quota::balance::*;
use crate::quota::coding_plan::*;
use crate::quota::official::*;
use crate::quota::sub2api;
use crate::quota::types::*;

/// Local listeners and managed gateway cards cannot be probed as vendor APIs.
fn quota_probe_skip_reason(provider: &Provider) -> Option<String> {
    match provider.provider_kind {
        ProviderKind::Antigravity | ProviderKind::SmartGateway => {
            Some("本地网关不支持供应商额度查询".to_string())
        }
        _ if is_self_referential_upstream(&provider.base_url, &reserved_listener_ports()) => {
            Some("本机监听口不支持供应商额度查询".to_string())
        }
        _ => None,
    }
}

/// Detect and query quota or balance for a custom Provider.
pub async fn query_provider_quota(provider: &Provider) -> ProviderQuotaResult {
    if let Some(reason) = quota_probe_skip_reason(provider) {
        return ProviderQuotaResult::Unsupported {
            reason: Some(reason),
        };
    }

    let api_key = provider.api_key.trim();
    if api_key.is_empty() {
        return ProviderQuotaResult::Unsupported {
            reason: Some("未配置 API Key".to_string()),
        };
    }

    let base_url = provider.base_url.to_lowercase();

    // 1. Coding Plans / Token Plans
    if base_url.contains("api.kimi.com/coding") {
        query_kimi_quota(api_key).await
    } else if base_url.contains("open.bigmodel.cn") || base_url.contains("bigmodel.cn") || base_url.contains("api.z.ai") {
        query_zhipu_quota(&provider.base_url, api_key).await
    } else if base_url.contains("api.minimaxi.com") || base_url.contains("api.minimax.io") {
        query_minimax_quota(&provider.base_url, api_key).await
    } else if base_url.contains("zenmux") {
        query_zenmux_quota(&provider.base_url, api_key).await
    }
    // 2. Pay-as-you-go Balances
    else if base_url.contains("api.deepseek.com") {
        query_deepseek_balance(api_key).await
    } else if base_url.contains("api.siliconflow.cn") || base_url.contains("api.siliconflow.com") {
        query_siliconflow_balance(&provider.base_url, api_key).await
    } else if base_url.contains("api.stepfun.com") || base_url.contains("api.stepfun.ai") {
        query_stepfun_balance(api_key).await
    } else if base_url.contains("openrouter.ai") {
        query_openrouter_balance(api_key).await
    } else if base_url.contains("api.novita.ai") {
        query_novita_balance(api_key).await
    } else {
        sub2api::query_sub2api_usage(&provider.base_url, api_key).await
    }
}

/// Query official account subscription quota for a given Agent target.
pub async fn query_official_quota(target: ProviderTarget) -> ProviderQuotaResult {
    match target {
        ProviderTarget::ClaudeCode | ProviderTarget::ClaudeDesktop => {
            query_claude_official_quota().await
        }
        ProviderTarget::Codex => {
            query_codex_official_quota(None).await
        }
        _ => ProviderQuotaResult::Unsupported {
            reason: Some("该 Agent 暂无官方订阅查询接口".to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ClaudeModelMapping, ProviderKind, ProviderTarget, ProtocolType};

    fn sample(kind: ProviderKind, base_url: &str) -> Provider {
        Provider {
            id: "up_test".into(),
            name: "Test".into(),
            base_url: base_url.into(),
            api_key: "sk-test".into(),
            api_key_set: true,
            model: "test-model".into(),
            model_context_window: None,
            auto_review_model_override: None,
            web_search_enabled: None,
            model_mapping: ClaudeModelMapping::default(),
            protocol_type: ProtocolType::Anthropic,
            provider_kind: kind,
            auth_binding: String::new(),
            target_app: ProviderTarget::ClaudeCode,
            notes: String::new(),
            sort_index: 0,
            failover_group: 0,
            failover_models: Vec::new(),
            hidden_models: Vec::new(),
            thinking_config: None,
            custom_headers: None,
            is_current: false,
            created_at: 0,
            health_status: None,
            health_checked_at: None,
            health_latency_ms: None,
        }
    }

    #[test]
    fn skips_antigravity_and_smart_gateway_kinds() {
        assert!(quota_probe_skip_reason(&sample(
            ProviderKind::Antigravity,
            "http://127.0.0.1:15830"
        ))
        .is_some());
        assert!(quota_probe_skip_reason(&sample(
            ProviderKind::SmartGateway,
            "http://127.0.0.1:15828"
        ))
        .is_some());
        assert!(quota_probe_skip_reason(&sample(
            ProviderKind::Standard,
            "https://api.deepseek.com"
        ))
        .is_none());
    }

    #[test]
    fn skips_reserved_listener_ports() {
        assert!(quota_probe_skip_reason(&sample(
            ProviderKind::Standard,
            "http://127.0.0.1:15828"
        ))
        .is_some());
        assert!(quota_probe_skip_reason(&sample(
            ProviderKind::Standard,
            "http://localhost:15830/v1"
        ))
        .is_some());
        assert!(quota_probe_skip_reason(&sample(
            ProviderKind::Standard,
            "https://api.kimi.com/coding"
        ))
        .is_none());
    }
}
