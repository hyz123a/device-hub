use std::{collections::VecDeque, mem};

use device_hub_shared::SerialConfig;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio_serial::SerialPortBuilderExt;

const DEFAULT_READ_CHUNK_BYTES: usize = 1024;
const DEFAULT_MAX_MESSAGE_BYTES: usize = 4096;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SerialLogChunk {
    pub line: String,
}

#[derive(Debug, Error)]
pub enum SerialAdapterError {
    #[error("failed to open serial port `{port}`: {source}")]
    Open {
        port: String,
        #[source]
        source: tokio_serial::Error,
    },
    #[error("failed to read serial stream: {0}")]
    Io(#[from] std::io::Error),
}

pub struct SerialTailSession {
    stream: tokio_serial::SerialStream,
    buffer: Vec<u8>,
    queued: VecDeque<SerialLogChunk>,
    max_message_bytes: usize,
    eof: bool,
}

#[derive(Clone, Debug)]
pub struct SerialAdapter {
    max_message_bytes: usize,
}

impl SerialAdapter {
    pub fn new() -> Self {
        Self {
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
        }
    }

    pub fn with_max_message_bytes(max_message_bytes: usize) -> Self {
        Self {
            max_message_bytes: max_message_bytes.max(1),
        }
    }

    pub async fn tail_logs_stream(
        &self,
        config: &SerialConfig,
    ) -> Result<SerialTailSession, SerialAdapterError> {
        let stream = tokio_serial::new(&config.port, config.baud)
            .open_native_async()
            .map_err(|source| SerialAdapterError::Open {
                port: config.port.clone(),
                source,
            })?;

        Ok(SerialTailSession {
            stream,
            buffer: Vec::new(),
            queued: VecDeque::new(),
            max_message_bytes: self.max_message_bytes,
            eof: false,
        })
    }
}

impl Default for SerialAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl SerialTailSession {
    pub async fn next_chunk(&mut self) -> Result<Option<SerialLogChunk>, SerialAdapterError> {
        loop {
            if let Some(chunk) = self.queued.pop_front() {
                return Ok(Some(chunk));
            }

            if self.eof {
                return Ok(None);
            }

            let mut read_buffer = [0_u8; DEFAULT_READ_CHUNK_BYTES];
            let read = self.stream.read(&mut read_buffer).await?;
            if read == 0 {
                self.eof = true;
                self.flush_pending();
                continue;
            }

            self.push_bytes(&read_buffer[..read]);
        }
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        push_bytes_into_queue(
            &mut self.buffer,
            &mut self.queued,
            self.max_message_bytes,
            bytes,
        );
    }

    fn flush_pending(&mut self) {
        if !self.buffer.is_empty() {
            emit_pending_line(&mut self.buffer, &mut self.queued);
        }
    }
}

fn push_bytes_into_queue(
    buffer: &mut Vec<u8>,
    queued: &mut VecDeque<SerialLogChunk>,
    max_message_bytes: usize,
    bytes: &[u8],
) {
    for &byte in bytes {
        if byte == b'\n' {
            emit_pending_line(buffer, queued);
            continue;
        }

        buffer.push(byte);
        if buffer.len() >= max_message_bytes {
            emit_buffer_chunk(buffer, queued, max_message_bytes);
        }
    }
}

fn emit_pending_line(buffer: &mut Vec<u8>, queued: &mut VecDeque<SerialLogChunk>) {
    let bytes = mem::take(buffer);
    let line = normalize_line(bytes);
    if !line.is_empty() {
        queued.push_back(SerialLogChunk { line });
    }
}

fn emit_buffer_chunk(
    buffer: &mut Vec<u8>,
    queued: &mut VecDeque<SerialLogChunk>,
    max_message_bytes: usize,
) {
    let remainder = buffer.split_off(max_message_bytes);
    let chunk = mem::replace(buffer, remainder);
    let line = normalize_line(chunk);
    if !line.is_empty() {
        queued.push_back(SerialLogChunk { line });
    }
}

fn normalize_line(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes)
        .trim_end_matches('\r')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_line_trims_trailing_carriage_return() {
        assert_eq!(normalize_line(b"boot ok\r".to_vec()), "boot ok");
    }

    #[test]
    fn chunking_keeps_messages_bounded() {
        let mut buffer = Vec::new();
        let mut queued = VecDeque::new();

        push_bytes_into_queue(&mut buffer, &mut queued, 4, b"abcdef\n");

        let chunks: Vec<_> = queued.into_iter().map(|chunk| chunk.line).collect();
        assert_eq!(chunks, vec!["abcd", "ef"]);
    }
}
