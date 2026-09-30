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
    pub event_type: String,
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
        return Err(FrameError::TooSmall { length: total_length });
    }
    if total_length as usize > MAX_MESSAGE_SIZE {
        return Err(FrameError::TooLarge { length: total_length });
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
    let event_type = header_value(&buffer[header_start..header_end], ":event-type")
        .ok_or(FrameError::BadHeaders)?;
    let payload = buffer[header_end..total_length - 4].to_vec();
    Ok(Some((
        Frame {
            event_type,
            payload,
        },
        total_length,
    )))
}

fn header_value(headers: &[u8], name: &str) -> Option<String> {
    let mut offset = 0;
    while offset < headers.len() {
        let name_len = *headers.get(offset)? as usize;
        offset += 1;
        let header_name = std::str::from_utf8(headers.get(offset..offset + name_len)?).ok()?;
        offset += name_len;
        let value_type = *headers.get(offset)?;
        offset += 1;
        if value_type != HEADER_STRING {
            return None;
        }
        let value_len = u16::from_be_bytes([*headers.get(offset)?, *headers.get(offset + 1)?]) as usize;
        offset += 2;
        let value = std::str::from_utf8(headers.get(offset..offset + value_len)?).ok()?;
        offset += value_len;
        if header_name == name {
            return Some(value.to_string());
        }
    }
    None
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
            match parse_frame(&self.buffer)? {
                Some((frame, consumed)) => {
                    self.buffer.drain(..consumed);
                    frames.push(frame);
                }
                None => break,
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
}
