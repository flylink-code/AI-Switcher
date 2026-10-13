//! One-line, content-free summary of an outbound Gemini request.
//!
//! Stored with upstream 429 diagnostics so a failing client request can be
//! compared with a passing one without keeping any prompt text.

use serde_json::Value;

fn text_chars(parts: &[Value]) -> usize {
    parts
        .iter()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .map(|text| text.chars().count())
        .sum()
}

pub fn summarize(request: &Value) -> String {
    let contents = request
        .get("contents")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut chars = 0usize;
    let mut calls = 0usize;
    let mut responses = 0usize;
    let mut media_parts = 0usize;
    let mut media_bytes = 0usize;
    for content in contents {
        let parts = content
            .get("parts")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        chars += text_chars(parts);
        for part in parts {
            if part.get("functionCall").is_some() {
                calls += 1;
            }
            if part.get("functionResponse").is_some() {
                responses += 1;
            }
            if let Some(data) = part.pointer("/inlineData/data").and_then(Value::as_str) {
                media_parts += 1;
                media_bytes += data.len();
            }
        }
    }
    let system_chars = request
        .pointer("/systemInstruction/parts")
        .and_then(Value::as_array)
        .map(|parts| text_chars(parts))
        .unwrap_or(0);
    let declarations = request
        .pointer("/tools/0/functionDeclarations")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let tool_bytes: usize = declarations
        .iter()
        .map(|declaration| declaration.to_string().len())
        .sum();
    let generation = request.get("generationConfig");
    let number = |pointer: &str| {
        generation
            .and_then(|config| config.pointer(pointer))
            .and_then(Value::as_u64)
            .map_or_else(|| "-".to_string(), |value| value.to_string())
    };
    let thoughts = generation
        .and_then(|config| config.pointer("/thinkingConfig/includeThoughts"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    format!(
        "shape: contents={} chars={chars} system_chars={system_chars} tools={} tool_bytes={tool_bytes} calls={calls} results={responses} media={media_parts}/{media_bytes}B maxOut={} budget={} thoughts={thoughts}",
        contents.len(),
        declarations.len(),
        number("/maxOutputTokens"),
        number("/thinkingConfig/thinkingBudget"),
    )
}

#[cfg(test)]
mod tests {
    use super::summarize;
    use serde_json::json;

    #[test]
    fn summary_counts_without_leaking_text() {
        let request = json!({
            "systemInstruction": { "parts": [{ "text": "secret system" }] },
            "contents": [
                { "role": "user", "parts": [{ "text": "hello secret" }] },
                { "role": "model", "parts": [{ "functionCall": { "name": "t", "args": {} } }] },
                { "role": "user", "parts": [
                    { "functionResponse": { "name": "t", "response": {} } },
                    { "inlineData": { "mimeType": "image/png", "data": "AAAA" } }
                ] }
            ],
            "tools": [{ "functionDeclarations": [{ "name": "t", "parameters": {} }] }],
            "generationConfig": {
                "maxOutputTokens": 64000,
                "thinkingConfig": { "thinkingBudget": 24576, "includeThoughts": true }
            }
        });
        let line = summarize(&request);
        assert!(line.contains("contents=3"), "{line}");
        assert!(line.contains("chars=12"), "{line}");
        assert!(line.contains("system_chars=13"), "{line}");
        assert!(line.contains("tools=1"), "{line}");
        assert!(line.contains("calls=1 results=1 media=1/4B"), "{line}");
        assert!(
            line.contains("maxOut=64000 budget=24576 thoughts=true"),
            "{line}"
        );
        assert!(!line.contains("secret"), "{line}");
    }

    #[test]
    fn summary_handles_empty_request() {
        let line = summarize(&json!({}));
        assert!(line.contains("contents=0"), "{line}");
        assert!(line.contains("maxOut=- budget=-"), "{line}");
    }
}
