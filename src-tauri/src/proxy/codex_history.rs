//! In-memory Codex Responses history for bridged upstreams.
//!
//! Chat / Anthropic targets need full input history when clients send only
//! `previous_response_id` and new input. Native Responses requests still use
//! server-side history; their answers are retained for later model switches.

use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};

const MAX_CACHED_RESPONSES: usize = 512;
const MAX_HISTORY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
struct CachedResponse {
    calls_by_id: HashMap<String, Value>,
    call_order: Vec<String>,
    conversation: Option<Arc<Vec<Value>>>,
    conversation_bytes: usize,
}

#[derive(Debug, Default)]
struct Inner {
    responses: HashMap<String, CachedResponse>,
    response_order: VecDeque<String>,
    call_index: HashMap<String, VecDeque<String>>,
    history_bytes: usize,
}

#[derive(Debug, Default)]
pub struct CodexHistoryStore {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct CachedLookup {
    previous: Option<CachedResponse>,
    fallback: CachedResponse,
}

impl CodexHistoryStore {
    /// Retain the input as well as the answer: a response ID alone cannot carry
    /// user text, images or completed tool rounds to a stateless upstream.
    pub fn record_exchange(&self, request: &Value, response: &Value) {
        if response.get("status").and_then(Value::as_str) == Some("failed") {
            return;
        }
        let Some(id) = response.get("id").and_then(Value::as_str).filter(|id| !id.is_empty()) else {
            return;
        };
        let Some(output) = response.get("output").and_then(Value::as_array) else {
            return;
        };
        let mut request = request.clone();
        if self.restore_conversation(&mut request).is_err() {
            return;
        }
        self.enrich_request(&mut request);
        let Some(mut items) = request.get("input").and_then(input_items) else {
            return;
        };
        items.extend(output.iter().cloned());
        let size = serde_json::to_vec(&items).map(|bytes| bytes.len()).unwrap_or_default();
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if !inner.responses.contains_key(id) {
            inner.response_order.push_back(id.to_string());
        }
        let entry = inner.responses.entry(id.to_string()).or_default();
        let old_size = entry.conversation_bytes;
        entry.conversation = Some(Arc::new(items));
        entry.conversation_bytes = size;
        inner.history_bytes = inner.history_bytes - old_size + size;
        inner.prune();
    }

    /// Expand only an explicitly referenced response. Never borrow another
    /// session's conversation or silently send a delta as a complete history.
    pub fn restore_conversation(&self, request: &mut Value) -> Result<(), String> {
        let Some(id) = request.get("previous_response_id").and_then(Value::as_str)
            .filter(|id| !id.is_empty()) else {
            return Ok(());
        };
        let conversation = self.inner.lock().ok()
            .and_then(|inner| inner.responses.get(id).and_then(|entry| entry.conversation.clone()))
            .ok_or_else(|| "previous_response_id 的本地会话历史不可用（程序重启或缓存已过期），请新建对话后重试".to_string())?;
        let mut items = conversation.as_ref().clone();
        let input = request.get("input").and_then(input_items)
            .ok_or_else(|| "Responses input 必须是字符串或数组".to_string())?;
        items.extend(input);
        request["input"] = Value::Array(items);
        request.as_object_mut().unwrap().remove("previous_response_id");
        Ok(())
    }

    pub fn record_response(&self, response: &Value) -> usize {
        let Some(response_id) = response
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        else {
            return 0;
        };
        let calls = response
            .get("output")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(cached_call_item)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if calls.is_empty() {
            return 0;
        }
        let Ok(mut inner) = self.inner.lock() else {
            return 0;
        };
        inner.insert_calls(response_id, calls)
    }

    pub fn record_call_item(&self, response_id: Option<&str>, item: &Value) -> bool {
        let Some(response_id) = response_id.filter(|value| !value.is_empty()) else {
            return false;
        };
        let Some(call) = cached_call_item(item) else {
            return false;
        };
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        inner.insert_calls(response_id, vec![call]) > 0
    }

    /// Insert missing assistant call items before tool outputs when bridging away
    /// from native Responses. Returns how many call items were restored/enriched.
    pub fn enrich_request(&self, body: &mut Value) -> usize {
        let previous_response_id = body
            .get("previous_response_id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let Some(input) = body.get_mut("input") else {
            return 0;
        };

        let original_input = std::mem::take(input);
        let original_was_object = matches!(&original_input, Value::Object(_));
        let items = match original_input {
            Value::Array(items) => items,
            Value::Object(object) => vec![Value::Object(object)],
            other => {
                *input = other;
                return 0;
            }
        };

        let output_call_ids = items
            .iter()
            .filter(|item| item_type(item).is_some_and(is_call_output_item_type))
            .filter_map(response_item_call_id)
            .collect::<HashSet<_>>();
        if output_call_ids.is_empty() {
            *input = restore_input_shape(items, original_was_object, 0);
            return 0;
        }
        let existing_call_ids = items
            .iter()
            .filter(|item| item_type(item).is_some_and(is_call_item_type))
            .filter_map(response_item_call_id)
            .collect::<HashSet<_>>();
        let requested_call_ids = output_call_ids
            .union(&existing_call_ids)
            .cloned()
            .collect::<HashSet<_>>();
        let lookup = self.lookup(previous_response_id.as_deref(), &requested_call_ids);
        let restore_group = lookup.restore_group(&output_call_ids, &existing_call_ids);
        let restore_group_ids = restore_group
            .iter()
            .map(|(call_id, _)| call_id.clone())
            .collect::<HashSet<_>>();

        let mut restore_group = Some(restore_group);
        let mut seen_call_ids = HashSet::new();
        let mut changed = 0usize;
        let mut new_items = Vec::new();

        for mut item in items {
            match item_type(&item) {
                Some(kind) if is_call_item_type(kind) => {
                    if let Some(call_id) = response_item_call_id(&item) {
                        if let Some(cached) = lookup.call(&call_id) {
                            changed += usize::from(enrich_call_item_from_cache(&mut item, cached));
                        }
                        seen_call_ids.insert(call_id);
                    }
                    new_items.push(item);
                }
                Some(kind) if is_call_output_item_type(kind) => {
                    if let Some(group) = restore_group.take().filter(|group| !group.is_empty()) {
                        for (call_id, cached_item) in group {
                            seen_call_ids.insert(call_id);
                            new_items.push(cached_item);
                            changed += 1;
                        }
                    }
                    if let Some(call_id) = response_item_call_id(&item) {
                        if !seen_call_ids.contains(&call_id)
                            && !restore_group_ids.contains(&call_id)
                        {
                            if let Some(cached) = lookup.call(&call_id).cloned() {
                                seen_call_ids.insert(call_id);
                                new_items.push(cached);
                                changed += 1;
                            }
                        }
                    }
                    new_items.push(item);
                }
                _ => new_items.push(item),
            }
        }

        *input = restore_input_shape(new_items, original_was_object, changed);
        changed
    }

    /// Inspect a Responses SSE JSON event and record call items when present.
    pub fn inspect_sse_event(&self, value: &Value, current_response_id: &mut Option<String>) {
        if let Some(response_id) = value
            .pointer("/response/id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            *current_response_id = Some(response_id.to_string());
        }
        match value.get("type").and_then(Value::as_str) {
            Some("response.output_item.done") => {
                if let Some(item) = value.get("item") {
                    self.record_call_item(current_response_id.as_deref(), item);
                }
            }
            Some("response.completed") => {
                if let Some(response) = value.get("response") {
                    self.record_response(response);
                }
            }
            _ => {}
        }
    }

    fn lookup(
        &self,
        previous_response_id: Option<&str>,
        requested_call_ids: &HashSet<String>,
    ) -> CachedLookup {
        let Ok(inner) = self.inner.lock() else {
            return CachedLookup::default();
        };
        let previous = previous_response_id.and_then(|id| inner.responses.get(id).cloned());
        let fallback = inner.unique_fallback_calls(requested_call_ids, previous.as_ref());
        CachedLookup { previous, fallback }
    }
}

impl Inner {
    fn insert_calls(&mut self, response_id: &str, calls: Vec<(String, Value)>) -> usize {
        if !self.responses.contains_key(response_id) {
            self.response_order.push_back(response_id.to_string());
        }
        let cached_response = self.responses.entry(response_id.to_string()).or_default();
        let mut changed = 0usize;
        let mut indexed_call_ids = Vec::new();
        for (call_id, item) in calls {
            if !cached_response.calls_by_id.contains_key(&call_id) {
                cached_response.call_order.push(call_id.clone());
            }
            cached_response.calls_by_id.insert(call_id.clone(), item);
            indexed_call_ids.push(call_id);
            changed += 1;
        }
        for call_id in indexed_call_ids {
            self.index_call(&call_id, response_id);
        }
        self.prune();
        changed
    }

    fn prune(&mut self) {
        while self.response_order.len() > MAX_CACHED_RESPONSES
            || self.history_bytes > MAX_HISTORY_BYTES
        {
            let Some(response_id) = self.response_order.pop_front() else {
                break;
            };
            if let Some(entry) = self.responses.remove(&response_id) {
                self.history_bytes -= entry.conversation_bytes;
            }
            for response_ids in self.call_index.values_mut() {
                response_ids.retain(|cached_id| cached_id != &response_id);
            }
            self.call_index
                .retain(|_, response_ids| !response_ids.is_empty());
        }
    }

    fn index_call(&mut self, call_id: &str, response_id: &str) {
        let response_ids = self.call_index.entry(call_id.to_string()).or_default();
        if !response_ids
            .iter()
            .any(|cached_response_id| cached_response_id == response_id)
        {
            response_ids.push_back(response_id.to_string());
        }
    }

    fn unique_fallback_calls(
        &self,
        requested_call_ids: &HashSet<String>,
        previous: Option<&CachedResponse>,
    ) -> CachedResponse {
        let mut selected = HashMap::new();
        for call_id in requested_call_ids {
            if previous.is_some_and(|response| response.calls_by_id.contains_key(call_id)) {
                continue;
            }
            if let Some(item) = self.unique_call(call_id) {
                selected.insert(call_id.clone(), item.clone());
            }
        }

        let mut fallback = CachedResponse::default();
        for response_id in &self.response_order {
            let Some(response) = self.responses.get(response_id) else {
                continue;
            };
            for call_id in &response.call_order {
                if let Some(item) = selected.remove(call_id) {
                    fallback.call_order.push(call_id.clone());
                    fallback.calls_by_id.insert(call_id.clone(), item);
                }
            }
        }
        fallback
    }

    fn unique_call(&self, call_id: &str) -> Option<&Value> {
        let response_ids = self.call_index.get(call_id)?;
        let mut found = None;
        for response_id in response_ids {
            let Some(item) = self
                .responses
                .get(response_id)
                .and_then(|response| response.calls_by_id.get(call_id))
            else {
                continue;
            };
            if found.is_some() {
                return None;
            }
            found = Some(item);
        }
        found
    }
}

fn input_items(input: &Value) -> Option<Vec<Value>> {
    match input {
        Value::String(text) => Some(vec![serde_json::json!({"role": "user", "content": text})]),
        Value::Array(items) => Some(items.clone()),
        Value::Object(_) => Some(vec![input.clone()]),
        _ => None,
    }
}

impl CachedLookup {
    fn call(&self, call_id: &str) -> Option<&Value> {
        self.previous
            .as_ref()
            .and_then(|previous| previous.calls_by_id.get(call_id))
            .or_else(|| self.fallback.calls_by_id.get(call_id))
    }

    fn restore_group(
        &self,
        output_call_ids: &HashSet<String>,
        existing_call_ids: &HashSet<String>,
    ) -> Vec<(String, Value)> {
        let mut group = Vec::new();
        let mut grouped_call_ids = HashSet::new();
        if let Some(previous) = &self.previous {
            append_restore_group(
                previous,
                output_call_ids,
                existing_call_ids,
                &mut grouped_call_ids,
                &mut group,
            );
        }
        append_restore_group(
            &self.fallback,
            output_call_ids,
            existing_call_ids,
            &mut grouped_call_ids,
            &mut group,
        );
        group
    }
}

fn append_restore_group(
    response: &CachedResponse,
    output_call_ids: &HashSet<String>,
    existing_call_ids: &HashSet<String>,
    grouped_call_ids: &mut HashSet<String>,
    group: &mut Vec<(String, Value)>,
) {
    for call_id in &response.call_order {
        if !output_call_ids.contains(call_id)
            || existing_call_ids.contains(call_id)
            || grouped_call_ids.contains(call_id)
        {
            continue;
        }
        if let Some(item) = response.calls_by_id.get(call_id).cloned() {
            grouped_call_ids.insert(call_id.clone());
            group.push((call_id.clone(), item));
        }
    }
}

fn restore_input_shape(items: Vec<Value>, original_was_object: bool, changed: usize) -> Value {
    if changed == 0 && original_was_object && items.len() == 1 {
        items.into_iter().next().unwrap_or(Value::Null)
    } else {
        Value::Array(items)
    }
}

fn cached_call_item(item: &Value) -> Option<(String, Value)> {
    if !item_type(item).is_some_and(is_call_item_type) {
        return None;
    }
    let call_id = response_item_call_id(item)?;
    Some((call_id, item.clone()))
}

fn item_type(item: &Value) -> Option<&str> {
    item.get("type").and_then(Value::as_str)
}

fn response_item_call_id(item: &Value) -> Option<String> {
    item.get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn is_call_item_type(item_type: &str) -> bool {
    matches!(
        item_type,
        "function_call" | "custom_tool_call" | "tool_search_call"
    )
}

fn is_call_output_item_type(item_type: &str) -> bool {
    matches!(
        item_type,
        "function_call_output" | "custom_tool_call_output" | "tool_search_output"
    )
}

fn is_empty_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(object) => object.is_empty(),
        _ => false,
    }
}

fn enrich_call_item_from_cache(item: &mut Value, cached: &Value) -> bool {
    let Some(object) = item.as_object_mut() else {
        return false;
    };
    let mut changed = false;
    for key in [
        "name",
        "namespace",
        "arguments",
        "input",
        "status",
        "execution",
        "reasoning_content",
        "reasoning",
    ] {
        if object.get(key).is_some_and(|value| !is_empty_value(value)) {
            continue;
        }
        let Some(value) = cached.get(key).filter(|value| !is_empty_value(value)) else {
            continue;
        };
        object.insert(key.to_string(), value.clone());
        changed = true;
    }
    changed
}

/// Find the earliest physical SSE frame delimiter (`\n\n` or `\r\n\r\n`).
pub fn take_sse_block(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let mut earliest: Option<(usize, usize)> = None;
    for index in 0..buffer.len().saturating_sub(1) {
        let delim = if buffer[index..].starts_with(b"\r\n\r\n") {
            Some(4usize)
        } else if buffer[index..].starts_with(b"\n\n") {
            Some(2usize)
        } else {
            None
        };
        let Some(delimiter_len) = delim else {
            continue;
        };
        match earliest {
            Some((end, _)) if index >= end => {}
            _ => earliest = Some((index, delimiter_len)),
        }
    }
    let (end, delimiter_len) = earliest?;
    let block = buffer.drain(..end).collect::<Vec<_>>();
    buffer.drain(..delimiter_len);
    Some(block)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn answer(id: &str, text: &str) -> Value {
        json!({"id": id, "output": [{
            "type": "message", "role": "assistant",
            "content": [{"type": "output_text", "text": text}]
        }]})
    }

    #[test]
    fn restores_text_across_models_and_chained_turns_without_duplication() {
        let history = CodexHistoryStore::default();
        history.record_exchange(
            &json!({"model":"gemini-4-argon-low", "input":"Remember BLUE742."}),
            &answer("resp_a", "已记住"),
        );
        let second = json!({
            "model":"claude-sonnet-5.5", "previous_response_id":"resp_a",
            "input":[{"role":"user", "content":[{"type":"input_text","text":"暗号是什么？"}]}]
        });
        history.record_exchange(&second, &answer("resp_b", "BLUE742"));
        let mut third = json!({
            "model":"gemini-4-argon-low", "previous_response_id":"resp_b", "input":"再回答一次"
        });
        history.restore_conversation(&mut third).unwrap();
        assert_eq!(third["input"].as_array().unwrap().len(), 5);
        assert_eq!(third["input"][0]["content"], "Remember BLUE742.");
        assert_eq!(third["input"][3]["content"][0]["text"], "BLUE742");
        assert!(third.get("previous_response_id").is_none());
        let expanded = third.clone();
        history.restore_conversation(&mut third).unwrap();
        assert_eq!(third, expanded);
        let mut independent = json!({"input":"new session"});
        history.restore_conversation(&mut independent).unwrap();
        assert_eq!(independent, json!({"input":"new session"}));
    }

    #[test]
    fn restores_original_task_images_and_tool_results_into_kiro_history() {
        let history = CodexHistoryStore::default();
        let image = json!({"type":"input_image","image_url":"data:image/png;base64,aGVsbG8="});
        history.record_exchange(
            &json!({"input":[{"role":"user","content":[
                {"type":"input_text","text":"Find Workspace Trust and describe this image."}, image
            ]}]}),
            &json!({"id":"resp_search","output":[{
                "type":"function_call","call_id":"call_search","name":"web_search",
                "arguments":"{\"query\":\"Workspace Trust\"}"
            }]}),
        );
        let mut followup = json!({
            "model":"claude-sonnet-5.5", "previous_response_id":"resp_search",
            "input":[{"type":"function_call_output","call_id":"call_search","output":"Restricted Mode"}]
        });
        history.restore_conversation(&mut followup).unwrap();
        let account = crate::kiro::account::account_from_import(&json!({
            "refreshToken":"rt", "authMethod":"idc", "provider":"BuilderId",
            "clientId":"cid", "clientSecret":"sec"
        })).unwrap();
        let prepared = crate::kiro::map::prepare(
            crate::kiro::map::WireProtocol::OpenAiResponses, &followup, &account,
        ).unwrap();
        let conversation = &prepared.body["conversationState"];
        assert_eq!(conversation["history"][0]["userInputMessage"]["content"],
            "Find Workspace Trust and describe this image.");
        assert_eq!(conversation["history"][0]["userInputMessage"]["images"][0]["source"]["bytes"],
            "aGVsbG8=");
        assert_eq!(conversation["history"][1]["assistantResponseMessage"]["toolUses"][0]["toolUseId"],
            "call_search");
        assert_eq!(conversation["currentMessage"]["userInputMessage"]["userInputMessageContext"]
            ["toolResults"][0]["content"][0]["text"], "Restricted Mode");
    }

    #[test]
    fn missing_history_does_not_borrow_another_conversation_or_mutate_input() {
        let history = CodexHistoryStore::default();
        history.record_exchange(&json!({"input":"private A"}), &answer("resp_a", "OK"));
        let mut request = json!({"previous_response_id":"resp_missing","input":"recall"});
        let original = request.clone();
        assert!(history.restore_conversation(&mut request).is_err());
        assert_eq!(request, original);
        history.record_exchange(&request, &answer("resp_incomplete_chain", "unknown"));
        assert!(!history.inner.lock().unwrap().responses.contains_key("resp_incomplete_chain"));
    }

    #[test]
    fn prunes_conversations_with_existing_response_limit_and_byte_budget() {
        let history = CodexHistoryStore::default();
        for i in 0..=MAX_CACHED_RESPONSES {
            history.record_exchange(&json!({"input":"hello"}), &answer(&format!("resp_{i}"), "OK"));
        }
        let mut inner = history.inner.lock().unwrap();
        assert_eq!(inner.responses.len(), MAX_CACHED_RESPONSES);
        assert!(!inner.responses.contains_key("resp_0"));
        assert_eq!(inner.history_bytes,
            inner.responses.values().map(|entry| entry.conversation_bytes).sum::<usize>());
        // Model a large image entry without allocating a 64 MiB fixture.
        inner.responses.get_mut("resp_1").unwrap().conversation_bytes += MAX_HISTORY_BYTES;
        inner.history_bytes += MAX_HISTORY_BYTES;
        inner.prune();
        assert!(!inner.responses.contains_key("resp_1"));
        assert!(inner.history_bytes < MAX_HISTORY_BYTES);
    }

    #[test]
    fn enriches_tool_output_with_cached_function_call_from_previous_response() {
        let history = CodexHistoryStore::default();
        history.record_response(&json!({
            "id": "resp_1",
            "output": [{
                "type": "function_call",
                "call_id": "call_1",
                "name": "read_file",
                "arguments": "{\"path\":\"README.md\"}",
                "reasoning_content": "Need to inspect the file."
            }]
        }));

        let mut request = json!({
            "previous_response_id": "resp_1",
            "input": [{
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "ok"
            }]
        });

        assert_eq!(history.enrich_request(&mut request), 1);
        let input = request["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["reasoning_content"], "Need to inspect the file.");
        assert_eq!(input[1]["type"], "function_call_output");
    }

    #[test]
    fn does_not_restore_ambiguous_call_id_without_previous_response() {
        let history = CodexHistoryStore::default();
        for response_id in ["resp_1", "resp_2"] {
            history.record_response(&json!({
                "id": response_id,
                "output": [{
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": "{}"
                }]
            }));
        }

        let mut request = json!({
            "input": [{
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "ok"
            }]
        });

        assert_eq!(history.enrich_request(&mut request), 0);
        assert_eq!(request["input"][0]["type"], "function_call_output");
    }

    #[test]
    fn takes_physically_earliest_sse_delimiter() {
        let mut buffer = b"data: first\r\n\r\ndata: second\n\n".to_vec();
        assert_eq!(
            take_sse_block(&mut buffer).as_deref(),
            Some(b"data: first".as_slice())
        );
        assert_eq!(
            take_sse_block(&mut buffer).as_deref(),
            Some(b"data: second".as_slice())
        );
    }
}
