//! Fuzzy model-name matching for usage pricing.
//!
//! Supports exact, prefix, and hyphen/underscore token containment so catalog
//! entries like `kimi-k3` can price log models like `k3` (and the reverse).

use crate::database::dao::proxy_logs::ModelPricing;

/// Resolve the best pricing row for a request/log model name.
pub fn find_pricing_for_model<'a>(
    pricing: &'a [ModelPricing],
    model: &str,
) -> Option<&'a ModelPricing> {
    let normalized = normalize_model_key(model);
    if normalized.is_empty() || pricing.is_empty() {
        return None;
    }

    if let Some(exact) = pricing
        .iter()
        .find(|entry| normalize_model_key(&entry.model) == normalized)
    {
        return Some(exact);
    }

    let mut candidates: Vec<(&ModelPricing, u8, usize)> = pricing
        .iter()
        .filter_map(|entry| {
            let catalog = normalize_model_key(&entry.model);
            if catalog.is_empty() {
                return None;
            }
            let rank = match_rank(&normalized, &catalog)?;
            Some((entry, rank, catalog.len()))
        })
        .collect();

    // Prefer stronger match rank, then longer catalog keys (more specific).
    candidates.sort_by(|left, right| {
        left.1
            .cmp(&right.1)
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| left.0.model.cmp(&right.0.model))
    });
    candidates.into_iter().next().map(|(entry, _, _)| entry)
}

fn normalize_model_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn model_tokens(value: &str) -> Vec<&str> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect()
}

/// Lower rank is better. Rejects weak single-character token matches.
fn match_rank(request: &str, catalog: &str) -> Option<u8> {
    if request == catalog {
        return Some(0);
    }
    if request.starts_with(catalog) || catalog.starts_with(request) {
        // Avoid matching tiny fragments like "3" or "k".
        let shorter = request.len().min(catalog.len());
        if shorter < 2 {
            return None;
        }
        return Some(1);
    }
    let request_tokens = model_tokens(request);
    let catalog_tokens = model_tokens(catalog);
    if token_sequence_contains(&request_tokens, &catalog_tokens)
        || token_sequence_contains(&catalog_tokens, &request_tokens)
    {
        let shorter = request_tokens.len().min(catalog_tokens.len());
        let shortest_token_len = request_tokens
            .iter()
            .chain(catalog_tokens.iter())
            .map(|token| token.len())
            .min()
            .unwrap_or(0);
        // Require at least one meaningful token (≥2 chars) in the shorter side.
        if shorter == 0 || shortest_token_len < 2 && shorter == 1 {
            return None;
        }
        if shorter == 1 {
            let only = if request_tokens.len() <= catalog_tokens.len() {
                request_tokens[0]
            } else {
                catalog_tokens[0]
            };
            if only.len() < 2 {
                return None;
            }
        }
        return Some(2);
    }
    None
}

fn token_sequence_contains(haystack: &[&str], needle: &[&str]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pricing(model: &str) -> ModelPricing {
        ModelPricing {
            model: model.to_string(),
            provider: "test".into(),
            input_price_per_million: 1.0,
            cache_read_price_per_million: 0.0,
            cache_write_price_per_million: 0.0,
            output_price_per_million: 1.0,
            batch_input_price_per_million: 0.0,
            batch_output_price_per_million: 0.0,
            currency: "USD".into(),
            source_url: String::new(),
            effective_date: String::new(),
            is_default: false,
        }
    }

    #[test]
    fn matches_kimi_alias_either_direction() {
        let catalog = vec![pricing("kimi-k3")];
        assert_eq!(
            find_pricing_for_model(&catalog, "k3").map(|row| row.model.as_str()),
            Some("kimi-k3")
        );
        let catalog = vec![pricing("k3")];
        assert_eq!(
            find_pricing_for_model(&catalog, "kimi-k3").map(|row| row.model.as_str()),
            Some("k3")
        );
    }

    #[test]
    fn prefers_longer_prefix_match() {
        let catalog = vec![pricing("claude-opus-5"), pricing("claude-opus-5-fast")];
        assert_eq!(
            find_pricing_for_model(&catalog, "claude-opus-5-fast")
                .map(|row| row.model.as_str()),
            Some("claude-opus-5-fast")
        );
    }

    #[test]
    fn rejects_weak_single_char_tokens() {
        let catalog = vec![pricing("gpt-5"), pricing("k3")];
        assert!(find_pricing_for_model(&catalog, "5").is_none());
    }
}
