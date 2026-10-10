//! 提交客户端响应前的有界首输出检测；预读字节由调用方原样回放。

use super::{find_sse_frame_end, Bytes, Value};

pub(super) const MAX_PREFETCH_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FirstOutputError {
    Timeout,
    BufferLimit,
    Protocol,
    EarlyEof,
}

impl FirstOutputError {
    pub(super) fn category(self) -> &'static str {
        match self {
            Self::Timeout => "first_output_timeout",
            Self::BufferLimit => "first_output_buffer_limit",
            Self::Protocol => "first_output_protocol_error",
            Self::EarlyEof => "first_output_early_eof",
        }
    }

    pub(super) fn message(self) -> &'static str {
        match self {
            Self::Timeout => "等待上游首输出超时",
            Self::BufferLimit => "上游首输出前的缓冲数据超过限制",
            Self::Protocol => "上游首输出前返回无效或错误的流式事件",
            Self::EarlyEof => "上游在生成输出或正常结束之前断开",
        }
    }
}

#[derive(Default)]
pub(super) struct FirstOutputProbe {
    buffer: Vec<u8>,
    offset: usize,
    scan_from: usize,
}

impl FirstOutputProbe {
    pub(super) fn push(&mut self, chunk: &[u8]) -> Result<bool, FirstOutputError> {
        if chunk.len() > MAX_PREFETCH_BYTES.saturating_sub(self.buffer.len()) {
            return Err(FirstOutputError::BufferLimit);
        }
        self.buffer.extend_from_slice(chunk);
        while let Some((end, delimiter)) = find_sse_frame_end(&self.buffer[self.scan_from..]) {
            let end = self.scan_from + end;
            let frame = &self.buffer[self.offset..end];
            self.offset = end + delimiter;
            self.scan_from = self.offset;
            let frame = std::str::from_utf8(frame).map_err(|_| FirstOutputError::Protocol)?;
            let data = frame.lines().filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim_start).collect::<Vec<_>>().join("\n");
            if data.is_empty() { continue; }
            if data == "[DONE]" { return Ok(true); }
            let event: Value = serde_json::from_str(&data).map_err(|_| FirstOutputError::Protocol)?;
            if event_ready(&event)? { return Ok(true); }
        }
        // 保留最多 3 字节重叠以识别跨 chunk 的 CRLF 分隔符，避免半帧重复全量扫描。
        self.scan_from = self.offset.max(self.buffer.len().saturating_sub(3));
        Ok(false)
    }

    pub(super) fn remaining_capacity(&self) -> usize {
        MAX_PREFETCH_BYTES.saturating_sub(self.buffer.len())
    }

    pub(super) fn into_bytes(self) -> Bytes { Bytes::from(self.buffer) }
}

fn nonempty(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str).is_some_and(|text| !text.is_empty())
}

fn event_ready(event: &Value) -> Result<bool, FirstOutputError> {
    let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
    if event.get("error").is_some_and(|error| !error.is_null())
        || matches!(kind, "error" | "response.failed" | "response.incomplete")
    {
        return Err(FirstOutputError::Protocol);
    }
    if matches!(kind, "message_stop" | "response.completed") { return Ok(true); }
    if kind == "content_block_start" {
        let block = &event["content_block"];
        if block["type"] == "tool_use" || nonempty(block.get("text"))
            || nonempty(block.get("thinking")) { return Ok(true); }
    }
    if kind == "content_block_delta" {
        let delta = &event["delta"];
        if ["text", "thinking", "partial_json"].iter().any(|key| nonempty(delta.get(key))) {
            return Ok(true);
        }
    }
    if matches!(kind, "response.output_text.delta" | "response.reasoning_text.delta"
        | "response.reasoning_summary_text.delta" | "response.function_call_arguments.delta")
        && nonempty(event.get("delta")) { return Ok(true); }
    if kind == "response.output_item.added" && event["item"]["type"] == "function_call"
        && nonempty(event["item"].get("name")) { return Ok(true); }
    if let Some(choices) = event.get("choices").and_then(Value::as_array) {
        for choice in choices {
            let delta = &choice["delta"];
            if ["content", "reasoning_content", "reasoning"].iter().any(|key| nonempty(delta.get(key))) {
                return Ok(true);
            }
            if choice.get("finish_reason").is_some_and(|value| !value.is_null()) { return Ok(true); }
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                if calls.iter().any(|call| nonempty(call["function"].get("name"))
                    || nonempty(call["function"].get("arguments"))) { return Ok(true); }
            }
            if nonempty(delta["function_call"].get("name"))
                || nonempty(delta["function_call"].get("arguments")) { return Ok(true); }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ignores_envelopes_heartbeats_roles_and_empty_deltas() {
        for event in [json!({"type":"message_start"}), json!({"type":"ping"}),
            json!({"type":"content_block_delta","delta":{"text":""}}),
            json!({"choices":[{"delta":{"role":"assistant","content":""}}]}),
            json!({"type":"response.created"})] {
            assert!(!event_ready(&event).unwrap());
        }
        assert!(!FirstOutputProbe::default().push(b": ping\r\n\r\n").unwrap());
    }

    #[test]
    fn recognizes_text_reasoning_tools_and_normal_empty_completion() {
        for event in [json!({"type":"content_block_delta","delta":{"text":"好"}}),
            json!({"type":"content_block_delta","delta":{"thinking":"想"}}),
            json!({"type":"content_block_start","content_block":{"type":"tool_use"}}),
            json!({"type":"content_block_delta","delta":{"partial_json":"{"}}),
            json!({"choices":[{"delta":{"reasoning_content":"think"}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"function":{"name":"read"}}]}}]}),
            json!({"type":"response.function_call_arguments.delta","delta":"{"}),
            json!({"type":"response.reasoning_summary_text.delta","delta":"think"}),
            json!({"type":"response.output_text.delta","delta":"hi"}),
            json!({"type":"message_stop"}), json!({"type":"response.completed"}),
            json!({"choices":[{"delta":{},"finish_reason":"stop"}]})] {
            assert!(event_ready(&event).unwrap(), "{event}");
        }
    }

    #[test]
    fn split_utf8_and_crlf_replay_original_bytes() {
        let bytes = "event: content_block_delta\r\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"你好\"}}\r\n\r\n".as_bytes();
        let mut probe = FirstOutputProbe::default();
        for (index, byte) in bytes.iter().enumerate() {
            assert_eq!(probe.push(&[*byte]).unwrap(), index == bytes.len() - 1);
        }
        assert_eq!(probe.into_bytes().as_ref(), bytes);
    }

    #[test]
    fn rejects_errors_invalid_frames_and_buffer_overflow() {
        assert_eq!(event_ready(&json!({"type":"response.failed"})), Err(FirstOutputError::Protocol));
        assert_eq!(FirstOutputProbe::default().push(b"data: broken\n\n"), Err(FirstOutputError::Protocol));
        let mut probe = FirstOutputProbe::default();
        assert!(!probe.push(&vec![b'x'; MAX_PREFETCH_BYTES]).unwrap());
        assert_eq!(probe.push(b"x"), Err(FirstOutputError::BufferLimit));
    }
}
