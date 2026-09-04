//! Split markdown `data:` images out of history text into Gemini inlineData.

use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
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
    let mut search_from = 0usize;
    while search_from < text.len() {
        let Some(rel) = text[search_from..].find("data:image") else {
            return None;
        };
        let start = search_from + rel;
        if let Some(parsed) = try_parse_data_image_at(text, start) {
            return Some(parsed);
        }
        search_from = next_char_boundary(text, start + 1);
    }
    None
}

fn try_parse_data_image_at(text: &str, start: usize) -> Option<(&str, String, String, &str)> {
    let markdown_start = text[..start].rfind("![");
    let prefix_end = markdown_start.unwrap_or(start);
    let before = &text[..prefix_end];
    let payload = &text[start..];
    let mime_end = payload.find(";base64,")?;
    let mime = payload["data:".len()..mime_end].trim().to_string();
    if !mime.starts_with("image/") || mime.len() > 64 {
        return None;
    }
    if mime
        .chars()
        .any(|c| !c.is_ascii() || c.is_whitespace() || matches!(c, '`' | '<' | '>' | '"' | '\''))
    {
        return None;
    }
    let data_start = mime_end + ";base64,".len();
    let data_body = &payload[data_start..];
    let data_end = data_body
        .find(|c: char| !is_data_uri_base64_char(c))
        .unwrap_or(data_body.len());
    let data = &data_body[..data_end];
    if !is_plausible_image_base64(data) {
        return None;
    }
    let mut after = &data_body[data_end..];
    if after.starts_with(')') {
        after = &after[1..];
    }
    if after.starts_with(']') {
        after = &after[1..];
    }
    Some((before, mime, data.to_string(), after))
}

fn is_data_uri_base64_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '-' | '_')
}

pub(crate) fn is_plausible_image_base64(data: &str) -> bool {
    data.len() >= 4
        && (STANDARD.decode(data).is_ok()
            || STANDARD_NO_PAD.decode(data).is_ok()
            || URL_SAFE.decode(data).is_ok()
            || URL_SAFE_NO_PAD.decode(data).is_ok())
}

fn next_char_boundary(text: &str, mut index: usize) -> usize {
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
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

    #[test]
    fn chinese_after_fake_data_uri_stays_text() {
        let text = "说明 data:image/png;base64,`，实现前端 不要当图片";
        let parts = parts_from_text(text);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], text);
        assert!(parts[0].get("inlineData").is_none());
    }

    #[test]
    fn truncated_junk_base64_is_not_inlined() {
        let text = "![x](data:image/png;base64,???，实现前端)";
        let parts = parts_from_text(text);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], text);
    }

    #[test]
    fn valid_image_then_chinese_stays_split() {
        let text = "![shot](data:image/png;base64,QUJD)，实现前端";
        let parts = parts_from_text(text);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["inlineData"]["data"], "QUJD");
        assert_eq!(parts[1]["text"], "，实现前端");
    }
}
