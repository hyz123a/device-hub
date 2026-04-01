use device_hub_shared::{ActionRequest, Device, Job};

use crate::{
    error::AppError,
    services::job_manager::JobManager,
    storage::{device_registry::DeviceRegistry, job_store::JobStore},
};

#[derive(Clone)]
pub struct AppUseCases {
    devices: DeviceRegistry,
    jobs: JobStore,
    job_manager: JobManager,
}

impl AppUseCases {
    pub fn new(devices: DeviceRegistry, jobs: JobStore, job_manager: JobManager) -> Self {
        Self {
            devices,
            jobs,
            job_manager,
        }
    }

    pub fn list_devices(&self) -> Vec<Device> {
        self.devices.list()
    }

    pub fn get_device(&self, device_id: &str) -> Result<Device, AppError> {
        self.devices.get(device_id).ok_or(AppError::DeviceNotFound)
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
    ) -> Result<Job, AppError> {
        let device = self.get_device(device_id)?;
        validate_action(&action, &device)?;
        validate_payload(&action, payload.as_ref())?;
        self.job_manager.start(device, action, payload).await
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
}

fn validate_action(action: &str, device: &Device) -> Result<(), AppError> {
    let supported = match action {
        "ssh.exec" | "ssh.tail" | "ssh.reboot" => device.ssh.is_some(),
        "serial.tail" | "serial.write" => device.serial.is_some(),
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

fn validate_payload(action: &str, payload: Option<&ActionRequest>) -> Result<(), AppError> {
    let value = payload.and_then(|request| request.payload.as_ref());

    match action {
        "ssh.exec" if value.is_none_or(|command| command.trim().is_empty()) => Err(
            AppError::BadRequest("action `ssh.exec` requires a command payload".into()),
        ),
        "ssh.tail" if value.is_none_or(|path| path.trim().is_empty()) => Err(AppError::BadRequest(
            "action `ssh.tail` requires a log path payload".into(),
        )),
        "serial.write" if value.is_none_or(|input| input.is_empty()) => Err(AppError::BadRequest(
            "action `serial.write` requires a payload".into(),
        )),
        "jlink.flash" if value.is_none_or(|path| path.trim().is_empty()) => Err(
            AppError::BadRequest("action `jlink.flash` requires a firmware path payload".into()),
        ),
        _ => Ok(()),
    }
}
