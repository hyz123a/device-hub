use device_hub_shared::{
    ActionRequest, ActionStartResponse, ActiveTailStatus, BoardProfile, Device, DeviceDetail,
    DeviceRuntimeStatus, DiscoveredJlinkProbe, DiscoveredSerialPort, Job, JobStatus,
    SerialExecRequest, SerialExecResult,
};
use std::sync::Arc;

use crate::{
    error::AppError,
    events::EventBus,
    ports::{
        DeviceRegistryError, DynDeviceRegistry, DynHostDiscovery, DynSerialCommandExecutor,
        HostDiscoveryError, SerialCommandError,
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
    serial_command_executor: DynSerialCommandExecutor,
}

impl AppUseCases {
    pub fn new(
        board_profiles: Vec<BoardProfile>,
        devices: DynDeviceRegistry,
        host_discovery: DynHostDiscovery,
        jobs: JobStore,
        job_manager: JobManager,
        events: EventBus,
        serial_command_executor: DynSerialCommandExecutor,
    ) -> Self {
        Self {
            board_profiles: Arc::new(board_profiles),
            devices,
            host_discovery,
            jobs,
            job_manager,
            events,
            serial_command_executor,
        }
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
        let mut ports = self
            .host_discovery
            .list_serial_ports()
            .await
            .map_err(map_host_discovery_error)?;
        ports.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(ports)
    }

    pub async fn list_jlink_probes(&self) -> Result<Vec<DiscoveredJlinkProbe>, AppError> {
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
        validate_action("serial.exec", &device)?;
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

    pub async fn start_action(
        &self,
        device_id: &str,
        action: String,
        payload: Option<ActionRequest>,
        reuse: bool,
    ) -> Result<ActionStartResponse, AppError> {
        let device = self.get_device(device_id)?;
        validate_action(&action, &device)?;
        validate_payload(&action, &device, payload.as_ref())?;
        if reuse && matches!(action.as_str(), "serial.tail" | "adb.logcat") {
            if let Some(job) = find_active_job(&self.jobs, device_id, &action) {
                return Ok(ActionStartResponse {
                    job,
                    already_running: true,
                });
            }
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
                job_id: job.id,
                active_since: job.created_at,
            })
            .collect();

        DeviceRuntimeStatus { active_tails }
    }
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

fn validate_action(action: &str, device: &Device) -> Result<(), AppError> {
    let supported = match action {
        "ssh.exec" | "ssh.tail" | "ssh.reboot" => device.ssh.is_some(),
        "serial.tail" | "serial.write" | "serial.exec" => device.serial.is_some(),
        "adb.logcat" => device.adb.is_some(),
        "jlink.probe" | "jlink.flash" | "jlink.verify" | "jlink.reset" | "jlink.erase" => {
            device.jlink.is_some()
        }
        _ => {
            return Err(AppError::BadRequest(format!(
                "unsupported action `{action}`"
            )));
        }
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
