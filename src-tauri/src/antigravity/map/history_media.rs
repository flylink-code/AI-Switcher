//! Split markdown `data:` images out of history text into Gemini inlineData.

use serde_json::{json, Value};

pub fn parts_from_text(text: &str) -> Vec<Value> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some((before, mime, data, after)) = next_data_image(rest) {
        if !before.is_empty() {
            parts.push(json!({ "text": before }));
        }
        parts.push(json!({
            "inlineData": {
                "mimeType": mime,
                "data": data,
            }
        }));
        rest = after;
    }
    if !rest.is_empty() || parts.is_empty() {
        if !rest.is_empty() {
            parts.push(json!({ "text": rest }));
        } else if parts.is_empty() && !text.is_empty() {
            parts.push(json!({ "text": text }));
        }
    }
    parts
}

fn next_data_image(text: &str) -> Option<(&str, String, String, &str)> {
    let start = text.find("data:image")?;
    let markdown_start = text[..start].rfind("![");
    let prefix_end = markdown_start.unwrap_or(start);
    let before = &text[..prefix_end];
    let payload = &text[start..];
    let mime_end = payload.find(";base64,")?;
    let mime = payload["data:".len()..mime_end].to_string();
    if !mime.starts_with("image/") {
        return None;
    }
    let data_start = mime_end + ";base64,".len();
    let data_body = &payload[data_start..];
    let data_end = data_body
        .find(|c: char| c == ')' || c == '"' || c == '\'' || c.is_whitespace())
        .unwrap_or(data_body.len());
    let data = data_body[..data_end].to_string();
    let mut after = &data_body[data_end..];
    if after.starts_with(')') {
        after = &after[1..];
    }
    if after.starts_with(']') {
        after = &after[1..];
    }
    Some((before, mime, data, after))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_markdown_data_uri() {
        let text = "see ![shot](data:image/png;base64,QUJD) end";
        let parts = parts_from_text(text);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0]["text"], "see ");
        assert_eq!(parts[1]["inlineData"]["mimeType"], "image/png");
        assert_eq!(parts[1]["inlineData"]["data"], "QUJD");
        assert_eq!(parts[2]["text"], " end");
    }

    #[test]
    fn plain_text_stays_one_part() {
        let parts = parts_from_text("hello");
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "hello");
    }
}
