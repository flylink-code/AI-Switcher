fn parse_search_results(html: &str, tool: &Value) -> Value {
    let mut results = Vec::new();
    let mut saw_result = false;
    for anchor in html.split("<a ").skip(1) {
        let Some((attrs, text)) = anchor.split_once('>') else { continue; };
        if !attrs.contains("result__a") { continue; }
        let Some(href) = attrs.split_once("href=\"").and_then(|(_, s)| s.split_once('"')).map(|(s, _)| s) else { continue; };
        let href = decode_html(href);
        let Ok(mut url) = url::Url::parse(&if href.starts_with("//") { format!("https:{href}") } else { href }) else { continue; };
        if url.host_str().is_some_and(|h| h == "duckduckgo.com" || h.ends_with(".duckduckgo.com")) {
            let target = url.query_pairs().find(|(key, _)| key == "uddg").map(|(_, value)| value.into_owned());
            if let Some(target) = target {
                if let Ok(parsed) = url::Url::parse(&target) { url = parsed; }
            }
        }
        if !matches!(url.scheme(), "http" | "https") { continue; }
        saw_result = true;
        let host = url.host_str().unwrap_or("");
        let matches = |domain: &Value| domain.as_str().is_some_and(|d| {
            let d = d.to_ascii_lowercase();
            host == d || host.ends_with(&format!(".{d}"))
        });
        if tool["blocked_domains"].as_array().is_some_and(|ds| ds.iter().any(matches))
            || tool["allowed_domains"].as_array().is_some_and(|ds| !ds.is_empty() && !ds.iter().any(matches)) { continue; }
        let title = decode_html(&strip_tags(text.split("</a>").next().unwrap_or("")));
        results.push(json!({"type":"web_search_result", "url":url.as_str(),
            "title":clip(&title, 4000), "encrypted_content":""}));
    }
    // A challenge page or changed markup must not masquerade as an empty search.
    if !saw_result && !html.contains("no-results") {
        search_error("unavailable")
    } else { Value::Array(results) }
}

fn decode_html(text: &str) -> String {
    text.replace("&quot;", "\"").replace("&#39;", "'").replace("&lt;", "<")
        .replace("&gt;", ">").replace("&amp;", "&")
}
