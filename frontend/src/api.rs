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

    Ok(format!("{scheme}://{host}/ws{query}"))
}
