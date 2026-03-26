use device_hub_shared::{Device, DeviceDetail, Job, LogEvent};
use gloo_net::http::Request;

pub async fn list_devices() -> Result<Vec<Device>, String> {
    Request::get("/devices")
        .send()
        .await
        .map_err(|err| err.to_string())?
        .json()
        .await
        .map_err(|err| err.to_string())
}

pub async fn get_device(device_id: &str) -> Result<DeviceDetail, String> {
    Request::get(&format!("/devices/{device_id}"))
        .send()
        .await
        .map_err(|err| err.to_string())?
        .json()
        .await
        .map_err(|err| err.to_string())
}

pub async fn get_job(job_id: &str) -> Result<Job, String> {
    Request::get(&format!("/jobs/{job_id}"))
        .send()
        .await
        .map_err(|err| err.to_string())?
        .json()
        .await
        .map_err(|err| err.to_string())
}

pub async fn get_job_events(job_id: &str) -> Result<Vec<LogEvent>, String> {
    Request::get(&format!("/jobs/{job_id}/events"))
        .send()
        .await
        .map_err(|err| err.to_string())?
        .json()
        .await
        .map_err(|err| err.to_string())
}
