//! Provider quota and balance detector.
//!
//! Automatically inspects a Provider's `base_url` to determine the suitable
//! quota or balance inquiry strategy.

use crate::provider::{Provider, ProviderTarget};
use crate::quota::balance::*;
use crate::quota::coding_plan::*;
use crate::quota::official::*;
use crate::quota::sub2api;
use crate::quota::types::*;

/// Detect and query quota or balance for a custom Provider.
pub async fn query_provider_quota(provider: &Provider) -> ProviderQuotaResult {
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
