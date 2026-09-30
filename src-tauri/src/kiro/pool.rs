//! Round-robin Kiro accounts. Monthly quota disables a credential.
//! Account-level throttling cools only that credential. Network errors do not rotate.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::account::{store, KiroAccount};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureClass {
    /// Rotate to the next account. Do not cool this one.
    Rotate,
    /// This account is rate-limited. Cool it, then rotate.
    CoolAndRotate,
    /// Monthly quota is gone.
    Disable,
    /// The request body is invalid. Do not retry.
    Client,
    /// Transport failure. Keep the same account.
    Network,
    /// Token was rejected after the one allowed refresh.
    Auth,
}

pub struct AccountPool {
    cursor: AtomicUsize,
}

impl AccountPool {
    pub fn new() -> Self {
        Self {
            cursor: AtomicUsize::new(0),
        }
    }

    pub fn select(&self) -> Option<KiroAccount> {
        let accounts = store().selectable();
        if accounts.is_empty() {
            return None;
        }
        let index = self.cursor.fetch_add(1, Ordering::Relaxed) % accounts.len();
        accounts.get(index).cloned()
    }

    pub fn note_failure(&self, account_id: &str, class: FailureClass) {
        match class {
            FailureClass::Rotate | FailureClass::Network | FailureClass::Client | FailureClass::Auth => {}
            FailureClass::CoolAndRotate => {
                let _ = store().mark_cooldown(account_id, 60_000);
            }
            FailureClass::Disable => {
                let _ = store().mark_disabled(account_id, "月度额度已用尽");
            }
        }
    }
}

impl Default for AccountPool {
    fn default() -> Self {
        Self::new()
    }
}

pub fn classify_upstream(status: u16, body: &str, network: bool) -> FailureClass {
    if network {
        return FailureClass::Network;
    }
    if is_monthly_limit(body) {
        return FailureClass::Disable;
    }
    if is_client_validation(body) {
        return FailureClass::Client;
    }
    if status == 401 || body.contains("The bearer token included in the request is invalid") {
        return FailureClass::Auth;
    }
    if status == 429 && body.to_ascii_lowercase().contains("suspicious activity") {
        return FailureClass::CoolAndRotate;
    }
    if status == 429 || status == 504 || (500..600).contains(&status) {
        return FailureClass::Rotate;
    }
    FailureClass::Client
}

pub fn is_monthly_limit(body: &str) -> bool {
    const REASONS: [&str; 2] = ["MONTHLY_REQUEST_COUNT", "OVERAGE_REQUEST_LIMIT_EXCEEDED"];
    if !REASONS.iter().any(|reason| body.contains(reason)) {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .map(|value| {
            let top = value.get("reason").and_then(|item| item.as_str());
            let nested = value.pointer("/error/reason").and_then(|item| item.as_str());
            [top, nested]
                .into_iter()
                .flatten()
                .any(|reason| REASONS.contains(&reason))
        })
        .unwrap_or(true)
}

fn is_client_validation(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("improperly formed request") || lower.contains("validationexception")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monthly_limit_disables_and_network_does_not_rotate() {
        assert_eq!(
            classify_upstream(400, r#"{"reason":"MONTHLY_REQUEST_COUNT"}"#, false),
            FailureClass::Disable
        );
        assert_eq!(classify_upstream(0, "", true), FailureClass::Network);
        assert_eq!(
            classify_upstream(429, "suspicious activity on this account", false),
            FailureClass::CoolAndRotate
        );
        assert_eq!(classify_upstream(429, "high traffic", false), FailureClass::Rotate);
        assert_eq!(
            classify_upstream(400, "Improperly formed request", false),
            FailureClass::Client
        );
    }
}
