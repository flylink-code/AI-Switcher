//! Refresh Kiro access tokens. Social and Builder ID use different hosts.

use serde::Deserialize;
use serde::Serialize;

use crate::error::{AppError, AppResult};

use super::account::{expires_after, store, KiroAccount};
use super::outbound::build_async_client;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SocialRefresh {
    refresh_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SocialRefreshResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    profile_arn: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IdcRefresh {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    grant_type: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdcRefreshResponse {
    access_token: String,
    #[serde(default, alias = "refresh_token")]
    refresh_token: Option<String>,
    #[serde(default, alias = "expires_in")]
    expires_in: Option<i64>,
}

pub async fn ensure_access_token(account: &KiroAccount, force: bool) -> AppResult<KiroAccount> {
    if !force && super::account::access_token_fresh(account) {
        return Ok(account.clone());
    }
    refresh(account).await
}

async fn refresh(account: &KiroAccount) -> AppResult<KiroAccount> {
    if account.refresh_token.trim().is_empty() {
        return Err(AppError::Config("Kiro 账号没有 refreshToken，需要重新登录".into()));
    }
    let client = build_async_client(20);
    let result = if account.auth_method == "social" {
        refresh_social(&client, account).await
    } else {
        refresh_idc(&client, account).await
    };
    match result {
        Ok(updated) => {
            store().update_tokens(
                &updated.id,
                updated.access_token.clone(),
                Some(updated.refresh_token.clone()),
                updated.expires_at.clone(),
                Some(updated.profile_arn.clone()),
            )?;
            Ok(updated)
        }
        Err(error) => Err(error),
    }
}

async fn refresh_social(client: &reqwest::Client, account: &KiroAccount) -> AppResult<KiroAccount> {
    let region = account.region.trim();
    let url = format!("https://prod.{region}.auth.desktop.kiro.dev/refreshToken");
    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .header(
            "user-agent",
            format!("KiroIDE-0.7.0-{}", account.machine_id),
        )
        .json(&SocialRefresh {
            refresh_token: account.refresh_token.clone(),
        })
        .send()
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(refresh_error(status.as_u16(), &body));
    }
    let parsed: SocialRefreshResponse = serde_json::from_str(&body)?;
    let mut updated = account.clone();
    updated.access_token = parsed.access_token;
    if let Some(refresh) = parsed.refresh_token.filter(|value| !value.is_empty()) {
        updated.refresh_token = refresh;
    }
    if let Some(arn) = parsed.profile_arn.filter(|value| !value.is_empty()) {
        updated.profile_arn = arn;
    }
    if let Some(seconds) = parsed.expires_in {
        updated.expires_at = Some(expires_after(seconds));
    }
    Ok(updated)
}

async fn refresh_idc(client: &reqwest::Client, account: &KiroAccount) -> AppResult<KiroAccount> {
    if account.client_id.trim().is_empty() || account.client_secret.trim().is_empty() {
        return Err(AppError::Config("Builder ID 刷新需要 clientId 和 clientSecret".into()));
    }
    let region = account.region.trim();
    let url = format!("https://oidc.{region}.amazonaws.com/token");
    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&IdcRefresh {
            client_id: account.client_id.clone(),
            client_secret: account.client_secret.clone(),
            refresh_token: account.refresh_token.clone(),
            grant_type: "refresh_token".to_string(),
        })
        .send()
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(refresh_error(status.as_u16(), &body));
    }
    let parsed: IdcRefreshResponse = serde_json::from_str(&body)?;
    let mut updated = account.clone();
    updated.access_token = parsed.access_token;
    if let Some(refresh) = parsed.refresh_token.filter(|value| !value.is_empty()) {
        updated.refresh_token = refresh;
    }
    if let Some(seconds) = parsed.expires_in {
        updated.expires_at = Some(expires_after(seconds));
    }
    Ok(updated)
}

fn refresh_error(status: u16, body: &str) -> AppError {
    if status == 400 && body.contains("invalid_grant") {
        AppError::Config("Kiro refreshToken 已失效，需要重新登录".into())
    } else if status == 401 {
        AppError::Config("Kiro 鉴权失败".into())
    } else {
        AppError::Other(format!("刷新 Kiro token 失败 HTTP {status}"))
    }
}

/// A 401 may refresh once. A second 401 is an authentication error.
pub fn should_force_refresh(status: u16, body: &str, already_refreshed: bool) -> bool {
    if already_refreshed {
        return false;
    }
    status == 401 || body.contains("The bearer token included in the request is invalid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_happens_once_per_request() {
        assert!(should_force_refresh(401, "", false));
        assert!(!should_force_refresh(401, "", true));
        assert!(should_force_refresh(
            403,
            "The bearer token included in the request is invalid",
            false
        ));
        assert!(!should_force_refresh(429, "slow down", false));
    }
}
