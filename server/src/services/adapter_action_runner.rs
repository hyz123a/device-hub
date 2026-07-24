use chrono::Utc;
#[cfg(feature = "adb")]
use device_hub_adapter_adb::AdbAdapter;
#[cfg(feature = "jlink")]
use device_hub_adapter_jlink::{
    FlashRequest, JlinkAdapter, JlinkAdapterError, JlinkArtifact, JlinkExecution,
};
#[cfg(feature = "serial")]
use device_hub_adapter_serial::{SerialAdapter, SerialAdapterError};
#[cfg(feature = "ssh")]
use device_hub_adapter_ssh::{SshAdapter, SshCommand, SshStreamChannel};
#[cfg(feature = "serial")]
use device_hub_shared::SerialExecRequest;
use device_hub_shared::{
    ActionRequest, AdapterCapability, CapabilitySnapshot, Device, Job, LogChannel, LogEvent,
    LogLevel, LogSource,
};
use futures::future::BoxFuture;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use thiserror::Error;
#[cfg(feature = "serial")]
use tokio::time::{Duration, Instant};

#[cfg(feature = "serial")]
use crate::ports::DynSerialCommandExecutor;
#[cfg(feature = "ssh")]
use crate::services::job_runtime::adapter_event;
#[cfg(feature = "serial")]
use crate::services::system_awake::SystemAwakeGuard;
use crate::{
    ports::{ActionExecutionContext, ActionRunner, CapabilityProviderPort},
    services::job_runtime::{
        adapter_event_with_source, fail_job, fail_job_with_source, succeed_job,
    },
};

pub trait AdapterActionHandler: Send + Sync {
    fn run(
        &self,
        context: ActionExecutionContext,
        job: Job,
        device: Device,
        action: String,
        payload: Option<ActionRequest>,
    ) -> BoxFuture<'static, ()>;
}

type DynAdapterActionHandler = Arc<dyn AdapterActionHandler>;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum CapabilityRegistryError {
    #[error("adapter capability `{0}` is unknown")]
    UnknownAdapter(String),
    #[error("action `{action}` is not declared by adapter `{adapter_id}`")]
    UndeclaredAction { adapter_id: String, action: String },
    #[error("action `{0}` is already registered")]
    DuplicateAction(String),
    #[error("adapter `{adapter_id}` registration is missing actions: {actions}")]
    IncompleteAdapterRegistration { adapter_id: String, actions: String },
}

#[derive(Clone)]
pub struct CapabilityRegistry {
    handlers: Arc<HashMap<String, DynAdapterActionHandler>>,
    snapshot: CapabilitySnapshot,
}

impl CapabilityRegistry {
    pub fn new(capabilities: Vec<AdapterCapability>) -> Self {
        let mut snapshot = CapabilitySnapshot {
            adapters: capabilities,
        };
        snapshot
            .adapters
            .sort_by(|left, right| left.adapter_id.cmp(&right.adapter_id));
        Self {
            handlers: Arc::new(HashMap::new()),
            snapshot,
        }
    }

    pub fn register(
        &mut self,
        adapter_id: &str,
        actions: &[&str],
        handler: impl AdapterActionHandler + 'static,
    ) -> Result<(), CapabilityRegistryError> {
        let Some(capability) = self
            .snapshot
            .adapters
            .iter_mut()
            .find(|capability| capability.adapter_id == adapter_id)
        else {
            return Err(CapabilityRegistryError::UnknownAdapter(
                adapter_id.to_owned(),
            ));
        };
        let mut requested = HashSet::new();
        for action in actions {
            if !requested.insert(*action) {
                return Err(CapabilityRegistryError::DuplicateAction(
                    (*action).to_owned(),
                ));
            }
            if !capability.actions.iter().any(|declared| declared == action) {
                return Err(CapabilityRegistryError::UndeclaredAction {
                    adapter_id: adapter_id.to_owned(),
                    action: (*action).to_owned(),
                });
            }
            if self.handlers.contains_key(*action) {
                return Err(CapabilityRegistryError::DuplicateAction(
                    (*action).to_owned(),
                ));
            }
        }
        let missing = capability
            .actions
            .iter()
            .filter(|action| !requested.contains(action.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(CapabilityRegistryError::IncompleteAdapterRegistration {
                adapter_id: adapter_id.to_owned(),
                actions: missing.join(", "),
            });
        }

        let handler: DynAdapterActionHandler = Arc::new(handler);
        let handlers = Arc::make_mut(&mut self.handlers);
        for action in actions {
            handlers.insert((*action).to_owned(), handler.clone());
        }
        capability.available = true;
        capability.unavailable_reason = None;
        Ok(())
    }
}

impl ActionRunner for CapabilityRegistry {
    fn run(
        &self,
        context: ActionExecutionContext,
        job: Job,
        device: Device,
        action: String,
        payload: Option<ActionRequest>,
    ) -> BoxFuture<'static, ()> {
        let Some(handler) = self.handlers.get(&action).cloned() else {
            return Box::pin(async move {
                fail_job(
                    &context.jobs,
                    &context.events,
                    &context.artifacts,
                    &job,
                    format!("capability for action `{action}` is unavailable"),
                );
            });
        };
        handler.run(context, job, device, action, payload)
    }
}

impl CapabilityProviderPort for CapabilityRegistry {
    fn snapshot(&self) -> CapabilitySnapshot {
        self.snapshot.clone()
    }
}

#[cfg(feature = "ssh")]
#[derive(Clone)]
pub struct SshActionHandler {
    adapter: SshAdapter,
}

#[cfg(feature = "ssh")]
impl SshActionHandler {
    pub fn new(adapter: SshAdapter) -> Self {
        Self { adapter }
    }
}

#[cfg(feature = "ssh")]
impl AdapterActionHandler for SshActionHandler {
    fn run(
        &self,
        context: ActionExecutionContext,
        job: Job,
        device: Device,
        action: String,
        payload: Option<ActionRequest>,
    ) -> BoxFuture<'static, ()> {
        let adapter = self.adapter.clone();
        Box::pin(async move {
            match action.as_str() {
                "ssh.exec" => {
                    execute_ssh_exec(&context, &adapter, &job, &device, payload.as_ref()).await
                }
                "ssh.reboot" => execute_ssh_reboot(&context, &adapter, &job, &device).await,
                "ssh.tail" => {
                    execute_ssh_tail(&context, &adapter, &job, &device, payload.as_ref()).await
                }
                _ => unreachable!("registry routed an action to the wrong SSH handler"),
            }
        })
    }
}

#[cfg(feature = "serial")]
#[derive(Clone)]
pub struct SerialActionHandler {
    adapter: SerialAdapter,
    command_executor: DynSerialCommandExecutor,
    reconnect_policy: SerialReconnectPolicy,
}

#[cfg(feature = "serial")]
impl SerialActionHandler {
    pub fn new(adapter: SerialAdapter, command_executor: DynSerialCommandExecutor) -> Self {
        Self {
            adapter,
            command_executor,
            reconnect_policy: SerialReconnectPolicy::default(),
        }
    }

    pub fn with_reconnect_policy(
        adapter: SerialAdapter,
        command_executor: DynSerialCommandExecutor,
        reconnect_policy: SerialReconnectPolicy,
    ) -> Self {
        Self {
            adapter,
            command_executor,
            reconnect_policy,
        }
    }
}

#[cfg(feature = "serial")]
#[derive(Clone, Copy, Debug)]
pub struct SerialReconnectPolicy {
    pub max_attempts: Option<u32>,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub prevent_system_sleep: bool,
}

#[cfg(feature = "serial")]
impl Default for SerialReconnectPolicy {
    fn default() -> Self {
        Self {
            max_attempts: None,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(2),
            prevent_system_sleep: true,
        }
    }
}

#[cfg(feature = "serial")]
impl AdapterActionHandler for SerialActionHandler {
    fn run(
        &self,
        context: ActionExecutionContext,
        job: Job,
        device: Device,
        action: String,
        payload: Option<ActionRequest>,
    ) -> BoxFuture<'static, ()> {
        let adapter = self.adapter.clone();
        let command_executor = self.command_executor.clone();
        let reconnect_policy = self.reconnect_policy;
        Box::pin(async move {
            match action.as_str() {
                "serial.tail" => {
                    execute_serial_tail(&context, &adapter, &job, &device, reconnect_policy).await
                }
                "serial.write" => {
                    execute_serial_write(&context, &adapter, &job, &device, payload.as_ref()).await
                }
                "serial.exec" => {
                    execute_serial_exec(&context, &command_executor, &job, device, payload.as_ref())
                        .await
                }
                _ => unreachable!("registry routed an action to the wrong serial handler"),
            }
        })
    }
}

#[cfg(feature = "adb")]
#[derive(Clone)]
pub struct AdbActionHandler {
    adapter: AdbAdapter,
}

#[cfg(feature = "adb")]
impl AdbActionHandler {
    pub fn new(adapter: AdbAdapter) -> Self {
        Self { adapter }
    }
}

#[cfg(feature = "adb")]
impl AdapterActionHandler for AdbActionHandler {
    fn run(
        &self,
        context: ActionExecutionContext,
        job: Job,
        device: Device,
        action: String,
        payload: Option<ActionRequest>,
    ) -> BoxFuture<'static, ()> {
        let adapter = self.adapter.clone();
        Box::pin(async move {
            match action.as_str() {
                "adb.logcat" => {
                    execute_adb_logcat(&context, &adapter, &job, &device, payload.as_ref()).await
                }
                _ => unreachable!("registry routed an action to the wrong ADB handler"),
            }
        })
    }
}

#[cfg(feature = "jlink")]
#[derive(Clone)]
pub struct JlinkActionHandler {
    adapter: JlinkAdapter,
}

#[cfg(feature = "jlink")]
impl JlinkActionHandler {
    pub fn new(adapter: JlinkAdapter) -> Self {
        Self { adapter }
    }
}

#[cfg(feature = "jlink")]
impl AdapterActionHandler for JlinkActionHandler {
    fn run(
        &self,
        context: ActionExecutionContext,
        job: Job,
        device: Device,
        action: String,
        payload: Option<ActionRequest>,
    ) -> BoxFuture<'static, ()> {
        let adapter = self.adapter.clone();
        Box::pin(async move {
            match action.as_str() {
                "jlink.probe" => execute_jlink_probe(&context, &adapter, &job, &device).await,
                "jlink.flash" => {
                    execute_jlink_flash(&context, &adapter, &job, &device, payload.as_ref()).await
                }
                "jlink.verify" => execute_jlink_verify(&context, &adapter, &job, &device).await,
                "jlink.reset" => execute_jlink_reset(&context, &adapter, &job, &device).await,
                "jlink.erase" => execute_jlink_erase(&context, &adapter, &job, &device).await,
                _ => unreachable!("registry routed an action to the wrong J-Link handler"),
            }
        })
    }
}

#[cfg(feature = "ssh")]
async fn execute_ssh_exec(
    context: &ActionExecutionContext,
    ssh_adapter: &SshAdapter,
    job: &Job,
    device: &Device,
    payload: Option<&ActionRequest>,
) {
    let Some(config) = device.ssh.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing ssh configuration",
        );
        return;
    };
    let command = payload
        .and_then(|request| request.payload.as_ref())
        .map(|value| value.trim())
        .unwrap_or_default();

    context.events.publish(adapter_event(
        job,
        LogChannel::System,
        LogLevel::Info,
        format!("exec: {command}"),
    ));

    match ssh_adapter
        .exec(
            config,
            &SshCommand {
                command: command.to_owned(),
            },
        )
        .await
    {
        Ok(result) => {
            publish_stream(&context.events, job, LogChannel::Stdout, &result.stdout);
            publish_stream(&context.events, job, LogChannel::Stderr, &result.stderr);

            if result.exit_code == 0 {
                succeed_job(
                    &context.jobs,
                    &context.events,
                    &context.artifacts,
                    job,
                    "ssh.exec completed",
                );
            } else {
                fail_job(
                    &context.jobs,
                    &context.events,
                    &context.artifacts,
                    job,
                    format!("ssh.exec exited with status {}", result.exit_code),
                );
            }
        }
        Err(error) => fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            error.to_string(),
        ),
    }
}

#[cfg(feature = "ssh")]
async fn execute_ssh_reboot(
    context: &ActionExecutionContext,
    ssh_adapter: &SshAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.ssh.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing ssh configuration",
        );
        return;
    };

    context.events.publish(adapter_event(
        job,
        LogChannel::System,
        LogLevel::Info,
        "reboot requested",
    ));

    match ssh_adapter.reboot(config).await {
        Ok(()) => succeed_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "ssh.reboot completed",
        ),
        Err(error) => fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            error.to_string(),
        ),
    }
}

#[cfg(feature = "ssh")]
async fn execute_ssh_tail(
    context: &ActionExecutionContext,
    ssh_adapter: &SshAdapter,
    job: &Job,
    device: &Device,
    payload: Option<&ActionRequest>,
) {
    let Some(config) = device.ssh.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing ssh configuration",
        );
        return;
    };
    let path = payload
        .and_then(|request| request.payload.as_ref())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(config.default_log_path.trim());

    context.events.publish(adapter_event(
        job,
        LogChannel::System,
        LogLevel::Info,
        format!("tailing logs from {path}"),
    ));

    match ssh_adapter.tail_logs_stream(config, path).await {
        Ok(mut session) => loop {
            match session.next_chunk().await {
                Ok(Some(chunk)) => context.events.publish(adapter_event(
                    job,
                    match chunk.channel {
                        SshStreamChannel::Stdout => LogChannel::Stdout,
                        SshStreamChannel::Stderr => LogChannel::Stderr,
                    },
                    LogLevel::Info,
                    chunk.line,
                )),
                Ok(None) => {
                    succeed_job(
                        &context.jobs,
                        &context.events,
                        &context.artifacts,
                        job,
                        "ssh.tail completed",
                    );
                    break;
                }
                Err(error) => {
                    fail_job(
                        &context.jobs,
                        &context.events,
                        &context.artifacts,
                        job,
                        error.to_string(),
                    );
                    break;
                }
            }
        },
        Err(error) => fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            error.to_string(),
        ),
    }
}

#[cfg(feature = "serial")]
async fn execute_serial_tail(
    context: &ActionExecutionContext,
    serial_adapter: &SerialAdapter,
    job: &Job,
    device: &Device,
    reconnect_policy: SerialReconnectPolicy,
) {
    let Some(config) = device.serial.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing serial configuration",
        );
        return;
    };

    let _system_awake_guard =
        if reconnect_policy.prevent_system_sleep && SystemAwakeGuard::supported() {
            match SystemAwakeGuard::acquire() {
                Ok(guard) => {
                    context.events.publish(adapter_event_with_source(
                        job,
                        LogSource::System,
                        LogChannel::System,
                        LogLevel::Info,
                        "Windows system sleep inhibition active for serial.tail",
                    ));
                    Some(guard)
                }
                Err(error) => {
                    context.events.publish(adapter_event_with_source(
                        job,
                        LogSource::System,
                        LogChannel::System,
                        LogLevel::Warn,
                        format!("failed to inhibit Windows system sleep: {error}"),
                    ));
                    None
                }
            }
        } else {
            None
        };

    context.events.publish(adapter_event_with_source(
        job,
        LogSource::Serial,
        LogChannel::System,
        LogLevel::Info,
        format!(
            "tailing serial logs from {} @ {} baud",
            config.port, config.baud
        ),
    ));

    let mut reconnect_attempts = 0_u32;
    let mut previous_endpoint = None::<String>;
    let mut disconnected_since = None::<Instant>;
    let mut total_disconnected = Duration::ZERO;
    loop {
        if reconnect_attempts > 0 {
            context.events.publish(adapter_event_with_source(
                job,
                LogSource::Serial,
                LogChannel::System,
                LogLevel::Info,
                format!(
                    "serial reconnecting: attempt {reconnect_attempts}/{}; previous_endpoint={}",
                    reconnect_policy
                        .max_attempts
                        .map(|maximum| maximum.to_string())
                        .unwrap_or_else(|| "unbounded".into()),
                    previous_endpoint.as_deref().unwrap_or("none")
                ),
            ));
        }

        match serial_adapter.tail_logs_stream(config).await {
            Ok(mut session) => {
                let endpoint = session.endpoint().to_owned();
                context.jobs.set_runtime_endpoint(&job.id, endpoint.clone());
                let outage = disconnected_since.take().map(|started| started.elapsed());
                if let Some(outage) = outage {
                    total_disconnected += outage;
                }
                let message = match previous_endpoint.as_deref() {
                    Some(previous) => format!(
                        "serial reconnected: {previous} -> {endpoint}; outage_ms={}; total_disconnected_ms={}",
                        outage.unwrap_or_default().as_millis(),
                        total_disconnected.as_millis()
                    ),
                    None => format!("serial connected: {endpoint}"),
                };
                reconnect_attempts = 0;
                context.events.publish(adapter_event_with_source(
                    job,
                    LogSource::Serial,
                    LogChannel::System,
                    LogLevel::Info,
                    message,
                ));

                let disconnect_reason = loop {
                    match session.next_chunk().await {
                        Ok(Some(chunk)) => {
                            context.events.publish(LogEvent {
                                ts: Utc::now(),
                                device_id: job.device_id.clone(),
                                job_id: job.id.clone(),
                                source: LogSource::Serial,
                                channel: LogChannel::Stdout,
                                level: LogLevel::Info,
                                message: chunk.line,
                            });
                        }
                        Ok(None) => break "unexpected EOF".to_owned(),
                        Err(error) => break error.to_string(),
                    }
                };
                previous_endpoint = Some(endpoint.clone());
                disconnected_since.get_or_insert_with(Instant::now);
                context.jobs.clear_runtime_endpoint(&job.id);
                context.events.publish(adapter_event_with_source(
                    job,
                    LogSource::Serial,
                    LogChannel::System,
                    LogLevel::Warn,
                    format!("serial disconnected from {endpoint}: {disconnect_reason}"),
                ));
            }
            Err(error) if !serial_error_is_recoverable(&error) => {
                fail_job_with_source(
                    &context.jobs,
                    &context.events,
                    &context.artifacts,
                    job,
                    LogSource::Serial,
                    error.to_string(),
                );
                return;
            }
            Err(error) => {
                disconnected_since.get_or_insert_with(Instant::now);
                context.events.publish(adapter_event_with_source(
                    job,
                    LogSource::Serial,
                    LogChannel::System,
                    LogLevel::Warn,
                    format!("serial reconnect open failed: {error}"),
                ));
            }
        }

        reconnect_attempts += 1;
        if reconnect_policy
            .max_attempts
            .is_some_and(|maximum| reconnect_attempts > maximum)
        {
            fail_job_with_source(
                &context.jobs,
                &context.events,
                &context.artifacts,
                job,
                LogSource::Serial,
                format!(
                    "serial reconnect exhausted after {} attempts",
                    reconnect_policy.max_attempts.unwrap_or_default()
                ),
            );
            return;
        }

        let delay = reconnect_delay(
            reconnect_attempts,
            reconnect_policy.initial_delay,
            reconnect_policy.max_delay,
        );
        context.events.publish(adapter_event_with_source(
            job,
            LogSource::Serial,
            LogChannel::System,
            LogLevel::Info,
            format!(
                "waiting_for_device: reconnect attempt {reconnect_attempts}/{} in {} ms",
                reconnect_policy
                    .max_attempts
                    .map(|maximum| maximum.to_string())
                    .unwrap_or_else(|| "unbounded".into()),
                delay.as_millis()
            ),
        ));
        tokio::time::sleep(delay).await;
    }
}

#[cfg(feature = "serial")]
fn serial_error_is_recoverable(error: &SerialAdapterError) -> bool {
    !matches!(
        error,
        SerialAdapterError::Busy { .. }
            | SerialAdapterError::EmptySelector
            | SerialAdapterError::SelectorAmbiguous { .. }
    )
}

#[cfg(feature = "serial")]
fn reconnect_delay(attempt: u32, initial: Duration, maximum: Duration) -> Duration {
    let factor = 1_u32
        .checked_shl(attempt.saturating_sub(1).min(16))
        .unwrap_or(u32::MAX);
    initial.saturating_mul(factor).min(maximum)
}

#[cfg(feature = "adb")]
async fn execute_adb_logcat(
    context: &ActionExecutionContext,
    adb_adapter: &AdbAdapter,
    job: &Job,
    device: &Device,
    payload: Option<&ActionRequest>,
) {
    let Some(config) = device.adb.as_ref() else {
        fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Adb,
            "device is missing adb configuration",
        );
        return;
    };
    let args = adb_logcat_args(config, payload);

    context.events.publish(adapter_event_with_source(
        job,
        LogSource::Adb,
        LogChannel::System,
        LogLevel::Info,
        format!(
            "tailing adb logcat from {} with args: {}",
            config.serial,
            args.join(" ")
        ),
    ));

    match adb_adapter.logcat(config, &args).await {
        Ok(mut session) => loop {
            match session.next_line().await {
                Ok(Some(line)) => context.events.publish(LogEvent {
                    ts: Utc::now(),
                    device_id: job.device_id.clone(),
                    job_id: job.id.clone(),
                    source: LogSource::Adb,
                    channel: LogChannel::Stdout,
                    level: LogLevel::Info,
                    message: line,
                }),
                Ok(None) => {
                    succeed_job(
                        &context.jobs,
                        &context.events,
                        &context.artifacts,
                        job,
                        "adb.logcat completed",
                    );
                    break;
                }
                Err(error) => {
                    fail_job_with_source(
                        &context.jobs,
                        &context.events,
                        &context.artifacts,
                        job,
                        LogSource::Adb,
                        error.to_string(),
                    );
                    break;
                }
            }
        },
        Err(error) => fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Adb,
            error.to_string(),
        ),
    }
}

#[cfg(feature = "adb")]
fn adb_logcat_args(
    config: &device_hub_shared::AdbConfig,
    payload: Option<&ActionRequest>,
) -> Vec<String> {
    payload
        .and_then(|request| request.payload.as_deref())
        .map(|payload| payload.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_else(|| config.default_logcat_args.clone())
}

#[cfg(feature = "serial")]
async fn execute_serial_write(
    context: &ActionExecutionContext,
    serial_adapter: &SerialAdapter,
    job: &Job,
    device: &Device,
    payload: Option<&ActionRequest>,
) {
    let Some(config) = device.serial.as_ref() else {
        fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Serial,
            "device is missing serial configuration",
        );
        return;
    };
    let input = payload
        .and_then(|request| request.payload.as_deref())
        .unwrap_or_default();

    context.events.publish(adapter_event_with_source(
        job,
        LogSource::Serial,
        LogChannel::System,
        LogLevel::Info,
        format!("writing serial input {}", input.escape_debug()),
    ));

    match serial_adapter.write(config, input.as_bytes()).await {
        Ok(bytes_written) => {
            context.events.publish(adapter_event_with_source(
                job,
                LogSource::Serial,
                LogChannel::System,
                LogLevel::Info,
                format!(
                    "wrote {bytes_written} bytes to {} @ {} baud",
                    config.port, config.baud
                ),
            ));
            succeed_job(
                &context.jobs,
                &context.events,
                &context.artifacts,
                job,
                "serial.write completed",
            );
        }
        Err(error) => fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Serial,
            error.to_string(),
        ),
    }
}

#[cfg(feature = "serial")]
async fn execute_serial_exec(
    context: &ActionExecutionContext,
    serial_command_executor: &DynSerialCommandExecutor,
    job: &Job,
    device: Device,
    payload: Option<&ActionRequest>,
) {
    let Some(request) = serial_exec_request_from_action_payload(payload) else {
        fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Serial,
            "action `serial.exec` requires a command payload",
        );
        return;
    };

    context.events.publish(adapter_event_with_source(
        job,
        LogSource::Serial,
        LogChannel::System,
        LogLevel::Info,
        format!("serial exec: {}", request.command),
    ));

    match serial_command_executor.exec(device, request).await {
        Ok(result) => {
            if !result.output.is_empty() {
                publish_stream_with_source(
                    &context.events,
                    job,
                    LogSource::Serial,
                    LogChannel::Stdout,
                    &result.output,
                );
            }
            persist_json_artifact(&context.artifacts, job, "result.json", &result);
            if let Err(error) = context.artifacts.write_named_artifact(
                &job.id,
                "output.txt",
                result.output.as_bytes(),
            ) {
                eprintln!("failed to persist job {} output.txt: {error}", job.id);
            }

            if result.timed_out {
                fail_job_with_source(
                    &context.jobs,
                    &context.events,
                    &context.artifacts,
                    job,
                    LogSource::Serial,
                    "serial.exec timed out before matching expected output",
                );
            } else {
                succeed_job(
                    &context.jobs,
                    &context.events,
                    &context.artifacts,
                    job,
                    "serial.exec completed",
                );
            }
        }
        Err(error) => fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Serial,
            error.to_string(),
        ),
    }
}

#[cfg(feature = "jlink")]
async fn execute_jlink_probe(
    context: &ActionExecutionContext,
    jlink_adapter: &JlinkAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.jlink.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing jlink configuration",
        );
        return;
    };

    context.events.publish(adapter_event_with_source(
        job,
        LogSource::Jlink,
        LogChannel::System,
        LogLevel::Info,
        format!("probing J-Link {} for {}", config.serial, config.device),
    ));

    match jlink_adapter.probe(config).await {
        Ok(execution) => {
            persist_jlink_execution(&context.events, &context.artifacts, job, &execution);
            succeed_job(
                &context.jobs,
                &context.events,
                &context.artifacts,
                job,
                "jlink.probe completed",
            );
        }
        Err(error) => fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Jlink,
            persist_jlink_failure(&context.events, &context.artifacts, job, error),
        ),
    }
}

#[cfg(feature = "jlink")]
async fn execute_jlink_flash(
    context: &ActionExecutionContext,
    jlink_adapter: &JlinkAdapter,
    job: &Job,
    device: &Device,
    payload: Option<&ActionRequest>,
) {
    let Some(config) = device.jlink.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing jlink configuration",
        );
        return;
    };
    let firmware_path = payload
        .and_then(|request| request.payload.as_deref())
        .map(str::trim)
        .unwrap_or_default();

    context.events.publish(adapter_event_with_source(
        job,
        LogSource::Jlink,
        LogChannel::System,
        LogLevel::Info,
        format!("flashing {} via J-Link {}", firmware_path, config.serial),
    ));

    match jlink_adapter
        .flash(
            config,
            &FlashRequest {
                firmware_path: firmware_path.to_owned(),
            },
        )
        .await
    {
        Ok(execution) => {
            persist_jlink_execution(&context.events, &context.artifacts, job, &execution);
            succeed_job(
                &context.jobs,
                &context.events,
                &context.artifacts,
                job,
                "jlink.flash completed",
            );
        }
        Err(error) => fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Jlink,
            persist_jlink_failure(&context.events, &context.artifacts, job, error),
        ),
    }
}

#[cfg(feature = "jlink")]
async fn execute_jlink_verify(
    context: &ActionExecutionContext,
    jlink_adapter: &JlinkAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.jlink.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing jlink configuration",
        );
        return;
    };

    context.events.publish(adapter_event_with_source(
        job,
        LogSource::Jlink,
        LogChannel::System,
        LogLevel::Info,
        format!(
            "verifying target {} via J-Link {}",
            config.device, config.serial
        ),
    ));

    match jlink_adapter.verify(config).await {
        Ok(execution) => {
            persist_jlink_execution(&context.events, &context.artifacts, job, &execution);
            succeed_job(
                &context.jobs,
                &context.events,
                &context.artifacts,
                job,
                "jlink.verify completed",
            );
        }
        Err(error) => fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Jlink,
            persist_jlink_failure(&context.events, &context.artifacts, job, error),
        ),
    }
}

#[cfg(feature = "jlink")]
async fn execute_jlink_reset(
    context: &ActionExecutionContext,
    jlink_adapter: &JlinkAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.jlink.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing jlink configuration",
        );
        return;
    };

    context.events.publish(adapter_event_with_source(
        job,
        LogSource::Jlink,
        LogChannel::System,
        LogLevel::Info,
        format!(
            "resetting target {} via J-Link {}",
            config.device, config.serial
        ),
    ));

    match jlink_adapter.reset(config).await {
        Ok(execution) => {
            persist_jlink_execution(&context.events, &context.artifacts, job, &execution);
            succeed_job(
                &context.jobs,
                &context.events,
                &context.artifacts,
                job,
                "jlink.reset completed",
            );
        }
        Err(error) => fail_job_with_source(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            LogSource::Jlink,
            persist_jlink_failure(&context.events, &context.artifacts, job, error),
        ),
    }
}

#[cfg(feature = "jlink")]
async fn execute_jlink_erase(
    context: &ActionExecutionContext,
    jlink_adapter: &JlinkAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.jlink.as_ref() else {
        fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            "device is missing jlink configuration",
        );
        return;
    };

    context.events.publish(adapter_event_with_source(
        job,
        LogSource::Jlink,
        LogChannel::System,
        LogLevel::Info,
        format!(
            "erasing target {} via J-Link {}",
            config.device, config.serial
        ),
    ));

    match jlink_adapter.erase(config).await {
        Ok(execution) => {
            persist_jlink_execution(&context.events, &context.artifacts, job, &execution);
            succeed_job(
                &context.jobs,
                &context.events,
                &context.artifacts,
                job,
                "jlink.erase completed",
            );
        }
        Err(error) => fail_job(
            &context.jobs,
            &context.events,
            &context.artifacts,
            job,
            error.to_string(),
        ),
    }
}

#[cfg(feature = "ssh")]
fn publish_stream(events: &crate::events::EventBus, job: &Job, channel: LogChannel, output: &str) {
    for line in output.lines().filter(|line| !line.is_empty()) {
        events.publish(adapter_event(job, channel.clone(), LogLevel::Info, line));
    }
}

#[cfg(feature = "serial")]
fn publish_stream_with_source(
    events: &crate::events::EventBus,
    job: &Job,
    source: LogSource,
    channel: LogChannel,
    output: &str,
) {
    for line in output.lines().filter(|line| !line.is_empty()) {
        events.publish(adapter_event_with_source(
            job,
            source.clone(),
            channel.clone(),
            LogLevel::Info,
            line,
        ));
    }
}

#[cfg(feature = "serial")]
fn serial_exec_request_from_action_payload(
    payload: Option<&ActionRequest>,
) -> Option<SerialExecRequest> {
    let payload = payload
        .and_then(|request| request.payload.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    if let Ok(request) = serde_json::from_str::<SerialExecRequest>(payload)
        && !request.command.trim().is_empty()
    {
        return Some(request);
    }

    Some(SerialExecRequest {
        command: payload.to_owned(),
        timeout_secs: None,
        until: None,
    })
}

#[cfg(feature = "serial")]
fn persist_json_artifact<T: Serialize>(
    artifacts: &crate::storage::artifact_store::ArtifactStore,
    job: &Job,
    artifact_name: &str,
    value: &T,
) {
    match serde_json::to_vec_pretty(value) {
        Ok(json) => {
            if let Err(error) = artifacts.write_named_artifact(&job.id, artifact_name, &json) {
                eprintln!(
                    "failed to persist job {} artifact {}: {error}",
                    job.id, artifact_name
                );
            }
        }
        Err(error) => eprintln!("failed to serialize job {} result payload: {error}", job.id),
    }
}

#[cfg(feature = "jlink")]
fn persist_jlink_execution<T: Serialize>(
    events: &crate::events::EventBus,
    artifacts: &crate::storage::artifact_store::ArtifactStore,
    job: &Job,
    execution: &JlinkExecution<T>,
) {
    for artifact in &execution.artifacts {
        publish_jlink_artifact_logs(events, job, artifact);
        if let Err(error) =
            artifacts.write_named_artifact(&job.id, &artifact.name, &artifact.contents)
        {
            eprintln!(
                "failed to persist job {} artifact {}: {error}",
                job.id, artifact.name
            );
        }
    }

    match serde_json::to_vec_pretty(&execution.result) {
        Ok(result_json) => {
            if let Err(error) = artifacts.write_named_artifact(&job.id, "result.json", &result_json)
            {
                eprintln!("failed to persist job {} result.json: {error}", job.id);
            }
        }
        Err(error) => eprintln!("failed to serialize job {} result payload: {error}", job.id),
    }
}

#[cfg(feature = "jlink")]
fn publish_jlink_artifact_logs(
    events: &crate::events::EventBus,
    job: &Job,
    artifact: &JlinkArtifact,
) {
    let (channel, level) = match artifact.name.as_str() {
        "jlink.stdout.log" => (LogChannel::Stdout, LogLevel::Info),
        "jlink.stderr.log" => (LogChannel::Stderr, LogLevel::Warn),
        _ => return,
    };
    let text = String::from_utf8_lossy(&artifact.contents);
    for line in text.lines().filter(|line| !line.is_empty()) {
        events.publish(adapter_event_with_source(
            job,
            LogSource::Jlink,
            channel.clone(),
            level.clone(),
            line,
        ));
    }
}

#[cfg(feature = "jlink")]
fn persist_jlink_failure(
    events: &crate::events::EventBus,
    artifacts: &crate::storage::artifact_store::ArtifactStore,
    job: &Job,
    error: JlinkAdapterError,
) -> String {
    if let Some(jlink_artifacts) = error.artifacts() {
        for artifact in jlink_artifacts {
            publish_jlink_artifact_logs(events, job, artifact);
            if let Err(write_error) =
                artifacts.write_named_artifact(&job.id, &artifact.name, &artifact.contents)
            {
                eprintln!(
                    "failed to persist job {} artifact {}: {write_error}",
                    job.id, artifact.name
                );
            }
        }
    }

    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct NeverRunHandler;

    impl AdapterActionHandler for NeverRunHandler {
        fn run(
            &self,
            _context: ActionExecutionContext,
            _job: Job,
            _device: Device,
            _action: String,
            _payload: Option<ActionRequest>,
        ) -> BoxFuture<'static, ()> {
            Box::pin(async { panic!("handler must not run in registry construction tests") })
        }
    }

    fn serial_capability() -> AdapterCapability {
        AdapterCapability {
            adapter_id: "serial".into(),
            compiled: true,
            enabled: true,
            available: false,
            actions: vec!["serial.tail".into()],
            unavailable_reason: Some("not_registered".into()),
        }
    }

    #[test]
    fn registration_makes_declared_capability_available() {
        let mut registry = CapabilityRegistry::new(vec![serial_capability()]);

        registry
            .register("serial", &["serial.tail"], NeverRunHandler)
            .expect("register action");

        let snapshot = registry.snapshot();
        let capability = snapshot.adapter("serial").expect("serial capability");
        assert!(capability.available);
        assert_eq!(capability.unavailable_reason, None);
    }

    #[test]
    fn duplicate_action_registration_is_rejected() {
        let mut registry = CapabilityRegistry::new(vec![serial_capability()]);
        registry
            .register("serial", &["serial.tail"], NeverRunHandler)
            .expect("register action");

        let error = registry
            .register("serial", &["serial.tail"], NeverRunHandler)
            .expect_err("duplicate registration must fail");

        assert_eq!(
            error,
            CapabilityRegistryError::DuplicateAction("serial.tail".into())
        );
    }

    #[test]
    fn unknown_adapter_registration_is_rejected() {
        let mut registry = CapabilityRegistry::new(vec![serial_capability()]);

        let error = registry
            .register("unknown", &["serial.tail"], NeverRunHandler)
            .expect_err("unknown adapter registration must fail");

        assert_eq!(
            error,
            CapabilityRegistryError::UnknownAdapter("unknown".into())
        );
    }

    #[test]
    fn undeclared_action_registration_is_rejected() {
        let mut registry = CapabilityRegistry::new(vec![serial_capability()]);

        let error = registry
            .register("serial", &["serial.unknown"], NeverRunHandler)
            .expect_err("undeclared registration must fail");

        assert_eq!(
            error,
            CapabilityRegistryError::UndeclaredAction {
                adapter_id: "serial".into(),
                action: "serial.unknown".into(),
            }
        );
    }

    #[test]
    fn partial_adapter_registration_is_rejected() {
        let mut capability = serial_capability();
        capability.actions.push("serial.write".into());
        let mut registry = CapabilityRegistry::new(vec![capability]);

        let error = registry
            .register("serial", &["serial.tail"], NeverRunHandler)
            .expect_err("partial registration must fail");

        assert_eq!(
            error,
            CapabilityRegistryError::IncompleteAdapterRegistration {
                adapter_id: "serial".into(),
                actions: "serial.write".into(),
            }
        );
    }
}
