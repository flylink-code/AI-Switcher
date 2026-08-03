//! Redact secrets and credential-bearing URLs before they reach logs or diagnostics.

/// Scrub common credential patterns and URL userinfo from diagnostic text.
pub fn redact_secrets(value: &str) -> String {
    let mut result = value
        .chars()
        .filter(|character| !character.is_control() || *character == ' ')
        .take(2_000)
        .collect::<String>();

    for marker in [
        "Bearer ",
        "bearer ",
        "sk-ant-",
        "sk-",
        "api_key=",
        "apiKey=",
        "x-api-key=",
        "X-Api-Key=",
        "refresh_token=",
        "access_token=",
        "client_secret=",
        "authorization=",
        "Authorization: ",
    ] {
        while let Some(start) = result.find(marker) {
            let token_start = start + marker.len();
            let token_len = result[token_start..]
                .find(|character: char| {
                    character.is_whitespace()
                        || matches!(character, ',' | ';' | '"' | '\'' | '&' | '}' | ']')
                })
                .unwrap_or(result.len().saturating_sub(token_start));
            result.replace_range(start..token_start + token_len, "[redacted]");
        }
    }

    result = redact_url_userinfo(&result);
    result = redact_query_secrets(&result);
    if result.len() > 500 {
        result.truncate(500);
        result.push('…');
    }
    result
}

fn redact_url_userinfo(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(scheme_at) = rest.find("://") {
        let before = &rest[..scheme_at + 3];
        output.push_str(before);
        let after_scheme = &rest[scheme_at + 3..];
        if let Some(at) = after_scheme.find('@') {
            let creds = &after_scheme[..at];
            if creds.contains(':') && !creds.contains('/') {
                output.push_str("[redacted]:[redacted]@");
                rest = &after_scheme[at + 1..];
                continue;
            }
        }
        rest = after_scheme;
    }
    output.push_str(rest);
    output
}

fn redact_query_secrets(value: &str) -> String {
    let keys = [
        "key=",
        "token=",
        "secret=",
        "password=",
        "api_key=",
        "access_token=",
        "refresh_token=",
    ];
    let mut result = value.to_string();
    for key in keys {
        let mut search_from = 0usize;
        while let Some(rel) = result[search_from..].to_ascii_lowercase().find(key) {
            let start = search_from + rel;
            let prefix = result.as_bytes().get(start.wrapping_sub(1)).copied();
            if start > 0 && !matches!(prefix, Some(b'?') | Some(b'&')) {
                search_from = start + key.len();
                continue;
            }
            let value_start = start + key.len();
            let value_len = result[value_start..]
                .find(|character: char| character.is_whitespace() || character == '&')
                .unwrap_or(result.len().saturating_sub(value_start));
            result.replace_range(value_start..value_start + value_len, "[redacted]");
            search_from = value_start + "[redacted]".len();
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_bearer_and_sk_tokens() {
        let text = redact_secrets("Authorization: Bearer secret-token-value failed sk-live-abc123 end");
        assert!(!text.contains("secret-token-value"));
        assert!(!text.contains("sk-live-abc123"));
        assert!(text.contains("[redacted]"));
    }

    #[test]
    fn redacts_url_userinfo_and_query() {
        let text = redact_secrets(
            "upstream https://user:pass@api.example.com/v1?api_key=supersecret&x=1",
        );
        assert!(!text.contains("user:pass"));
        assert!(!text.contains("supersecret"));
        assert!(text.contains("[redacted]"));
    }

    #[test]
    fn truncates_long_payloads() {
        let long = format!("ok {}", "a".repeat(800));
        let text = redact_secrets(&long);
        assert!(text.chars().count() <= 501);
        assert!(text.ends_with('…'));
    }
}
