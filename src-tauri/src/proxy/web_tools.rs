//! Local Anthropic `web_search` / `web_fetch` helpers for the local proxy.

use serde_json::{json, Value};

pub fn rewrite_server_tools(request: &mut Value) {
    let Some(tools) = request.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };
    for tool in tools.iter_mut() {
        let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
        let kind = tool.get("type").and_then(Value::as_str).unwrap_or("");
        if is_web_search(name, kind) {
            *tool = json!({
                "name": "web_search",
                "description": "Search the public web. Returns titles, URLs, and snippets.",
                "input_schema": {
                    "type": "object",
                    "properties": { "query": { "type": "string" } },
                    "required": ["query"]
                }
            });
        } else if is_web_fetch(name, kind) {
            *tool = json!({
                "name": "web_fetch",
                "description": "Fetch a URL and return extracted text.",
                "input_schema": {
                    "type": "object",
                    "properties": { "url": { "type": "string" } },
                    "required": ["url"]
                }
            });
        }
    }
}

pub fn is_web_search(name: &str, kind: &str) -> bool {
    name.starts_with("web_search") || kind.starts_with("web_search")
}

pub fn is_web_fetch(name: &str, kind: &str) -> bool {
    name.starts_with("web_fetch") || kind.starts_with("web_fetch")
}

pub fn execute_web_search(query: &str) -> String {
    let query = query.trim();
    if query.is_empty() {
        return "No search query.".into();
    }
    let encoded = urlencoding_lite(query);
    let url = format!("https://html.duckduckgo.com/html/?q={encoded}");
    match fetch_text(&url) {
        Ok(html) => format!("Search results for {query}:\n{}", clip(&strip_tags(&html), 4000)),
        Err(error) => format!("web_search failed: {error}"),
    }
}

pub fn execute_web_fetch(url: &str) -> String {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return "web_fetch only allows http(s) URLs.".into();
    }
    match fetch_text(url) {
        Ok(body) => clip(&strip_tags(&body), 8000),
        Err(error) => format!("web_fetch failed: {error}"),
    }
}

/// If the model asked for web_search/web_fetch, run it locally and replace
/// `tool_use` blocks with text so Claude Code does not wait on a server tool.
pub fn materialize_web_tool_uses(response: &mut Value) -> bool {
    let Some(content) = response.get_mut("content").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for block in content.iter_mut() {
        if block.get("type").and_then(Value::as_str) != Some("tool_use") {
            continue;
        }
        let name = block.get("name").and_then(Value::as_str).unwrap_or("");
        if is_web_search(name, "") {
            let query = block
                .pointer("/input/query")
                .and_then(Value::as_str)
                .unwrap_or("");
            *block = json!({ "type": "text", "text": execute_web_search(query) });
            changed = true;
        } else if is_web_fetch(name, "") {
            let url = block
                .pointer("/input/url")
                .and_then(Value::as_str)
                .unwrap_or("");
            *block = json!({ "type": "text", "text": execute_web_fetch(url) });
            changed = true;
        }
    }
    changed
}

fn fetch_text(url: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .user_agent("AI-Switcher/1.3")
        .build()
        .map_err(|error| error.to_string())?;
    let response = client.get(url).send().map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    response.text().map_err(|error| error.to_string())
}

fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        text.to_string()
    } else {
        format!("{}…", &text[..max])
    }
}

fn urlencoding_lite(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_server_search_tool() {
        let mut request = json!({
            "tools": [{ "type": "web_search_20250305", "name": "web_search" }]
        });
        rewrite_server_tools(&mut request);
        assert_eq!(request["tools"][0]["name"], "web_search");
        assert!(request["tools"][0].get("input_schema").is_some());
    }
}
