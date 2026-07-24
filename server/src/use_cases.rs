use device_hub_shared::{
    ActionRequest, ActionStartResponse, ActiveTailStatus, BoardProfile, CapabilitySnapshot, Device,
    DeviceDetail, DeviceRuntimeStatus, DiscoveredJlinkProbe, DiscoveredSerialPort, EventFilter,
    Job, JobEvidence, JobStatus, JobSummary, LogChannel, LogEvent, LogLevel, SerialExecRequest,
    SerialExecResult,
};
use std::{path::PathBuf, sync::Arc};

use crate::{
    error::AppError,
    events::EventBus,
    ports::{
        DeviceRegistryError, DynCapabilityProvider, DynDeviceRegistry, DynHostDiscovery,
        DynJobEvidenceStore, DynSerialCommandExecutor, HostDiscoveryError, PersistedEventSummary,
        SerialCommandError,
    },
    services::job_manager::JobManager,
    storage::job_store::JobStore,
};

#[derive(Clone)]
pub struct AppUseCases {
    board_profiles: Arc<Vec<BoardProfile>>,
    devices: DynDeviceRegistry,
    host_discovery: DynHostDiscovery,
    jobs: JobStore,
    job_manager: JobManager,
    events: EventBus,
    evidence_store: DynJobEvidenceStore,
    serial_command_executor: DynSerialCommandExecutor,
    capabilities: DynCapabilityProvider,
}

pub struct AppUseCaseDependencies {
    pub devices: DynDeviceRegistry,
    pub host_discovery: DynHostDiscovery,
    pub jobs: JobStore,
    pub job_manager: JobManager,
    pub events: EventBus,
    pub evidence_store: DynJobEvidenceStore,
    pub serial_command_executor: DynSerialCommandExecutor,
    pub capabilities: DynCapabilityProvider,
}

impl AppUseCases {
    pub fn new(board_profiles: Vec<BoardProfile>, dependencies: AppUseCaseDependencies) -> Self {
        Self {
            board_profiles: Arc::new(board_profiles),
            devices: dependencies.devices,
            host_discovery: dependencies.host_discovery,
            jobs: dependencies.jobs,
            job_manager: dependencies.job_manager,
            events: dependencies.events,
            evidence_store: dependencies.evidence_store,
            serial_command_executor: dependencies.serial_command_executor,
            capabilities: dependencies.capabilities,
        }
    }

    pub fn list_capabilities(&self) -> CapabilitySnapshot {
        self.capabilities.snapshot()
    }

    pub fn list_board_profiles(&self) -> Vec<BoardProfile> {
        let mut profiles = self.board_profiles.as_ref().clone();
        profiles.sort_by(|left, right| left.id.cmp(&right.id));
        profiles
    }

    pub fn list_devices(&self) -> Vec<Device> {
        self.devices.list()
    }

    pub fn get_device(&self, device_id: &str) -> Result<Device, AppError> {
        self.devices.get(device_id).ok_or(AppError::DeviceNotFound)
    }

    pub fn get_device_detail(&self, device_id: &str) -> Result<DeviceDetail, AppError> {
        let device = self.get_device(device_id)?;
        Ok(device.detail_with_runtime(self.device_runtime_status(device_id)))
    }

    pub fn create_device(&self, device: Device) -> Result<Device, AppError> {
        validate_device(&device)?;
        self.devices
            .create(device)
            .map_err(map_device_registry_error)
    }

    pub fn update_device(&self, device_id: &str, device: Device) -> Result<Device, AppError> {
        if device.id != device_id {
            return Err(AppError::BadRequest(
                "device id in path must match request body".into(),
            ));
        }
        validate_device(&device)?;
        ensure_device_has_no_active_jobs(&self.jobs, device_id)?;
        self.devices
            .update(device)
            .map_err(map_device_registry_error)
    }

    pub fn delete_device(&self, device_id: &str) -> Result<Device, AppError> {
        ensure_device_has_no_active_jobs(&self.jobs, device_id)?;
        self.devices
            .delete(device_id)
            .map_err(map_device_registry_error)
    }

    pub async fn list_serial_ports(&self) -> Result<Vec<DiscoveredSerialPort>, AppError> {
        ensure_adapter_available("serial", &self.capabilities.snapshot())?;
        let mut ports = self
            .host_discovery
            .list_serial_ports()
            .await
            .map_err(map_host_discovery_error)?;
        ports.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(ports)
    }

    pub async fn list_jlink_probes(&self) -> Result<Vec<DiscoveredJlinkProbe>, AppError> {
        ensure_adapter_available("jlink", &self.capabilities.snapshot())?;
        let mut probes = self
            .host_discovery
            .list_jlink_probes()
            .await
            .map_err(map_host_discovery_error)?;
        probes.sort_by(|left, right| left.serial.cmp(&right.serial));
        Ok(probes)
    }

    pub async fn exec_serial_command(
        &self,
        device_id: &str,
        request: SerialExecRequest,
    ) -> Result<SerialExecResult, AppError> {
        let device = self.get_device(device_id)?;
        validate_action("serial.exec", &device, &self.capabilities.snapshot())?;
        validate_serial_exec_request(&request)?;
        ensure_device_has_no_active_jobs_except(&self.jobs, device_id, "serial.tail")?;
        self.serial_command_executor
            .exec(device, request)
            .await
            .map_err(map_serial_command_error)
    }

    pub fn list_jobs(&self, device_id: Option<&str>) -> Vec<Job> {
        let mut jobs = self.jobs.list();
        if let Some(device_id) = device_id {
            jobs.retain(|job| job.device_id == device_id);
        }
        jobs
    }

    pub fn get_job(&self, job_id: &str) -> Result<Job, AppError> {
        self.jobs.get(job_id).ok_or(AppError::JobNotFound)
    }

    pub fn get_job_events(
        &self,
        job_id: &str,
        filter: &EventFilter,
        tail: Option<usize>,
    ) -> Result<Vec<LogEvent>, AppError> {
        self.get_job(job_id)?;
        self.events
            .flush_job(job_id)
            .map_err(|error| AppError::Internal(error.to_string()))?;

        let persisted = if let Some(tail) = tail {
            self.evidence_store.read_tail_events(job_id, tail, filter)
        } else {
            self.evidence_store.read_events(job_id)
        }
        .map_err(|error| AppError::Internal(error.to_string()))?;

        let events = persisted.unwrap_or_else(|| {
            self.events.snapshot(&EventFilter {
                job_id: Some(job_id.to_owned()),
                ..EventFilter::default()
            })
        });
        let filtered = events
            .into_iter()
            .filter(|event| filter.matches(event))
            .collect::<Vec<_>>();
        Ok(tail_events(filtered, tail))
    }

    pub fn get_job_evidence(&self, job_id: &str) -> Result<JobEvidence, AppError> {
        const EVIDENCE_EVENT_SAMPLE_LIMIT: usize = 128;

        let job = self.get_job(job_id)?;
        self.events
            .flush_job(job_id)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let summary = if let Some(summary) = self
            .evidence_store
            .read_event_summary(job_id)
            .map_err(|error| AppError::Internal(error.to_string()))?
        {
            summary
        } else {
            let events = self
                .evidence_store
                .read_tail_events(job_id, EVIDENCE_EVENT_SAMPLE_LIMIT, &EventFilter::default())
                .map_err(|error| AppError::Internal(error.to_string()))?
                .unwrap_or_else(|| {
                    self.events.snapshot(&EventFilter {
                        job_id: Some(job_id.to_owned()),
                        ..EventFilter::default()
                    })
                });
            summarize_events(&events)
        };
        let artifacts = self
            .evidence_store
            .list_job_artifacts(job_id)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let device = self.get_device(&job.device_id).ok();

        Ok(JobEvidence {
            summary: build_job_summary(&job, device.as_ref(), &summary, &artifacts),
            job,
            artifacts,
        })
    }

    pub fn get_job_artifact_path(
        &self,
        job_id: &str,
        artifact_name: &str,
    ) -> Result<PathBuf, AppError> {
        self.get_job(job_id)?;
        self.events
            .flush_job(job_id)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        self.evidence_store
            .artifact_path(job_id, artifact_name)
            .ok_or(AppError::JobNotFound)
    }

    pub async fn start_action(
        &self,
        device_id: &str,
        action: String,
        payload: Option<ActionRequest>,
        reuse: bool,
    ) -> Result<ActionStartResponse, AppError> {
        let device = self.get_device(device_id)?;
        validate_action(&action, &device, &self.capabilities.snapshot())?;
        validate_payload(&action, &device, payload.as_ref())?;
        if reuse
            && matches!(action.as_str(), "serial.tail" | "adb.logcat")
            && let Some(job) = find_active_job(&self.jobs, device_id, &action)
        {
            return Ok(ActionStartResponse {
                job,
                already_running: true,
            });
        }

        let job = self.job_manager.start(device, action, payload).await?;
        Ok(ActionStartResponse {
            job,
            already_running: false,
        })
    }

    pub async fn cancel_job(&self, job_id: &str) -> Result<Job, AppError> {
        self.job_manager.cancel(job_id).await
    }

    pub async fn rerun_job(&self, job_id: &str) -> Result<Job, AppError> {
        let previous = self.get_job(job_id)?;
        let device = self.get_device(&previous.device_id)?;
        validate_action(&previous.action, &device, &self.capabilities.snapshot())?;
        let payload = previous.payload.clone().map(|payload| ActionRequest {
            payload: Some(payload),
        });
        self.job_manager
            .start(device, previous.action.clone(), payload)
            .await
    }

    pub fn clear_event_cache(&self) -> usize {
        self.events.clear_cache()
    }

    fn device_runtime_status(&self, device_id: &str) -> DeviceRuntimeStatus {
        let active_tails = self
            .jobs
            .list()
            .into_iter()
            .filter(|job| {
                job.device_id == device_id
                    && is_tail_action(&job.action)
                    && matches!(job.status, JobStatus::Pending | JobStatus::Running)
            })
            .map(|job| ActiveTailStatus {
                action: job.action,
                current_endpoint: self.jobs.runtime_endpoint(&job.id),
                job_id: job.id,
                active_since: job.created_at,
            })
            .collect();

        DeviceRuntimeStatus { active_tails }
    }
}

fn tail_events(events: Vec<LogEvent>, tail: Option<usize>) -> Vec<LogEvent> {
    let Some(tail) = tail else {
        return events;
    };
    let start = events.len().saturating_sub(tail);
    events.into_iter().skip(start).collect()
}

fn build_job_summary(
    job: &Job,
    device: Option<&Device>,
    event_summary: &PersistedEventSummary,
    artifacts: &[device_hub_shared::JobArtifact],
) -> JobSummary {
    let stdout_lines = event_summary.stdout_events;
    let stderr_lines = event_summary.stderr_events;
    let system_events = event_summary.system_events;

    let request = match job.action.as_str() {
        "ssh.exec" => job
            .payload
            .as_ref()
            .map(|payload| format!("command: {payload}")),
        "ssh.tail" => job
            .payload
            .as_ref()
            .map(|payload| format!("path: {payload}"))
            .or_else(|| {
                device.and_then(|device| {
                    device
                        .ssh
                        .as_ref()
                        .map(|ssh| format!("path: {}", ssh.default_log_path))
                })
            }),
        "serial.tail" => device.and_then(|device| {
            device
                .serial
                .as_ref()
                .map(|serial| format!("serial: {} @ {} baud", serial.port, serial.baud))
        }),
        "adb.logcat" => device.and_then(|device| {
            device
                .adb
                .as_ref()
                .map(|adb| format!("adb: {} logcat", adb.serial))
        }),
        "serial.write" => job
            .payload
            .as_ref()
            .map(|payload| format!("input: {}", payload.escape_debug())),
        "serial.exec" => job
            .payload
            .as_ref()
            .map(|payload| format!("command: {}", payload.escape_debug())),
        "ssh.reboot" => device.and_then(|device| {
            device
                .ssh
                .as_ref()
                .map(|ssh| format!("target: {}@{}", ssh.user, ssh.host))
        }),
        "jlink.probe" | "jlink.verify" | "jlink.reset" | "jlink.erase" => device
            .and_then(|device| device.jlink.as_ref())
            .map(|jlink| format!("probe: {} on {}", jlink.serial, jlink.device)),
        "jlink.flash" => job
            .payload
            .as_ref()
            .map(|payload| format!("firmware: {payload}")),
        _ => job
            .payload
            .as_ref()
            .map(|payload| format!("payload: {payload}")),
    };

    let mut highlights = vec![format!(
        "captured {stdout_lines} stdout lines, {stderr_lines} stderr lines, and {system_events} system events"
    )];
    highlights.push(format!(
        "persisted {} artifact files under the server artifact root",
        artifacts.len()
    ));
    if matches!(job.status, JobStatus::Failed | JobStatus::Cancelled)
        && let Some(last_error) = &event_summary.last_error
    {
        highlights.push(format!("last error: {last_error}"));
    }

    JobSummary {
        headline: format!(
            "{} {} on {}",
            job.action,
            match job.status {
                JobStatus::Pending => "is pending",
                JobStatus::Running => "is running",
                JobStatus::Succeeded => "succeeded",
                JobStatus::Failed => "failed",
                JobStatus::Cancelled => "was cancelled",
            },
            job.device_id
        ),
        request,
        outcome: event_summary.outcome.clone(),
        highlights,
    }
}

fn summarize_events(events: &[LogEvent]) -> PersistedEventSummary {
    let mut summary = PersistedEventSummary::default();
    for event in events {
        match event.channel {
            LogChannel::Stdout => summary.stdout_events += 1,
            LogChannel::Stderr => summary.stderr_events += 1,
            LogChannel::System => summary.system_events += 1,
        }
        if matches!(event.level, LogLevel::Error) {
            summary.last_error = Some(event.message.clone());
        }
        if matches!(event.level, LogLevel::Error)
            || event.source == device_hub_shared::LogSource::Job
        {
            summary.outcome = Some(event.message.clone());
        }
    }
    summary
}

fn map_device_registry_error(error: DeviceRegistryError) -> AppError {
    match error {
        DeviceRegistryError::NotFound => AppError::DeviceNotFound,
        DeviceRegistryError::AlreadyExists(device_id) => {
            AppError::Conflict(format!("device `{device_id}` already exists"))
        }
        DeviceRegistryError::Persistence(message) => AppError::Internal(message),
    }
}

fn map_host_discovery_error(error: HostDiscoveryError) -> AppError {
    AppError::Internal(error.to_string())
}

fn map_serial_command_error(error: SerialCommandError) -> AppError {
    AppError::Internal(error.to_string())
}

fn ensure_device_has_no_active_jobs(jobs: &JobStore, device_id: &str) -> Result<(), AppError> {
    ensure_device_has_no_active_jobs_except(jobs, device_id, "")
}

fn ensure_device_has_no_active_jobs_except(
    jobs: &JobStore,
    device_id: &str,
    allowed_action: &str,
) -> Result<(), AppError> {
    if jobs.list().into_iter().any(|job| {
        job.device_id == device_id
            && job.action != allowed_action
            && matches!(job.status, JobStatus::Pending | JobStatus::Running)
    }) {
        return Err(AppError::Conflict(format!(
            "device `{device_id}` has active jobs"
        )));
    }

    Ok(())
}

fn find_active_job(jobs: &JobStore, device_id: &str, action: &str) -> Option<Job> {
    jobs.list().into_iter().find(|job| {
        job.device_id == device_id
            && job.action == action
            && matches!(job.status, JobStatus::Pending | JobStatus::Running)
    })
}

fn is_tail_action(action: &str) -> bool {
    matches!(action, "serial.tail" | "ssh.tail" | "adb.logcat")
}

fn validate_device(device: &Device) -> Result<(), AppError> {
    if device.id.trim().is_empty() {
        return Err(AppError::BadRequest("device id is required".into()));
    }
    if device.id.trim() != device.id {
        return Err(AppError::BadRequest(
            "device id must not include leading or trailing whitespace".into(),
        ));
    }
    if device.id.contains('/') {
        return Err(AppError::BadRequest(
            "device id must not contain `/`".into(),
        ));
    }
    if device.label.trim().is_empty() {
        return Err(AppError::BadRequest("device label is required".into()));
    }
    if device.ssh.is_none()
        && device.serial.is_none()
        && device.jlink.is_none()
        && device.adb.is_none()
    {
        return Err(AppError::BadRequest(
            "device must configure at least one connection".into(),
        ));
    }

    if let Some(ssh) = &device.ssh {
        if ssh.host.trim().is_empty() || ssh.user.trim().is_empty() || ssh.key.trim().is_empty() {
            return Err(AppError::BadRequest(
                "ssh config requires host, user, and key".into(),
            ));
        }
        if ssh.default_log_path.trim().is_empty() {
            return Err(AppError::BadRequest(
                "ssh config requires default_log_path".into(),
            ));
        }
    }

    if let Some(serial) = &device.serial {
        if serial.port.trim().is_empty() {
            return Err(AppError::BadRequest("serial config requires a port".into()));
        }
        if serial.baud == 0 {
            return Err(AppError::BadRequest(
                "serial config requires baud > 0".into(),
            ));
        }
        if let Some(selector) = serial.selector.as_ref() {
            if selector.vid.is_none()
                && selector.pid.is_none()
                && selector.serial_number.is_none()
                && selector.physical_location.is_none()
            {
                return Err(AppError::BadRequest(
                    "serial selector requires at least one stable USB attribute".into(),
                ));
            }
            if selector
                .serial_number
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
                || selector
                    .physical_location
                    .as_deref()
                    .is_some_and(|value| value.trim().is_empty())
            {
                return Err(AppError::BadRequest(
                    "serial selector string attributes must not be empty".into(),
                ));
            }
        }
    }

    if let Some(jlink) = &device.jlink {
        if jlink.serial.trim().is_empty()
            || jlink.device.trim().is_empty()
            || jlink.interface.trim().is_empty()
        {
            return Err(AppError::BadRequest(
                "jlink config requires serial, device, and interface".into(),
            ));
        }
        if jlink.speed_khz == 0 {
            return Err(AppError::BadRequest(
                "jlink config requires speed_khz > 0".into(),
            ));
        }
    }

    if let Some(adb) = &device.adb {
        if adb.serial.trim().is_empty() {
            return Err(AppError::BadRequest("adb config requires serial".into()));
        }
        if adb
            .transport
            .as_deref()
            .is_some_and(|transport| transport.trim().is_empty())
        {
            return Err(AppError::BadRequest(
                "adb config transport must not be empty when set".into(),
            ));
        }
        if let Some(transport) = adb.transport.as_deref()
            && !matches!(transport, "serial" | "usb" | "emulator")
        {
            return Err(AppError::BadRequest(
                "adb config transport must be serial, usb, or emulator".into(),
            ));
        }
    }

    Ok(())
}

fn validate_action(
    action: &str,
    device: &Device,
    capabilities: &CapabilitySnapshot,
) -> Result<(), AppError> {
    let Some(capability) = capabilities.action(action) else {
        return Err(AppError::BadRequest(format!(
            "unsupported action `{action}`"
        )));
    };
    if !capability.available {
        return Err(AppError::CapabilityUnavailable(format!(
            "action `{action}` ({})",
            capability
                .unavailable_reason
                .as_deref()
                .unwrap_or("unavailable")
        )));
    }

    let supported = match action {
        "ssh.exec" | "ssh.tail" | "ssh.reboot" => device.ssh.is_some(),
        "serial.tail" | "serial.write" | "serial.exec" => device.serial.is_some(),
        "adb.logcat" => device.adb.is_some(),
        "jlink.probe" | "jlink.flash" | "jlink.verify" | "jlink.reset" | "jlink.erase" => {
            device.jlink.is_some()
        }
        _ => false,
    };

    if supported {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "device `{}` does not support action `{action}`",
            device.id
        )))
    }
}

fn ensure_adapter_available(
    adapter_id: &str,
    capabilities: &CapabilitySnapshot,
) -> Result<(), AppError> {
    let Some(capability) = capabilities.adapter(adapter_id) else {
        return Err(AppError::CapabilityUnavailable(format!(
            "adapter `{adapter_id}` (unknown)"
        )));
    };
    if capability.available {
        Ok(())
    } else {
        Err(AppError::CapabilityUnavailable(format!(
            "adapter `{adapter_id}` ({})",
            capability
                .unavailable_reason
                .as_deref()
                .unwrap_or("unavailable")
        )))
    }
}

fn validate_payload(
    action: &str,
    device: &Device,
    payload: Option<&ActionRequest>,
) -> Result<(), AppError> {
    let value = payload.and_then(|request| request.payload.as_ref());

    match action {
        "ssh.exec" if value.is_none_or(|command| command.trim().is_empty()) => Err(
            AppError::BadRequest("action `ssh.exec` requires a command payload".into()),
        ),
        "ssh.tail"
            if value.is_none_or(|path| path.trim().is_empty())
                && device
                    .ssh
                    .as_ref()
                    .is_none_or(|ssh| ssh.default_log_path.trim().is_empty()) =>
        {
            Err(AppError::BadRequest(
                "action `ssh.tail` requires a log path payload or device ssh.default_log_path"
                    .into(),
            ))
        }
        "serial.write" if value.is_none_or(|input| input.is_empty()) => Err(AppError::BadRequest(
            "action `serial.write` requires a payload".into(),
        )),
        "serial.exec" if serial_exec_request_from_action_payload(value).is_none() => Err(
            AppError::BadRequest("action `serial.exec` requires a command payload".into()),
        ),
        "jlink.flash" if value.is_none_or(|path| path.trim().is_empty()) => Err(
            AppError::BadRequest("action `jlink.flash` requires a firmware path payload".into()),
        ),
        _ => Ok(()),
    }
}

fn validate_serial_exec_request(request: &SerialExecRequest) -> Result<(), AppError> {
    if request.command.trim().is_empty() {
        return Err(AppError::BadRequest(
            "serial exec request requires a command".into(),
        ));
    }
    if request.timeout_secs == Some(0) {
        return Err(AppError::BadRequest(
            "serial exec request requires timeout_secs > 0".into(),
        ));
    }
    Ok(())
}

fn serial_exec_request_from_action_payload(value: Option<&String>) -> Option<SerialExecRequest> {
    let payload = value
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    if let Ok(request) = serde_json::from_str::<SerialExecRequest>(payload) {
        return validate_serial_exec_request(&request)
            .is_ok()
            .then_some(request);
    }
    Some(SerialExecRequest {
        command: payload.to_owned(),
        timeout_secs: None,
        until: None,
    })
}
