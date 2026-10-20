//! Local Anthropic `web_search` / `web_fetch` helpers for the local proxy.

use serde_json::{json, Value};

pub fn rewrite_server_tools(request: &mut Value) {
    let Some(tools) = request.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };
    for tool in tools.iter_mut() {
        let name = tool.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        let kind = tool.get("type").and_then(Value::as_str).unwrap_or("").to_string();
        if is_web_search(&name, &kind) {
            *tool = json!({
                "name": "web_search",
                "description": "Search the public web. Returns titles, URLs, and snippets.",
                "input_schema": {
                    "type": "object",
                    "properties": { "query": { "type": "string" } },
                    "required": ["query"]
                }
            });
        } else if is_web_fetch(&name, &kind) {
            *tool = json!({
                "name": "web_fetch",
                "description": "Fetch a URL and return extracted text.",
                "input_schema": {
                    "type": "object",
                    "properties": { "url": { "type": "string" } },
                    "required": ["url"]
                }
            });
        } else if kind.starts_with("bash_") {
            let resolved_name = if name.is_empty() { "bash" } else { &name };
            *tool = json!({
                "name": resolved_name,
                "description": tool.get("description").and_then(Value::as_str).unwrap_or("Execute a bash command in a persistent shell session."),
                "input_schema": tool.get("input_schema").cloned().unwrap_or_else(|| json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "The bash command to run." },
                        "restart": { "type": "boolean", "description": "Restart the bash session." }
                    }
                }))
            });
        } else if kind.starts_with("text_editor_") {
            let resolved_name = if name.is_empty() { "str_replace_editor" } else { &name };
            *tool = json!({
                "name": resolved_name,
                "description": tool.get("description").and_then(Value::as_str).unwrap_or("View, create, and edit files."),
                "input_schema": tool.get("input_schema").cloned().unwrap_or_else(|| json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "enum": ["view", "create", "str_replace", "insert", "undo_edit"] },
                        "path": { "type": "string" },
                        "file_text": { "type": "string" },
                        "old_str": { "type": "string" },
                        "new_str": { "type": "string" },
                        "insert_line": { "type": "integer" },
                        "view_range": { "type": "array", "items": { "type": "integer" } }
                    },
                    "required": ["command", "path"]
                }))
            });
        } else if kind.starts_with("computer_") {
            let resolved_name = if name.is_empty() { "computer" } else { &name };
            *tool = json!({
                "name": resolved_name,
                "description": tool.get("description").and_then(Value::as_str).unwrap_or("Interact with the desktop screen, mouse, and keyboard."),
                "input_schema": tool.get("input_schema").cloned().unwrap_or_else(|| json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string" },
                        "coordinate": { "type": "array", "items": { "type": "integer" } },
                        "text": { "type": "string" }
                    },
                    "required": ["action"]
                }))
            });
        } else if super::convert::is_claude_server_tool_type(&kind) {
            let resolved_name = if name.is_empty() { &kind } else { &name };
            *tool = json!({
                "name": resolved_name,
                "description": tool.get("description").and_then(Value::as_str).unwrap_or(""),
                "input_schema": tool.get("input_schema").cloned().unwrap_or_else(|| json!({
                    "type": "object",
                    "properties": {}
                }))
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

pub fn server_web_tools(request: &Value) -> Vec<Value> {
    request.get("tools").and_then(Value::as_array).into_iter().flatten()
        .filter(|tool| tool.get("type").and_then(Value::as_str)
            .is_some_and(|kind| kind.starts_with("web_search_")))
        .cloned().collect()
}

/// Execute only declared server tools; client-owned tools remain client-owned.
pub async fn materialize_web_tool_uses(
    response: &mut Value, tools: &[Value],
) -> bool {
    let Some(content) = response.get_mut("content").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    let mut output = Vec::new();
    for mut block in std::mem::take(content) {
        if block["type"] == "tool_use"
            && is_web_fetch(block["name"].as_str().unwrap_or(""), "") {
            let url = block.pointer("/input/url").and_then(Value::as_str).unwrap_or("");
            output.push(json!({"type":"text", "text":execute_web_fetch(url).await}));
            changed = true;
            continue;
        }
        let tool = tools.iter().find(|tool| tool["name"] == block["name"]);
        if block["type"] != "tool_use" || tool.is_none() {
            output.push(block);
            continue;
        }
        let tool = tool.unwrap();
        let result = execute_web_search(
            block.pointer("/input/query").and_then(Value::as_str).unwrap_or(""), tool,
        ).await;
        let id = block["id"].clone();
        block["type"] = json!("server_tool_use");
        output.push(block);
        output.push(json!({
            "type":"web_search_tool_result",
            "tool_use_id":id, "content":result
        }));
        changed = true;
    }
    *content = output;
    if changed && response["stop_reason"] == "tool_use"
        && !response["content"].as_array().unwrap().iter().any(|b| b["type"] == "tool_use") {
        response["stop_reason"] = json!("end_turn");
    }
    changed
}

async fn execute_web_search(query: &str, tool: &Value) -> Value {
    let mut query = query.trim().to_string();
    if query.is_empty() { return search_error("invalid_input"); }
    if let Some(domains) = tool["allowed_domains"].as_array() {
        let sites: Vec<_> = domains.iter().filter_map(Value::as_str).map(|s| format!("site:{s}")).collect();
        if !sites.is_empty() { query.push_str(&format!(" ({})", sites.join(" OR "))); }
    }
    if let Some(domains) = tool["blocked_domains"].as_array() {
        for domain in domains.iter().filter_map(Value::as_str) { query.push_str(&format!(" -site:{domain}")); }
    }
    let url = format!("https://html.duckduckgo.com/html/?q={}", urlencoding_lite(&query));
    match fetch_text(&url).await {
        Ok(html) => parse_search_results(&html, tool),
        Err(error) => {
            log::warn!("web_search failed: {error}");
            search_error("unavailable")
        }
    }
}

fn search_error(code: &str) -> Value {
    json!({"type":"web_search_tool_result_error", "error_code":code})
}

async fn execute_web_fetch(url: &str) -> String {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return "web_fetch only allows http(s) URLs.".into();
    }
    match fetch_text(url).await {
        Ok(body) => clip(&strip_tags(&body), 8000),
        Err(error) => format!("web_fetch failed: {error}"),
    }
}

async fn fetch_text(url: &str) -> Result<String, String> {
    let client = crate::system_proxy::apply_to_builder(reqwest::Client::builder())
        .build().map_err(|error| error.to_string())?;
    let response = client.get(url).timeout(std::time::Duration::from_secs(12))
        .header(reqwest::header::USER_AGENT, "AI-Switcher/1.3")
        .send().await.map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    response.text().await.map_err(|error| error.to_string())
}

include!("web_tools_results.rs");
include!("web_tools_stream.rs");

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
        let mut end = max;
        while !text.is_char_boundary(end) { end -= 1; }
        format!("{}…", &text[..end])
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
    include!("web_tools_tests.rs");

    #[test]
    fn rewrites_server_search_tool() {
        let mut request = json!({
            "tools": [
                { "type": "web_search_20250305", "name": "web_search" },
                { "type": "bash_20250124", "name": "bash" },
                { "type": "text_editor_20250124", "name": "str_replace_editor" }
            ]
        });
        rewrite_server_tools(&mut request);
        assert_eq!(request["tools"][0]["name"], "web_search");
        assert!(request["tools"][0].get("input_schema").is_some());
        assert_eq!(request["tools"][1]["name"], "bash");
        assert!(request["tools"][1].get("type").is_none());
        assert!(request["tools"][1]["input_schema"]["properties"]["command"].is_object());
        assert_eq!(request["tools"][2]["name"], "str_replace_editor");
        assert!(request["tools"][2].get("type").is_none());
        assert!(request["tools"][2]["input_schema"]["properties"]["path"].is_object());
    }
}
