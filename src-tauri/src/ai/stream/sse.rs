//! 有界增量 SSE 解码器。
//!
//! 网络分块不等于 SSE 事件边界，本模块先按行增量解码，再按空行组装事件。
//! 所有缓冲区都有硬上限，避免异常中转站用无限长行或事件持续占用内存。

use reqwest::Response;
use tokio_util::sync::CancellationToken;

use crate::ai::error::ModelError;

const MAX_STREAM_BYTES: usize = 64 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_EVENT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event_type: Option<String>,
    pub data: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SseReadOutcome {
    Completed,
    Cancelled,
}

#[derive(Default)]
struct SseDecoder {
    buffer: Vec<u8>,
    event_type: Option<String>,
    data_lines: Vec<String>,
    event_bytes: usize,
}

impl SseDecoder {
    fn feed(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, ModelError> {
        if self.buffer.len().saturating_add(chunk.len()) > MAX_LINE_BYTES {
            return Err(ModelError::invalid_response("流式响应包含过长的 SSE 行。"));
        }
        self.buffer.extend_from_slice(chunk);

        let mut events = Vec::new();
        let mut consumed = 0;
        while let Some(relative_end) = self.buffer[consumed..]
            .iter()
            .position(|byte| *byte == b'\n')
        {
            let end = consumed + relative_end;
            let line = self.buffer[consumed..end].to_vec();
            self.process_line(&line, &mut events)?;
            consumed = end + 1;
        }
        if consumed > 0 {
            self.buffer.drain(..consumed);
        }
        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<SseEvent>, ModelError> {
        let mut events = Vec::new();
        if !self.buffer.is_empty() {
            let line = std::mem::take(&mut self.buffer);
            self.process_line(&line, &mut events)?;
        }
        if let Some(event) = self.take_event() {
            events.push(event);
        }
        Ok(events)
    }

    fn process_line(
        &mut self,
        raw_line: &[u8],
        events: &mut Vec<SseEvent>,
    ) -> Result<(), ModelError> {
        let raw_line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        let line = std::str::from_utf8(raw_line)
            .map_err(|_| ModelError::invalid_response("SSE 响应不是有效的 UTF-8。"))?;
        if line.is_empty() {
            if let Some(event) = self.take_event() {
                events.push(event);
            }
            return Ok(());
        }
        if line.starts_with(':') {
            return Ok(());
        }

        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => self.event_type = Some(value.to_string()),
            "data" => {
                self.event_bytes = self.event_bytes.saturating_add(value.len());
                if self.event_bytes > MAX_EVENT_BYTES {
                    return Err(ModelError::invalid_response("单个 SSE 事件过大。"));
                }
                self.data_lines.push(value.to_string());
            }
            _ => {}
        }
        Ok(())
    }

    fn take_event(&mut self) -> Option<SseEvent> {
        let event_type = self.event_type.take();
        let data_lines = std::mem::take(&mut self.data_lines);
        self.event_bytes = 0;
        if data_lines.is_empty() {
            return None;
        }
        Some(SseEvent {
            event_type,
            data: data_lines.join("\n"),
        })
    }
}

pub async fn consume<F>(
    mut response: Response,
    cancellation: &CancellationToken,
    mut on_event: F,
) -> Result<SseReadOutcome, ModelError>
where
    F: FnMut(SseEvent) -> Result<(), ModelError>,
{
    if response
        .content_length()
        .is_some_and(|length| length > MAX_STREAM_BYTES as u64)
    {
        return Err(ModelError::invalid_response("流式响应超过允许的大小。"));
    }

    let mut decoder = SseDecoder::default();
    let mut total_bytes = 0usize;
    loop {
        let chunk = tokio::select! {
            _ = cancellation.cancelled() => return Ok(SseReadOutcome::Cancelled),
            chunk = response.chunk() => chunk.map_err(ModelError::from_reqwest)?,
        };
        let Some(chunk) = chunk else {
            break;
        };
        total_bytes = total_bytes.saturating_add(chunk.len());
        if total_bytes > MAX_STREAM_BYTES {
            return Err(ModelError::invalid_response("流式响应超过允许的大小。"));
        }
        for event in decoder.feed(&chunk)? {
            on_event(event)?;
        }
    }
    for event in decoder.finish()? {
        on_event(event)?;
    }
    Ok(SseReadOutcome::Completed)
}

#[cfg(test)]
mod tests {
    use super::SseDecoder;

    #[test]
    fn decodes_split_multiline_events() {
        let mut decoder = SseDecoder::default();
        assert!(decoder.feed(b"event: text\r\nda").unwrap().is_empty());
        let events = decoder.feed(b"ta: hello\r\ndata: world\r\n\r\n").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type.as_deref(), Some("text"));
        assert_eq!(events[0].data, "hello\nworld");
    }

    #[test]
    fn ignores_comments_and_flushes_last_event_at_eof() {
        let mut decoder = SseDecoder::default();
        decoder.feed(b": ping\ndata: final").unwrap();
        let events = decoder.finish().unwrap();
        assert_eq!(events[0].data, "final");
    }
}
