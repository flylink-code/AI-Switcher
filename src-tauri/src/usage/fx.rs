//! Approximate FX conversion for usage cost display.
//!
//! Pricing rows may be quoted in CNY/EUR/etc. Headline totals convert to USD
//! so mixed-currency usage can be summed. Rates are mid-market style estimates
//! for display only — not live quotes or accounting-grade FX.

/// Units of `currency` per 1 USD (approximate).
pub fn units_per_usd(currency: &str) -> Option<f64> {
    match normalize_currency(currency).as_str() {
        "USD" => Some(1.0),
        "CNY" | "RMB" => Some(7.25),
        "EUR" => Some(0.92),
        "GBP" => Some(0.79),
        "HKD" => Some(7.80),
        "JPY" => Some(150.0),
        "KRW" => Some(1_350.0),
        "SGD" => Some(1.34),
        "AUD" => Some(1.55),
        "CAD" => Some(1.38),
        "TWD" => Some(32.0),
        _ => None,
    }
}

pub fn normalize_currency(currency: &str) -> String {
    let trimmed = currency.trim();
    if trimmed.is_empty() {
        "USD".to_string()
    } else {
        trimmed.to_ascii_uppercase()
    }
}

/// Convert an amount in `currency` into USD. Unknown currencies fall back to 1:1
/// so costs are not silently dropped from the headline total.
pub fn to_usd(amount: f64, currency: &str) -> f64 {
    let rate = units_per_usd(currency).unwrap_or(1.0);
    if rate.abs() <= f64::EPSILON {
        amount
    } else {
        amount / rate
    }
}

/// Sum mixed-currency amounts as USD.
///
/// - Empty → USD 0
/// - Single currency → keep native amount/currency (no surprise conversion)
/// - Multiple currencies → convert each to USD and sum
pub fn summarize_costs_as_usd(
    amounts: &[(String, f64)],
) -> (String, f64) {
    let meaningful: Vec<_> = amounts
        .iter()
        .filter(|(_, amount)| amount.abs() > f64::EPSILON)
        .cloned()
        .collect();
    if meaningful.is_empty() {
        return ("USD".to_string(), 0.0);
    }
    if meaningful.len() == 1 {
        let (currency, amount) = &meaningful[0];
        return (normalize_currency(currency), *amount);
    }
    let usd: f64 = meaningful
        .iter()
        .map(|(currency, amount)| to_usd(*amount, currency))
        .sum();
    ("USD".to_string(), usd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cny_converts_to_usd() {
        let usd = to_usd(72.5, "CNY");
        assert!((usd - 10.0).abs() < 1e-9);
    }

    #[test]
    fn single_currency_keeps_native() {
        let (currency, amount) = summarize_costs_as_usd(&[("CNY".into(), 48.0)]);
        assert_eq!(currency, "CNY");
        assert!((amount - 48.0).abs() < f64::EPSILON);
    }

    #[test]
    fn multi_currency_sums_as_usd() {
        // 72.5 CNY = 10 USD + 2 USD = 12 USD
        let (currency, amount) =
            summarize_costs_as_usd(&[("CNY".into(), 72.5), ("USD".into(), 2.0)]);
        assert_eq!(currency, "USD");
        assert!((amount - 12.0).abs() < 1e-9);
    }
}
