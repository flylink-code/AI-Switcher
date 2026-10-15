//! AWS event-stream frames used by Kiro `generateAssistantResponse`.
//!
//! Layout: total length, header length, prelude CRC32, headers, payload, message CRC32.
//! CRC is ISO-HDLC (polynomial 0xEDB88320).

/// CRC32/ISO-HDLC. Empty input is 0; `"123456789"` is `0xCBF43926`.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

const PRELUDE_SIZE: usize = 12;
const MIN_MESSAGE_SIZE: usize = PRELUDE_SIZE + 4;
const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;
const HEADER_STRING: u8 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub message_type: String,
    pub event_type: String,
    pub exception_type: Option<String>,
    pub error_code: Option<String>,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    TooSmall { length: u32 },
    TooLarge { length: u32 },
    PreludeCrc,
    MessageCrc,
    BadHeaders,
}

/// Pull one complete frame. `Ok(None)` means the buffer needs more bytes.
pub fn parse_frame(buffer: &[u8]) -> Result<Option<(Frame, usize)>, FrameError> {
    if buffer.len() < PRELUDE_SIZE {
        return Ok(None);
    }
    let total_length = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
    let header_length = u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
    let prelude_crc = u32::from_be_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]);
    if (total_length as usize) < MIN_MESSAGE_SIZE {
        return Err(FrameError::TooSmall {
            length: total_length,
        });
    }
    if total_length as usize > MAX_MESSAGE_SIZE {
        return Err(FrameError::TooLarge {
            length: total_length,
        });
    }
    let total_length = total_length as usize;
    if buffer.len() < total_length {
        return Ok(None);
    }
    if crc32(&buffer[..8]) != prelude_crc {
        return Err(FrameError::PreludeCrc);
    }
    let message_crc = u32::from_be_bytes([
        buffer[total_length - 4],
        buffer[total_length - 3],
        buffer[total_length - 2],
        buffer[total_length - 1],
    ]);
    if crc32(&buffer[..total_length - 4]) != message_crc {
        return Err(FrameError::MessageCrc);
    }
    let header_start = PRELUDE_SIZE;
    let header_end = header_start + header_length as usize;
    if header_end + 4 > total_length {
        return Err(FrameError::BadHeaders);
    }
    let headers = parse_string_headers(&buffer[header_start..header_end]).ok_or(FrameError::BadHeaders)?;
    let message_type = headers
        .iter()
        .find(|(k, _)| k == ":message-type")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| "event".to_string());
    let exception_type = headers
        .iter()
        .find(|(k, _)| k == ":exception-type")
        .map(|(_, v)| v.clone());
    let error_code = headers
        .iter()
        .find(|(k, _)| k == ":error-code")
        .map(|(_, v)| v.clone());
    let event_type = headers
        .iter()
        .find(|(k, _)| k == ":event-type")
        .map(|(_, v)| v.clone())
        .or_else(|| exception_type.clone())
        .or_else(|| error_code.clone())
        .unwrap_or_else(|| message_type.clone());
    let payload = buffer[header_end..total_length - 4].to_vec();
    Ok(Some((
        Frame {
            message_type,
            event_type,
            exception_type,
            error_code,
            payload,
        },
        total_length,
    )))
}

fn parse_string_headers(headers: &[u8]) -> Option<Vec<(String, String)>> {
    let mut offset = 0;
    let mut out = Vec::new();
    while offset < headers.len() {
        let name_len = *headers.get(offset)? as usize;
        offset += 1;
        if name_len == 0 || offset + name_len > headers.len() {
            return None;
        }
        let header_name = std::str::from_utf8(headers.get(offset..offset + name_len)?).ok()?;
        offset += name_len;
        let value_type = *headers.get(offset)?;
        offset += 1;
        match value_type {
            // 0 = BoolTrue, 1 = BoolFalse (0 bytes)
            0 | 1 => {}
            // 2 = Byte (1 byte)
            2 => {
                if offset + 1 > headers.len() {
                    return None;
                }
                offset += 1;
            }
            // 3 = Short (2 bytes)
            3 => {
                if offset + 2 > headers.len() {
                    return None;
                }
                offset += 2;
            }
            // 4 = Integer (4 bytes)
            4 => {
                if offset + 4 > headers.len() {
                    return None;
                }
                offset += 4;
            }
            // 5 = Long, 8 = Timestamp (8 bytes)
            5 | 8 => {
                if offset + 8 > headers.len() {
                    return None;
                }
                offset += 8;
            }
            // 6 = ByteArray (2-byte length + bytes)
            6 => {
                let value_len =
                    u16::from_be_bytes([*headers.get(offset)?, *headers.get(offset + 1)?]) as usize;
                offset += 2;
                if offset + value_len > headers.len() {
                    return None;
                }
                offset += value_len;
            }
            // 7 = String (2-byte length + UTF-8 bytes)
            HEADER_STRING => {
                let value_len =
                    u16::from_be_bytes([*headers.get(offset)?, *headers.get(offset + 1)?]) as usize;
                offset += 2;
                let value = std::str::from_utf8(headers.get(offset..offset + value_len)?).ok()?;
                offset += value_len;
                out.push((header_name.to_string(), value.to_string()));
            }
            // 9 = UUID (16 bytes)
            9 => {
                if offset + 16 > headers.len() {
                    return None;
                }
                offset += 16;
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Encode one string-header frame. Used by tests and not sent upstream.
pub fn encode_frame(event_type: &str, payload: &[u8]) -> Vec<u8> {
    let mut headers = Vec::new();
    let name = b":event-type";
    headers.push(name.len() as u8);
    headers.extend_from_slice(name);
    headers.push(HEADER_STRING);
    headers.extend_from_slice(&(event_type.len() as u16).to_be_bytes());
    headers.extend_from_slice(event_type.as_bytes());

    let total_length = (PRELUDE_SIZE + headers.len() + payload.len() + 4) as u32;
    let mut message = Vec::new();
    message.extend_from_slice(&total_length.to_be_bytes());
    message.extend_from_slice(&(headers.len() as u32).to_be_bytes());
    let prelude_crc = crc32(&message);
    message.extend_from_slice(&prelude_crc.to_be_bytes());
    message.extend_from_slice(&headers);
    message.extend_from_slice(payload);
    let message_crc = crc32(&message);
    message.extend_from_slice(&message_crc.to_be_bytes());
    message
}

pub struct FrameDecoder {
    buffer: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Frame>, FrameError> {
        self.buffer.extend_from_slice(chunk);
        let mut frames = Vec::new();
        loop {
            match parse_frame(&self.buffer) {
                Ok(Some((frame, consumed))) => {
                    self.buffer.drain(..consumed);
                    frames.push(frame);
                }
                Ok(None) => break,
                Err(err) => {
                    if !frames.is_empty() {
                        // Try skipping a corrupt frame boundary if we already have valid frames
                        let skip = match err {
                            FrameError::BadHeaders | FrameError::MessageCrc => {
                                if self.buffer.len() >= 4 {
                                    let len = u32::from_be_bytes([
                                        self.buffer[0],
                                        self.buffer[1],
                                        self.buffer[2],
                                        self.buffer[3],
                                    ]) as usize;
                                    len.clamp(1, self.buffer.len())
                                } else {
                                    1
                                }
                            }
                            _ => 1,
                        };
                        self.buffer.drain(..skip);
                        continue;
                    }
                    return Err(err);
                }
            }
        }
        Ok(frames)
    }
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vectors() {
        assert_eq!(crc32(&[]), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn round_trip_event_frame() {
        let payload = br#"{"content":"hi"}"#;
        let bytes = encode_frame("assistantResponseEvent", payload);
        let (frame, consumed) = parse_frame(&bytes).unwrap().unwrap();
        assert_eq!(consumed, bytes.len());
        assert_eq!(frame.event_type, "assistantResponseEvent");
        assert_eq!(frame.payload, payload);
    }

    #[test]
    fn decoder_waits_for_a_partial_frame() {
        let bytes = encode_frame("assistantResponseEvent", b"{}");
        let mut decoder = FrameDecoder::new();
        assert!(decoder.push(&bytes[..8]).unwrap().is_empty());
        let frames = decoder.push(&bytes[8..]).unwrap();
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn parse_frame_supports_non_string_headers_and_exception_frames() {
        let mut headers = Vec::new();
        // Header 1: ":timestamp" (type 8 = 8-byte timestamp) before ":event-type"
        let ts_name = b":timestamp";
        headers.push(ts_name.len() as u8);
        headers.extend_from_slice(ts_name);
        headers.push(8u8);
        headers.extend_from_slice(&1760511600000i64.to_be_bytes());
        // Header 2: ":message-type" = "exception"
        let mt_name = b":message-type";
        headers.push(mt_name.len() as u8);
        headers.extend_from_slice(mt_name);
        headers.push(HEADER_STRING);
        headers.extend_from_slice(&(b"exception".len() as u16).to_be_bytes());
        headers.extend_from_slice(b"exception");
        // Header 3: ":exception-type" = "ThrottlingException"
        let ex_name = b":exception-type";
        headers.push(ex_name.len() as u8);
        headers.extend_from_slice(ex_name);
        headers.push(HEADER_STRING);
        headers.extend_from_slice(&(b"ThrottlingException".len() as u16).to_be_bytes());
        headers.extend_from_slice(b"ThrottlingException");

        let payload = br#"{"message":"Slow down"}"#;
        let total_length = (PRELUDE_SIZE + headers.len() + payload.len() + 4) as u32;
        let mut message = Vec::new();
        message.extend_from_slice(&total_length.to_be_bytes());
        message.extend_from_slice(&(headers.len() as u32).to_be_bytes());
        let prelude_crc = crc32(&message);
        message.extend_from_slice(&prelude_crc.to_be_bytes());
        message.extend_from_slice(&headers);
        message.extend_from_slice(payload);
        let message_crc = crc32(&message);
        message.extend_from_slice(&message_crc.to_be_bytes());

        let (frame, consumed) = parse_frame(&message).unwrap().unwrap();
        assert_eq!(consumed, message.len());
        assert_eq!(frame.message_type, "exception");
        assert_eq!(frame.exception_type.as_deref(), Some("ThrottlingException"));
    }
}
