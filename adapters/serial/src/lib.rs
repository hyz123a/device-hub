use std::{
    collections::{HashMap, VecDeque},
    mem,
    sync::{Arc, Mutex},
};

use device_hub_shared::{
    DiscoveredSerialPort, SerialConfig, SerialDeviceSelector, SerialExecRequest, SerialExecResult,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf, split};
use tokio::sync::Notify;
use tokio::time::{Duration, timeout};
use tokio_serial::{SerialPortBuilderExt, SerialPortType};

const DEFAULT_READ_CHUNK_BYTES: usize = 1024;
const DEFAULT_MAX_MESSAGE_BYTES: usize = 4096;
const DEFAULT_CAPTURE_BUFFER_BYTES: usize = 1024 * 1024;
const DEFAULT_EXEC_TIMEOUT: Duration = Duration::from_secs(10);
const IDLE_FLUSH_TIMEOUT: Duration = Duration::from_millis(40);
type SerialStreamOpener =
    dyn Fn(&SerialConfig) -> Result<tokio_serial::SerialStream, SerialAdapterError> + Send + Sync;
type SerialPortDiscoverer =
    dyn Fn() -> Result<Vec<DiscoveredSerialPort>, SerialAdapterError> + Send + Sync;

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
    #[error("serial selector must contain at least one stable USB attribute")]
    EmptySelector,
    #[error("no serial port matches selector {selector}")]
    SelectorNotFound { selector: String },
    #[error("serial selector {selector} is ambiguous; matched ports: {ports}")]
    SelectorAmbiguous { selector: String, ports: String },
}

pub struct SerialTailSession {
    reader: ReadHalf<tokio_serial::SerialStream>,
    endpoint: String,
    shared_session: Arc<SharedSerialSession>,
    session_key: String,
    sessions: Arc<Mutex<HashMap<String, std::sync::Weak<SharedSerialSession>>>>,
    buffer: Vec<u8>,
    queued: VecDeque<SerialLogChunk>,
    max_message_bytes: usize,
    eof: bool,
}

#[derive(Clone)]
pub struct SerialAdapter {
    max_message_bytes: usize,
    sessions: Arc<Mutex<HashMap<String, std::sync::Weak<SharedSerialSession>>>>,
    opener: Arc<SerialStreamOpener>,
    discoverer: Arc<SerialPortDiscoverer>,
}

#[derive(Debug)]
struct SharedSerialSession {
    writer: tokio::sync::Mutex<WriteHalf<tokio_serial::SerialStream>>,
    capture: Mutex<SharedSerialCapture>,
    notify: Notify,
}

#[derive(Debug, Default)]
struct SharedSerialCapture {
    text: String,
    dropped_bytes: usize,
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
            discoverer: Arc::new(discover_serial_ports),
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
            discoverer: Arc::new(discover_serial_ports),
        }
    }

    pub fn with_io(
        max_message_bytes: usize,
        opener: impl Fn(&SerialConfig) -> Result<tokio_serial::SerialStream, SerialAdapterError>
        + Send
        + Sync
        + 'static,
        discoverer: impl Fn() -> Result<Vec<DiscoveredSerialPort>, SerialAdapterError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            max_message_bytes: max_message_bytes.max(1),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            opener: Arc::new(opener),
            discoverer: Arc::new(discoverer),
        }
    }

    pub async fn tail_logs_stream(
        &self,
        config: &SerialConfig,
    ) -> Result<SerialTailSession, SerialAdapterError> {
        let resolved_config = self.resolve_config(config)?;
        let config = &resolved_config;
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
        let shared_session = Arc::new(SharedSerialSession {
            writer: tokio::sync::Mutex::new(writer),
            capture: Mutex::new(SharedSerialCapture::default()),
            notify: Notify::new(),
        });

        self.sessions
            .lock()
            .expect("serial session registry poisoned")
            .insert(session_key.clone(), Arc::downgrade(&shared_session));

        Ok(SerialTailSession {
            reader,
            endpoint: config.port.clone(),
            shared_session,
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
        let resolved_config = self.resolve_config(config)?;
        let config = &resolved_config;
        if let Some(shared_session) = self.lookup_shared_session(config) {
            shared_session.write(payload).await?;
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
        let resolved_config = self.resolve_config(config)?;
        let config = &resolved_config;
        if let Some(shared_session) = self.lookup_shared_session(config) {
            return shared_session.exec(request).await;
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
            if let Some(marker) = request.until.as_deref().filter(|marker| !marker.is_empty())
                && output.contains(marker)
            {
                return Ok(SerialExecResult {
                    command: request.command.clone(),
                    output,
                    matched: Some(marker.to_owned()),
                    timed_out: false,
                });
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
        (self.discoverer)()
    }

    pub fn resolve_config(
        &self,
        config: &SerialConfig,
    ) -> Result<SerialConfig, SerialAdapterError> {
        let Some(selector) = config.selector.as_ref() else {
            return Ok(config.clone());
        };
        if selector_is_empty(selector) {
            return Err(SerialAdapterError::EmptySelector);
        }

        let matches = self
            .discover_ports()?
            .into_iter()
            .filter(|port| selector_matches(selector, port))
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => Err(SerialAdapterError::SelectorNotFound {
                selector: format_selector(selector),
            }),
            [port] => Ok(SerialConfig {
                port: port.path.clone(),
                baud: config.baud,
                selector: config.selector.clone(),
            }),
            ports => Err(SerialAdapterError::SelectorAmbiguous {
                selector: format_selector(selector),
                ports: ports
                    .iter()
                    .map(|port| port.path.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            }),
        }
    }

    fn lookup_shared_session(&self, config: &SerialConfig) -> Option<Arc<SharedSerialSession>> {
        let session_key = session_key(config);
        let mut sessions = self
            .sessions
            .lock()
            .expect("serial session registry poisoned");
        match sessions
            .get(&session_key)
            .and_then(std::sync::Weak::upgrade)
        {
            Some(shared_session) => Some(shared_session),
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
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

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
        self.shared_session.append_capture(bytes);
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
            .is_some_and(|shared_session| Arc::ptr_eq(&shared_session, &self.shared_session))
        {
            sessions.remove(&self.session_key);
        }
    }
}

impl SharedSerialSession {
    async fn write(&self, payload: &[u8]) -> Result<(), SerialAdapterError> {
        let mut writer = self.writer.lock().await;
        writer.write_all(payload).await?;
        writer.flush().await?;
        Ok(())
    }

    async fn exec(
        &self,
        request: &SerialExecRequest,
    ) -> Result<SerialExecResult, SerialAdapterError> {
        let timeout_duration = request
            .timeout_secs
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_EXEC_TIMEOUT);
        let deadline = tokio::time::Instant::now() + timeout_duration;
        let start_offset = self.capture_offset();
        let input = command_input(&request.command);

        self.write(input.as_bytes()).await?;

        loop {
            let output = self.captured_since(start_offset);
            if let Some(marker) = request.until.as_deref().filter(|marker| !marker.is_empty())
                && output.contains(marker)
            {
                return Ok(SerialExecResult {
                    command: request.command.clone(),
                    output,
                    matched: Some(marker.to_owned()),
                    timed_out: false,
                });
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

            if timeout(deadline - now, self.notify.notified())
                .await
                .is_err()
            {
                return Ok(SerialExecResult {
                    command: request.command.clone(),
                    output: self.captured_since(start_offset),
                    matched: None,
                    timed_out: true,
                });
            }
        }
    }

    fn append_capture(&self, bytes: &[u8]) {
        let chunk = decode_chunk(bytes.to_vec());
        if chunk.is_empty() {
            return;
        }

        self.capture
            .lock()
            .expect("serial capture poisoned")
            .append(&chunk, DEFAULT_CAPTURE_BUFFER_BYTES);
        self.notify.notify_waiters();
    }

    fn captured_since(&self, start_offset: usize) -> String {
        self.capture
            .lock()
            .expect("serial capture poisoned")
            .since(start_offset)
    }

    fn capture_offset(&self) -> usize {
        self.capture
            .lock()
            .expect("serial capture poisoned")
            .offset()
    }
}

impl SharedSerialCapture {
    fn append(&mut self, chunk: &str, max_bytes: usize) {
        self.text.push_str(chunk);
        if self.text.len() <= max_bytes {
            return;
        }

        let mut drain_len = self.text.len() - max_bytes;
        while drain_len < self.text.len() && !self.text.is_char_boundary(drain_len) {
            drain_len += 1;
        }
        self.text.drain(..drain_len);
        self.dropped_bytes += drain_len;
    }

    fn since(&self, start_offset: usize) -> String {
        let local_offset = start_offset.saturating_sub(self.dropped_bytes);
        self.text
            .get(local_offset..)
            .unwrap_or(&self.text)
            .to_owned()
    }

    fn offset(&self) -> usize {
        self.dropped_bytes + self.text.len()
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

fn discover_serial_ports() -> Result<Vec<DiscoveredSerialPort>, SerialAdapterError> {
    let mut ports = tokio_serial::available_ports().map_err(SerialAdapterError::Enumerate)?;
    ports.sort_by(|left, right| left.port_name.cmp(&right.port_name));
    #[cfg(windows)]
    let physical_locations = windows_physical_locations();
    Ok(ports
        .into_iter()
        .map(|info| {
            let (vid, pid, serial_number) = match &info.port_type {
                SerialPortType::UsbPort(usb) => {
                    (Some(usb.vid), Some(usb.pid), usb.serial_number.clone())
                }
                _ => (None, None, None),
            };
            let path = info.port_name;
            #[cfg(windows)]
            let physical_location = physical_locations.get(&path.to_ascii_uppercase()).cloned();
            DiscoveredSerialPort {
                path,
                label: serial_port_label(&info.port_type),
                vid,
                pid,
                serial_number,
                physical_location: {
                    #[cfg(windows)]
                    {
                        physical_location
                    }
                    #[cfg(not(windows))]
                    {
                        None
                    }
                },
            }
        })
        .collect())
}

#[cfg(windows)]
fn windows_physical_locations() -> HashMap<String, String> {
    use std::ptr;
    use windows_sys::Win32::{
        Devices::DeviceAndDriverInstallation::{
            DIGCF_PRESENT, GUID_DEVCLASS_PORTS, HDEVINFO, SP_DEVINFO_DATA, SPDRP_FRIENDLYNAME,
            SPDRP_LOCATION_INFORMATION, SPDRP_LOCATION_PATHS, SetupDiDestroyDeviceInfoList,
            SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
        },
        Foundation::INVALID_HANDLE_VALUE,
    };

    struct DeviceInfoSet(HDEVINFO);
    impl Drop for DeviceInfoSet {
        fn drop(&mut self) {
            // SAFETY: the handle was returned by SetupDiGetClassDevsW and is owned by this guard.
            unsafe {
                SetupDiDestroyDeviceInfoList(self.0);
            }
        }
    }

    let handle = unsafe {
        // SAFETY: all optional pointers are null and the class GUID has static storage.
        SetupDiGetClassDevsW(
            &GUID_DEVCLASS_PORTS,
            ptr::null(),
            ptr::null_mut(),
            DIGCF_PRESENT,
        )
    };
    if handle == INVALID_HANDLE_VALUE as HDEVINFO {
        return HashMap::new();
    }
    let device_info_set = DeviceInfoSet(handle);
    let mut locations = HashMap::new();
    let mut index = 0;

    loop {
        let mut device = SP_DEVINFO_DATA {
            cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        let found = unsafe {
            // SAFETY: device_info_set is valid and device points to an initialized output buffer.
            SetupDiEnumDeviceInfo(device_info_set.0, index, &mut device)
        };
        if found == 0 {
            break;
        }
        index += 1;

        let Some(friendly_name) =
            setupapi_registry_property(device_info_set.0, &device, SPDRP_FRIENDLYNAME)
        else {
            continue;
        };
        let Some(port) = extract_com_port(&friendly_name) else {
            continue;
        };
        let location = setupapi_registry_property(device_info_set.0, &device, SPDRP_LOCATION_PATHS)
            .or_else(|| {
                setupapi_registry_property(device_info_set.0, &device, SPDRP_LOCATION_INFORMATION)
            });
        if let Some(location) = location.filter(|value| !value.trim().is_empty()) {
            locations.insert(port, location);
        }
    }

    locations
}

#[cfg(windows)]
fn setupapi_registry_property(
    device_info_set: windows_sys::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    device: &windows_sys::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
    property: windows_sys::Win32::Devices::DeviceAndDriverInstallation::SETUP_DI_REGISTRY_PROPERTY,
) -> Option<String> {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetDeviceRegistryPropertyW;

    let mut value_type = 0;
    let mut required_size = 0;
    let mut buffer = [0_u16; 2048];
    let found = unsafe {
        // SAFETY: buffer is writable for the advertised byte length and the device belongs to
        // device_info_set for the duration of this call.
        SetupDiGetDeviceRegistryPropertyW(
            device_info_set,
            device,
            property,
            &mut value_type,
            buffer.as_mut_ptr().cast(),
            std::mem::size_of_val(&buffer) as u32,
            &mut required_size,
        )
    };
    if found == 0 {
        return None;
    }

    let units = ((required_size as usize) / std::mem::size_of::<u16>()).min(buffer.len());
    decode_windows_property(&buffer[..units])
}

#[cfg(windows)]
fn decode_windows_property(buffer: &[u16]) -> Option<String> {
    let end = buffer
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(buffer.len());
    (end > 0).then(|| String::from_utf16_lossy(&buffer[..end]))
}

#[cfg(windows)]
fn extract_com_port(friendly_name: &str) -> Option<String> {
    let marker = friendly_name.rfind("(COM")?;
    let suffix = &friendly_name[marker + 1..];
    let port = suffix.strip_suffix(')')?;
    port.strip_prefix("COM")?
        .chars()
        .all(|character| character.is_ascii_digit())
        .then(|| port.to_ascii_uppercase())
}

fn selector_is_empty(selector: &SerialDeviceSelector) -> bool {
    selector.vid.is_none()
        && selector.pid.is_none()
        && selector.serial_number.is_none()
        && selector.physical_location.is_none()
}

fn selector_matches(selector: &SerialDeviceSelector, port: &DiscoveredSerialPort) -> bool {
    selector.vid.is_none_or(|vid| port.vid == Some(vid))
        && selector.pid.is_none_or(|pid| port.pid == Some(pid))
        && selector
            .serial_number
            .as_deref()
            .is_none_or(|serial| port.serial_number.as_deref() == Some(serial))
        && selector
            .physical_location
            .as_deref()
            .is_none_or(|location| port.physical_location.as_deref() == Some(location))
}

fn format_selector(selector: &SerialDeviceSelector) -> String {
    let mut fields = Vec::new();
    if let Some(vid) = selector.vid {
        fields.push(format!("vid={vid:04x}"));
    }
    if let Some(pid) = selector.pid {
        fields.push(format!("pid={pid:04x}"));
    }
    if let Some(serial) = selector.serial_number.as_deref() {
        fields.push(format!("serial={serial}"));
    }
    if let Some(location) = selector.physical_location.as_deref() {
        fields.push(format!("location={location}"));
    }
    fields.join(",")
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
    #[cfg(unix)]
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

    #[cfg(windows)]
    #[test]
    fn extracts_com_port_from_windows_friendly_name() {
        assert_eq!(
            extract_com_port("USB-SERIAL CH340 (COM24)").as_deref(),
            Some("COM24")
        );
        assert_eq!(extract_com_port("USB device without a port"), None);
        assert_eq!(extract_com_port("USB-SERIAL CH340 (COMx)"), None);
    }

    #[cfg(windows)]
    #[test]
    fn decodes_first_windows_location_path_from_multi_string() {
        let encoded = "PCIROOT(0)#USBROOT(0)#USB(4)\0SECOND\0\0"
            .encode_utf16()
            .collect::<Vec<_>>();
        assert_eq!(
            decode_windows_property(&encoded).as_deref(),
            Some("PCIROOT(0)#USBROOT(0)#USB(4)")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn idle_gap_flushes_partial_serial_output() {
        let config = SerialConfig {
            port: "test-port".into(),
            baud: 115200,
            selector: None,
        };
        let (master, mut slave) = tokio_serial::SerialStream::pair().expect("serial pair");
        let (reader, writer) = split(master);
        let shared_session = Arc::new(SharedSerialSession {
            writer: tokio::sync::Mutex::new(writer),
            capture: Mutex::new(SharedSerialCapture::default()),
            notify: Notify::new(),
        });
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let key = session_key(&config);

        let mut tail_session = SerialTailSession {
            reader,
            endpoint: config.port.clone(),
            shared_session,
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
            selector: None,
        };
        let (master, mut slave) = tokio_serial::SerialStream::pair().expect("serial pair");
        let (reader, writer) = split(master);
        let shared_session = Arc::new(SharedSerialSession {
            writer: tokio::sync::Mutex::new(writer),
            capture: Mutex::new(SharedSerialCapture::default()),
            notify: Notify::new(),
        });
        let sessions = Arc::new(Mutex::new(HashMap::new()));
        let key = session_key(&config);
        sessions
            .lock()
            .expect("serial session registry poisoned")
            .insert(key.clone(), Arc::downgrade(&shared_session));

        let adapter = SerialAdapter {
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            sessions: sessions.clone(),
            opener: Arc::new(open_stream),
            discoverer: Arc::new(discover_serial_ports),
        };
        let mut tail_session = SerialTailSession {
            reader,
            endpoint: config.port.clone(),
            shared_session,
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

    #[test]
    fn selector_resolves_the_current_endpoint() {
        let ports = vec![DiscoveredSerialPort {
            path: "COM25".into(),
            vid: Some(0x1a86),
            pid: Some(0x7523),
            serial_number: Some("CH341-A".into()),
            ..Default::default()
        }];
        let adapter = SerialAdapter::with_io(DEFAULT_MAX_MESSAGE_BYTES, open_stream, move || {
            Ok(ports.clone())
        });
        let config = SerialConfig {
            port: "COM7".into(),
            baud: 115200,
            selector: Some(SerialDeviceSelector {
                vid: Some(0x1a86),
                pid: Some(0x7523),
                serial_number: Some("CH341-A".into()),
                physical_location: None,
            }),
        };

        let resolved = adapter.resolve_config(&config).expect("resolved selector");

        assert_eq!(resolved.port, "COM25");
        assert_eq!(resolved.selector, config.selector);
    }

    #[test]
    fn selector_rejects_ambiguous_usb_devices() {
        let ports = vec![
            DiscoveredSerialPort {
                path: "COM25".into(),
                vid: Some(0x1a86),
                pid: Some(0x7523),
                ..Default::default()
            },
            DiscoveredSerialPort {
                path: "COM26".into(),
                vid: Some(0x1a86),
                pid: Some(0x7523),
                ..Default::default()
            },
        ];
        let adapter = SerialAdapter::with_io(DEFAULT_MAX_MESSAGE_BYTES, open_stream, move || {
            Ok(ports.clone())
        });
        let config = SerialConfig {
            port: "COM7".into(),
            baud: 115200,
            selector: Some(SerialDeviceSelector {
                vid: Some(0x1a86),
                pid: Some(0x7523),
                ..Default::default()
            }),
        };

        let error = adapter
            .resolve_config(&config)
            .expect_err("ambiguous selector must fail");

        assert!(matches!(
            error,
            SerialAdapterError::SelectorAmbiguous { .. }
        ));
        assert!(error.to_string().contains("COM25, COM26"));
    }

    #[test]
    fn selector_rejects_missing_and_empty_identity() {
        let adapter =
            SerialAdapter::with_io(DEFAULT_MAX_MESSAGE_BYTES, open_stream, || Ok(Vec::new()));
        let empty = SerialConfig {
            port: "COM7".into(),
            baud: 115200,
            selector: Some(SerialDeviceSelector::default()),
        };
        assert!(matches!(
            adapter.resolve_config(&empty),
            Err(SerialAdapterError::EmptySelector)
        ));

        let missing = SerialConfig {
            selector: Some(SerialDeviceSelector {
                serial_number: Some("missing".into()),
                ..Default::default()
            }),
            ..empty
        };
        assert!(matches!(
            adapter.resolve_config(&missing),
            Err(SerialAdapterError::SelectorNotFound { .. })
        ));
    }
}
