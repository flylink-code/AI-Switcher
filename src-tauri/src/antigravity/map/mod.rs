//! Protocol mappers: Anthropic / OpenAI chat / Responses → Gemini → wire formats.

use serde_json::{json, Value};

pub mod anthropic;
pub mod args_fix;
pub mod history_media;
pub mod latex;
pub mod models;
pub mod openai;
pub mod responses;

pub use models::list_public_models;

const SYSTEM_REMINDER_START: &str = "<system-reminder>";
const SYSTEM_REMINDER_END: &str = "</system-reminder>";

fn text_parts(content: &Value) -> Vec<String> {
    match content {
        Value::String(text) if !text.trim().is_empty() => vec![text.clone()],
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .or_else(|| item.get("text").and_then(Value::as_str))
                    .filter(|text| !text.trim().is_empty())
                    .map(str::to_string)
            })
            .collect(),
        Value::Object(map) => map
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(|text| vec![text.to_string()])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn system_reminder_text(content: &Value) -> Option<String> {
    let text = text_parts(content).join("\n");
    (!text.trim().is_empty())
        .then(|| format!("{SYSTEM_REMINDER_START}\n{text}\n{SYSTEM_REMINDER_END}"))
}

/// 按原位置把中途 system/developer 降级为 user part；只合并紧邻 user。
fn push_system_reminder(contents: &mut Vec<Value>, content: &Value) {
    let Some(text) = system_reminder_text(content) else {
        return;
    };
    let part = json!({ "text": text });
    if let Some(parts) = contents
        .last_mut()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .and_then(|message| message.get_mut("parts"))
        .and_then(Value::as_array_mut)
    {
        parts.push(part);
    } else {
        contents.push(json!({ "role": "user", "parts": [part] }));
    }
}

/// Normalize Gemini `contents` so Cloud Code's Anthropic backend never rejects
/// multi-tool turns, mid-turn system reminders, or interrupted tool calls:
/// 1. Merge consecutive turns with the same `role`.
/// 2. Ensure every `functionCall` in a `model` turn has a matching
///    `functionResponse` at the front of the immediately following `user` turn.
/// 3. Strip orphan `functionResponse` parts without a preceding `functionCall`.
pub(crate) fn normalize_gemini_contents(contents: &mut Vec<Value>) {
    let mut merged: Vec<Value> = Vec::with_capacity(contents.len());
    for turn in contents.drain(..) {
        let role = turn
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .to_string();
        let parts = turn
            .get("parts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if parts.is_empty() {
            continue;
        }
        if let Some(last) = merged
            .last_mut()
            .filter(|m| m.get("role").and_then(Value::as_str) == Some(role.as_str()))
        {
            if let Some(existing_parts) = last.get_mut("parts").and_then(Value::as_array_mut) {
                existing_parts.extend(parts);
            }
        } else {
            merged.push(json!({ "role": role, "parts": parts }));
        }
    }

    let mut i = 0;
    while i < merged.len() {
        let role = merged[i]
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .to_string();
        if role == "model" {
            let parts = merged[i]
                .get("parts")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut thought_parts = Vec::new();
            let mut other_parts = Vec::new();
            let mut fc_parts = Vec::new();
            let mut expected_calls: Vec<(String, String)> = Vec::new();
            for p in parts {
                if p.get("thought").and_then(Value::as_bool).unwrap_or(false) {
                    thought_parts.push(p);
                } else if let Some(fc) = p.get("functionCall") {
                    let id = fc
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let name = fc
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("tool")
                        .to_string();
                    if id.is_empty() || !expected_calls.iter().any(|(eid, _)| eid == &id) {
                        expected_calls.push((id, name));
                        fc_parts.push(p);
                    }
                } else {
                    other_parts.push(p);
                }
            }
            let mut ordered = thought_parts;
            ordered.extend(other_parts);
            ordered.extend(fc_parts);
            merged[i]["parts"] = Value::Array(ordered);

            if !expected_calls.is_empty() {
                let next_is_user = merged
                    .get(i + 1)
                    .and_then(|m| m.get("role"))
                    .and_then(Value::as_str)
                    == Some("user");
                if !next_is_user {
                    merged.insert(i + 1, json!({ "role": "user", "parts": [] }));
                }
                let next_parts = merged[i + 1]
                    .get("parts")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let mut fr_by_id: std::collections::HashMap<String, Value> =
                    std::collections::HashMap::new();
                let mut fr_without_id: Vec<Value> = Vec::new();
                let mut non_fr_parts: Vec<Value> = Vec::new();
                for p in next_parts {
                    if let Some(fr) = p.get("functionResponse") {
                        let id = fr
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        if !id.is_empty() {
                            fr_by_id.entry(id).or_insert(p);
                        } else {
                            fr_without_id.push(p);
                        }
                    } else {
                        non_fr_parts.push(p);
                    }
                }
                let mut final_next =
                    Vec::with_capacity(expected_calls.len() + non_fr_parts.len());
                let mut unid_iter = fr_without_id.into_iter();
                for (call_id, call_name) in &expected_calls {
                    if !call_id.is_empty() {
                        if let Some(matched) = fr_by_id.remove(call_id) {
                            final_next.push(matched);
                            continue;
                        }
                    } else if let Some(unid) = unid_iter.next() {
                        final_next.push(unid);
                        continue;
                    }
                    let mut fr = json!({
                        "name": call_name,
                        "response": { "result": "[Tool execution was cancelled or skipped before returning a result]" }
                    });
                    if !call_id.is_empty() {
                        fr["id"] = json!(call_id);
                    }
                    final_next.push(json!({ "functionResponse": fr }));
                }
                final_next.extend(non_fr_parts);
                merged[i + 1]["parts"] = Value::Array(final_next);
            }
        } else {
            let prev_has_fc = i > 0
                && merged[i - 1].get("role").and_then(Value::as_str) == Some("model")
                && merged[i - 1]
                    .get("parts")
                    .and_then(Value::as_array)
                    .is_some_and(|arr| arr.iter().any(|p| p.get("functionCall").is_some()));
            if !prev_has_fc {
                let parts = merged[i]
                    .get("parts")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let filtered: Vec<Value> = parts
                    .into_iter()
                    .filter(|p| p.get("functionResponse").is_none())
                    .collect();
                if filtered.is_empty() {
                    merged.remove(i);
                    continue;
                }
                merged[i]["parts"] = Value::Array(filtered);
            }
        }
        i += 1;
    }

    enforce_gemini_context_budget(&mut merged);
    *contents = merged;
}

const MAX_GEMINI_CONTEXT_CHARS: usize = 2_400_000;

pub(crate) fn truncate_middle_chars(
    text: &str,
    max_chars: usize,
    head_chars: usize,
    tail_chars: usize,
) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    let total_chars = text.chars().count();
    if total_chars <= max_chars {
        return text.to_string();
    }
    let head: String = text.chars().take(head_chars).collect();
    let tail: String = text
        .chars()
        .rev()
        .take(tail_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let omitted = total_chars.saturating_sub(head_chars + tail_chars);
    format!("{head}\n\n... [{omitted} chars truncated to fit context window] ...\n\n{tail}")
}

fn estimate_part_chars(part: &Value) -> usize {
    if let Some(text) = part.get("text").and_then(Value::as_str) {
        return text.len();
    }
    if let Some(fr) = part.get("functionResponse") {
        return fr
            .pointer("/response/result")
            .and_then(Value::as_str)
            .map(str::len)
            .unwrap_or(64);
    }
    if let Some(fc) = part.get("functionCall") {
        return fc
            .get("args")
            .map(|v| v.to_string().len())
            .unwrap_or(64);
    }
    if let Some(inline) = part.get("inlineData") {
        return inline
            .get("data")
            .and_then(Value::as_str)
            .map(str::len)
            .unwrap_or(0);
    }
    64
}

fn estimate_contents_chars(contents: &[Value]) -> usize {
    contents
        .iter()
        .map(|turn| {
            turn.get("parts")
                .and_then(Value::as_array)
                .map(|parts| parts.iter().map(estimate_part_chars).sum::<usize>())
                .unwrap_or(0)
        })
        .sum()
}

/// Guard against Gemini's 1,048,576 input token limit by progressively compacting
/// historical tool outputs, inline media, and oversized text blocks while preserving
/// `functionCall` / `functionResponse` turn structure.
fn enforce_gemini_context_budget(contents: &mut [Value]) {
    let mut total = estimate_contents_chars(contents);
    if total <= MAX_GEMINI_CONTEXT_CHARS {
        return;
    }

    let contents_len = contents.len();
    let older_cutoff = contents_len.saturating_sub(2);
    // Pass 1: compact older functionResponse outputs and inlineData payloads.
    for idx in 0..older_cutoff {
        if total <= MAX_GEMINI_CONTEXT_CHARS {
            return;
        }
        let Some(parts) = contents[idx].get_mut("parts").and_then(Value::as_array_mut) else {
            continue;
        };
        for part in parts.iter_mut() {
            if total <= MAX_GEMINI_CONTEXT_CHARS {
                break;
            }
            if let Some(result_val) = part.pointer_mut("/functionResponse/response/result") {
                if let Some(text) = result_val.as_str() {
                    if text.len() > 4_000 {
                        let old_len = text.len();
                        let compacted = truncate_middle_chars(text, 4_000, 2_500, 1_000);
                        let new_len = compacted.len();
                        *result_val = Value::String(compacted);
                        total = total.saturating_sub(old_len.saturating_sub(new_len));
                    }
                }
            } else if part.get("inlineData").is_some() && idx + 4 < contents_len {
                let old_len = estimate_part_chars(part);
                *part = json!({ "text": "[Earlier inline image omitted to fit context window]" });
                total = total.saturating_sub(old_len.saturating_sub(64));
            }
        }
    }

    // Pass 2: compact older text parts if still over budget.
    for idx in 0..older_cutoff {
        if total <= MAX_GEMINI_CONTEXT_CHARS {
            return;
        }
        let Some(parts) = contents[idx].get_mut("parts").and_then(Value::as_array_mut) else {
            continue;
        };
        for part in parts.iter_mut() {
            if total <= MAX_GEMINI_CONTEXT_CHARS {
                break;
            }
            if let Some(text_val) = part.get_mut("text") {
                if let Some(text) = text_val.as_str() {
                    if text.len() > 4_000 {
                        let old_len = text.len();
                        let compacted = truncate_middle_chars(text, 4_000, 2_500, 1_000);
                        let new_len = compacted.len();
                        *text_val = Value::String(compacted);
                        total = total.saturating_sub(old_len.saturating_sub(new_len));
                    }
                }
            }
        }
    }

    // Pass 3: if recent turns alone exceed budget, cap individual giant parts in the tail turns.
    for idx in older_cutoff..contents.len() {
        if total <= MAX_GEMINI_CONTEXT_CHARS {
            return;
        }
        let Some(parts) = contents[idx].get_mut("parts").and_then(Value::as_array_mut) else {
            continue;
        };
        for part in parts.iter_mut() {
            if total <= MAX_GEMINI_CONTEXT_CHARS {
                break;
            }
            if let Some(result_val) = part.pointer_mut("/functionResponse/response/result") {
                if let Some(text) = result_val.as_str() {
                    if text.len() > 200_000 {
                        let old_len = text.len();
                        let compacted = truncate_middle_chars(text, 200_000, 140_000, 40_000);
                        let new_len = compacted.len();
                        *result_val = Value::String(compacted);
                        total = total.saturating_sub(old_len.saturating_sub(new_len));
                    }
                }
            } else if let Some(text_val) = part.get_mut("text") {
                if let Some(text) = text_val.as_str() {
                    if text.len() > 200_000 {
                        let old_len = text.len();
                        let compacted = truncate_middle_chars(text, 200_000, 140_000, 40_000);
                        let new_len = compacted.len();
                        *text_val = Value::String(compacted);
                        total = total.saturating_sub(old_len.saturating_sub(new_len));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminder_extracts_supported_text_without_rewriting_it() {
        let content = json!([
            { "type": "text", "text": "first" },
            "second",
            { "type": "image", "source": "ignored" },
            { "text": "  " }
        ]);
        assert_eq!(
            system_reminder_text(&content).as_deref(),
            Some("<system-reminder>\nfirst\nsecond\n</system-reminder>")
        );
    }

    #[test]
    fn reminder_appends_only_to_adjacent_user_turn() {
        let mut contents = vec![json!({ "role": "user", "parts": [{ "text": "question" }] })];
        push_system_reminder(&mut contents, &json!({ "text": "directive" }));
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["parts"].as_array().unwrap().len(), 2);

        contents.push(json!({ "role": "model", "parts": [{ "text": "answer" }] }));
        push_system_reminder(&mut contents, &json!("later"));
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[2]["role"], "user");
    }
}
