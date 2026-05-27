use std::{
    collections::{HashMap, VecDeque},
    mem,
    sync::{Arc, Mutex},
};

use device_hub_shared::{DiscoveredSerialPort, SerialConfig, SerialExecRequest, SerialExecResult};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf, split};
use tokio::time::{Duration, timeout};
use tokio_serial::{SerialPortBuilderExt, SerialPortType};

const DEFAULT_READ_CHUNK_BYTES: usize = 1024;
const DEFAULT_MAX_MESSAGE_BYTES: usize = 4096;
const DEFAULT_EXEC_TIMEOUT: Duration = Duration::from_secs(10);
const IDLE_FLUSH_TIMEOUT: Duration = Duration::from_millis(40);
type SerialStreamOpener =
    dyn Fn(&SerialConfig) -> Result<tokio_serial::SerialStream, SerialAdapterError> + Send + Sync;

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
    #[error("failed to enumerate serial ports: {0}")]
    Enumerate(#[source] tokio_serial::Error),
    #[error("serial port `{port}` is already in use by an active serial.tail job")]
    Busy { port: String },
}

pub struct SerialTailSession {
    reader: ReadHalf<tokio_serial::SerialStream>,
    shared_writer: Arc<SharedSerialWriter>,
    session_key: String,
    sessions: Arc<Mutex<HashMap<String, std::sync::Weak<SharedSerialWriter>>>>,
    buffer: Vec<u8>,
    queued: VecDeque<SerialLogChunk>,
    max_message_bytes: usize,
    eof: bool,
}

#[derive(Clone)]
pub struct SerialAdapter {
    max_message_bytes: usize,
    sessions: Arc<Mutex<HashMap<String, std::sync::Weak<SharedSerialWriter>>>>,
    opener: Arc<SerialStreamOpener>,
}

#[derive(Debug)]
struct SharedSerialWriter {
    writer: tokio::sync::Mutex<WriteHalf<tokio_serial::SerialStream>>,
}

impl SerialAdapter {
    pub fn new() -> Self {
        Self::with_max_message_bytes(DEFAULT_MAX_MESSAGE_BYTES)
    }

    pub fn with_max_message_bytes(max_message_bytes: usize) -> Self {
        Self {
            max_message_bytes: max_message_bytes.max(1),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            opener: Arc::new(open_stream),
        }
    }

    pub fn with_stream_opener(
        max_message_bytes: usize,
        opener: impl Fn(&SerialConfig) -> Result<tokio_serial::SerialStream, SerialAdapterError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            max_message_bytes: max_message_bytes.max(1),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            opener: Arc::new(opener),
        }
    }

    pub async fn tail_logs_stream(
        &self,
        config: &SerialConfig,
    ) -> Result<SerialTailSession, SerialAdapterError> {
        let session_key = session_key(config);
        {
            let mut sessions = self
                .sessions
                .lock()
                .expect("serial session registry poisoned");
            match sessions
                .get(&session_key)
                .and_then(std::sync::Weak::upgrade)
            {
                Some(_) => {
                    return Err(SerialAdapterError::Busy {
                        port: config.port.clone(),
                    });
                }
                None => {
                    sessions.remove(&session_key);
                }
            }
        }

        let stream = (self.opener)(config)?;
        let (reader, writer) = split(stream);
        let shared_writer = Arc::new(SharedSerialWriter {
            writer: tokio::sync::Mutex::new(writer),
        });

        self.sessions
            .lock()
            .expect("serial session registry poisoned")
            .insert(session_key.clone(), Arc::downgrade(&shared_writer));

        Ok(SerialTailSession {
            reader,
            shared_writer,
            session_key,
            sessions: self.sessions.clone(),
            buffer: Vec::new(),
            queued: VecDeque::new(),
            max_message_bytes: self.max_message_bytes,
            eof: false,
        })
    }

    pub async fn write(
        &self,
        config: &SerialConfig,
        payload: &[u8],
    ) -> Result<usize, SerialAdapterError> {
        if let Some(shared_writer) = self.lookup_shared_writer(config) {
            shared_writer.write(payload).await?;
            return Ok(payload.len());
        }

        let mut stream = (self.opener)(config)?;

        stream.write_all(payload).await?;
        stream.flush().await?;
        Ok(payload.len())
    }

    pub async fn exec(
        &self,
        config: &SerialConfig,
        request: &SerialExecRequest,
    ) -> Result<SerialExecResult, SerialAdapterError> {
        if self.lookup_shared_writer(config).is_some() {
            return Err(SerialAdapterError::Busy {
                port: config.port.clone(),
            });
        }

        let timeout_duration = request
            .timeout_secs
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_EXEC_TIMEOUT);
        let deadline = tokio::time::Instant::now() + timeout_duration;
        let mut stream = (self.opener)(config)?;
        let input = command_input(&request.command);

        stream.write_all(input.as_bytes()).await?;
        stream.flush().await?;

        let mut output = String::new();
        let mut read_buffer = [0_u8; DEFAULT_READ_CHUNK_BYTES];
        loop {
            if let Some(marker) = request.until.as_deref().filter(|marker| !marker.is_empty()) {
                if output.contains(marker) {
                    return Ok(SerialExecResult {
                        command: request.command.clone(),
                        output,
                        matched: Some(marker.to_owned()),
                        timed_out: false,
                    });
                }
            }

            let now = tokio::time::Instant::now();
            if now >= deadline {
                return Ok(SerialExecResult {
                    command: request.command.clone(),
                    output,
                    matched: None,
                    timed_out: true,
                });
            }

            match timeout(deadline - now, stream.read(&mut read_buffer)).await {
                Ok(result) => {
                    let read = result?;
                    if read == 0 {
                        return Ok(SerialExecResult {
                            command: request.command.clone(),
                            output,
                            matched: None,
                            timed_out: false,
                        });
                    }
                    output.push_str(&decode_chunk(read_buffer[..read].to_vec()));
                }
                Err(_) => {
                    return Ok(SerialExecResult {
                        command: request.command.clone(),
                        output,
                        matched: None,
                        timed_out: true,
                    });
                }
            }
        }
    }

    pub fn discover_ports(&self) -> Result<Vec<DiscoveredSerialPort>, SerialAdapterError> {
        let mut ports = tokio_serial::available_ports().map_err(SerialAdapterError::Enumerate)?;
        ports.sort_by(|left, right| left.port_name.cmp(&right.port_name));
        Ok(ports
            .into_iter()
            .map(|info| DiscoveredSerialPort {
                path: info.port_name,
                label: serial_port_label(&info.port_type),
            })
            .collect())
    }

    fn lookup_shared_writer(&self, config: &SerialConfig) -> Option<Arc<SharedSerialWriter>> {
        let session_key = session_key(config);
        let mut sessions = self
            .sessions
            .lock()
            .expect("serial session registry poisoned");
        match sessions
            .get(&session_key)
            .and_then(std::sync::Weak::upgrade)
        {
            Some(shared_writer) => Some(shared_writer),
            None => {
                sessions.remove(&session_key);
                None
            }
        }
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
            let read = if self.buffer.is_empty() {
                self.reader.read(&mut read_buffer).await?
            } else {
                match timeout(IDLE_FLUSH_TIMEOUT, self.reader.read(&mut read_buffer)).await {
                    Ok(result) => result?,
                    Err(_) => {
                        self.flush_pending();
                        continue;
                    }
                }
            };
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

impl Drop for SerialTailSession {
    fn drop(&mut self) {
        let mut sessions = self
            .sessions
            .lock()
            .expect("serial session registry poisoned");
        if sessions
            .get(&self.session_key)
            .and_then(std::sync::Weak::upgrade)
            .is_some_and(|shared_writer| Arc::ptr_eq(&shared_writer, &self.shared_writer))
        {
            sessions.remove(&self.session_key);
        }
    }
}

impl SharedSerialWriter {
    async fn write(&self, payload: &[u8]) -> Result<(), SerialAdapterError> {
        let mut writer = self.writer.lock().await;
        writer.write_all(payload).await?;
        writer.flush().await?;
        Ok(())
    }
}

fn serial_port_label(port_type: &SerialPortType) -> Option<String> {
    match port_type {
        SerialPortType::UsbPort(info) => {
            let mut parts = Vec::new();
            if let Some(manufacturer) = info
                .manufacturer
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                parts.push(manufacturer.to_owned());
            }
            if let Some(product) = info
                .product
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                parts.push(product.to_owned());
            }
            if let Some(serial_number) = info
                .serial_number
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                parts.push(format!("SN {serial_number}"));
            }
            (!parts.is_empty()).then(|| parts.join(" · "))
        }
        SerialPortType::PciPort => Some("PCI serial port".into()),
        SerialPortType::BluetoothPort => Some("Bluetooth serial port".into()),
        SerialPortType::Unknown => None,
    }
}

fn open_stream(config: &SerialConfig) -> Result<tokio_serial::SerialStream, SerialAdapterError> {
    tokio_serial::new(&config.port, config.baud)
        .open_native_async()
        .map_err(|source| SerialAdapterError::Open {
            port: config.port.clone(),
            source,
        })
}

fn session_key(config: &SerialConfig) -> String {
    format!("{}:{}", config.port, config.baud)
}

fn command_input(command: &str) -> String {
    if command.ends_with('\n') || command.ends_with('\r') {
        command.to_owned()
    } else {
        format!("{command}\n")
    }
}

fn push_bytes_into_queue(
    buffer: &mut Vec<u8>,
    queued: &mut VecDeque<SerialLogChunk>,
    max_message_bytes: usize,
    bytes: &[u8],
) {
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];

        if byte == b'\r' {
            buffer.push(byte);
            if bytes.get(index + 1) == Some(&b'\n') {
                buffer.push(b'\n');
                index += 1;
            }
            emit_pending_line(buffer, queued);
            index += 1;
            continue;
        }

        if byte == b'\n' {
            buffer.push(byte);
            emit_pending_line(buffer, queued);
            index += 1;
            continue;
        }

        buffer.push(byte);
        if buffer.len() >= max_message_bytes {
            emit_buffer_chunk(buffer, queued, max_message_bytes);
        }
        index += 1;
    }
}

fn emit_pending_line(buffer: &mut Vec<u8>, queued: &mut VecDeque<SerialLogChunk>) {
    let bytes = mem::take(buffer);
    let line = decode_chunk(bytes);
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
    let line = decode_chunk(chunk);
    if !line.is_empty() {
        queued.push_back(SerialLogChunk { line });
    }
}

fn decode_chunk(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn decode_chunk_preserves_terminal_control_bytes() {
        assert_eq!(decode_chunk(b"boot ok\r".to_vec()), "boot ok\r");
    }

    #[test]
    fn chunking_keeps_messages_bounded() {
        let mut buffer = Vec::new();
        let mut queued = VecDeque::new();

        push_bytes_into_queue(&mut buffer, &mut queued, 4, b"abcdef\n");

        let chunks: Vec<_> = queued.into_iter().map(|chunk| chunk.line).collect();
        assert_eq!(chunks, vec!["abcd", "ef\n"]);
    }

    #[test]
    fn carriage_return_also_flushes_serial_chunks() {
        let mut buffer = Vec::new();
        let mut queued = VecDeque::new();

        push_bytes_into_queue(&mut buffer, &mut queued, 64, b"booting\rready\r\nnext\n");

        let chunks: Vec<_> = queued.into_iter().map(|chunk| chunk.line).collect();
        assert_eq!(chunks, vec!["booting\r", "ready\r\n", "next\n"]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn idle_gap_flushes_partial_serial_output() {
        let config = SerialConfig {
            port: "test-port".into(),
            baud: 115200,
        };
        let (master, mut slave) = tokio_serial::SerialStream::pair().expect("serial pair");
        let (reader, writer) = split(master);
        let shared_writer = Arc::new(SharedSerialWriter {
            writer: tokio::sync::Mutex::new(writer),
        });
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let key = session_key(&config);

        let mut tail_session = SerialTailSession {
            reader,
            shared_writer,
            session_key: key,
            sessions,
            buffer: Vec::new(),
            queued: VecDeque::new(),
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            eof: false,
        };

        slave
            .write_all(b"partial frame")
            .await
            .expect("write serial output");

        let chunk = tail_session
            .next_chunk()
            .await
            .expect("tail chunk result")
            .expect("tail chunk");
        assert_eq!(chunk.line, "partial frame");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_reuses_active_tail_session() {
        let config = SerialConfig {
            port: "test-port".into(),
            baud: 115200,
        };
        let (master, mut slave) = tokio_serial::SerialStream::pair().expect("serial pair");
        let (reader, writer) = split(master);
        let shared_writer = Arc::new(SharedSerialWriter {
            writer: tokio::sync::Mutex::new(writer),
        });
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let key = session_key(&config);
        sessions
            .lock()
            .expect("serial session registry poisoned")
            .insert(key.clone(), Arc::downgrade(&shared_writer));

        let adapter = SerialAdapter {
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            sessions: sessions.clone(),
            opener: Arc::new(open_stream),
        };
        let mut tail_session = SerialTailSession {
            reader,
            shared_writer,
            session_key: key.clone(),
            sessions: sessions.clone(),
            buffer: Vec::new(),
            queued: VecDeque::new(),
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            eof: false,
        };

        let write_task = tokio::spawn(async move { adapter.write(&config, b"help\n").await });
        let mut command = [0_u8; 5];
        slave.read_exact(&mut command).await.expect("read command");
        assert_eq!(&command, b"help\n");

        slave.write_all(b"ok\n").await.expect("write response");
        let chunk = tail_session
            .next_chunk()
            .await
            .expect("tail chunk result")
            .expect("tail chunk");
        assert_eq!(chunk.line, "ok\n");
        assert_eq!(
            write_task
                .await
                .expect("join write task")
                .expect("write result"),
            5
        );

        drop(tail_session);
        assert!(
            sessions
                .lock()
                .expect("serial session registry poisoned")
                .get(&key)
                .is_none()
        );
    }
}
