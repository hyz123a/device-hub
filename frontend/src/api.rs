use device_hub_shared::{
    ActionRequest, BoardProfile, Device, DeviceDetail, DiscoveredJlinkProbe, DiscoveredSerialPort,
    Job, JobEvidence, LogEvent,
};
use gloo_net::http::{Request, Response};
use serde::Deserialize;

const API_PREFIX: &str = "/api";

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
}

pub async fn list_devices() -> Result<Vec<Device>, String> {
    let response = Request::get(&format!("{API_PREFIX}/devices"))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    decode_json(response).await
}

pub async fn get_device(device_id: &str) -> Result<DeviceDetail, String> {
    let response = Request::get(&format!("{API_PREFIX}/devices/{device_id}"))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    decode_json(response).await
}

pub async fn create_device(device: &Device) -> Result<Device, String> {
    let response = Request::post(&format!("{API_PREFIX}/devices"))
        .json(device)
        .map_err(|err| err.to_string())?
        .send()
        .await
        .map_err(|err| err.to_string())?;
    decode_json(response).await
}

pub async fn update_device(device_id: &str, device: &Device) -> Result<Device, String> {
    let response = Request::put(&format!("{API_PREFIX}/devices/{device_id}"))
        .json(device)
        .map_err(|err| err.to_string())?
        .send()
        .await
        .map_err(|err| err.to_string())?;
    decode_json(response).await
}

pub async fn delete_device(device_id: &str) -> Result<Device, String> {
    let response = Request::delete(&format!("{API_PREFIX}/devices/{device_id}"))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    decode_json(response).await
}

pub async fn list_serial_ports() -> Result<Vec<DiscoveredSerialPort>, String> {
    let response = Request::get(&format!("{API_PREFIX}/host/serial-ports"))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    decode_json(response).await
}

pub async fn list_board_profiles() -> Result<Vec<BoardProfile>, String> {
    let response = Request::get(&format!("{API_PREFIX}/board-profiles"))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    decode_json(response).await
}

pub async fn list_jlink_probes() -> Result<Vec<DiscoveredJlinkProbe>, String> {
    let response = Request::get(&format!("{API_PREFIX}/host/jlink-probes"))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    decode_json(response).await
}

pub async fn get_job_evidence(job_id: &str) -> Result<JobEvidence, String> {
    let response = Request::get(&format!("{API_PREFIX}/jobs/{job_id}/evidence"))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    decode_json(response).await
}

pub async fn list_jobs(device_id: Option<&str>) -> Result<Vec<Job>, String> {
    let query = device_id
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("?device_id={}", urlencoding::encode(value.trim())))
        .unwrap_or_default();
    let response = Request::get(&format!("{API_PREFIX}/jobs{query}"))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    decode_json(response).await
}

pub async fn get_job_events(job_id: &str) -> Result<Vec<LogEvent>, String> {
    let response = Request::get(&format!("{API_PREFIX}/jobs/{job_id}/events"))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    decode_json(response).await
}

pub async fn start_action(
    device_id: &str,
    action: &str,
    payload: Option<String>,
) -> Result<Job, String> {
    let response = Request::post(&format!(
        "{API_PREFIX}/devices/{device_id}/actions/{action}"
    ))
    .json(&Some(ActionRequest { payload }))
    .map_err(|err| err.to_string())?
    .send()
    .await
    .map_err(|err| err.to_string())?;
    decode_json(response).await
}

pub async fn cancel_job(job_id: &str) -> Result<Job, String> {
    let response = Request::delete(&format!("{API_PREFIX}/jobs/{job_id}"))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    decode_json(response).await
}

pub async fn rerun_job(job_id: &str) -> Result<Job, String> {
    let response = Request::post(&format!("{API_PREFIX}/jobs/{job_id}/rerun"))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    decode_json(response).await
}

pub fn artifact_url(download_url: &str) -> String {
    download_url.to_owned()
}

async fn decode_json<T>(response: Response) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    if response.ok() {
        response.json().await.map_err(|err| err.to_string())
    } else {
        let status = response.status();
        let body = response.text().await.map_err(|err| err.to_string())?;
        match serde_json::from_str::<ErrorBody>(&body) {
            Ok(error) => Err(error.error),
            Err(_) => Err(format!("request failed with status {status}: {body}")),
        }
    }
}

pub fn ws_url(device_id: Option<&str>, job_id: Option<&str>) -> Result<String, String> {
    let window = web_sys::window().ok_or_else(|| "window is not available".to_owned())?;
    let location = window.location();
    let protocol = location
        .protocol()
        .map_err(|err| format!("protocol lookup failed: {err:?}"))?;
    let host = location
        .host()
        .map_err(|err| format!("host lookup failed: {err:?}"))?;
    let scheme = if protocol == "https:" { "wss" } else { "ws" };

    let mut params = Vec::new();
    if let Some(device_id) = device_id.filter(|value| !value.trim().is_empty()) {
        params.push(format!(
            "device_id={}",
            urlencoding::encode(device_id.trim())
        ));
    }
    if let Some(job_id) = job_id.filter(|value| !value.trim().is_empty()) {
        params.push(format!("job_id={}", urlencoding::encode(job_id.trim())));
    }

    let query = if params.is_empty() {
        String::new()
    } else {
        format!("?{}", params.join("&"))
    };

    Ok(format!("{scheme}://{host}{API_PREFIX}/ws{query}"))
}
