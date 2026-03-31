use axum::{
    Json,
    extract::{Path, Query, State, WebSocketUpgrade},
    http::{HeaderValue, StatusCode, header},
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;

use crate::{app::AppState, error::AppError};
use device_hub_shared::{
    ActionRequest, EventFilter, Job, JobEvidence, JobStatus, JobSummary, LogChannel, LogEvent,
    LogLevel,
};

pub async fn root() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "name": "device-hub",
        "status": "ok"
    }))
}

pub async fn list_devices(State(state): State<AppState>) -> Json<Vec<device_hub_shared::Device>> {
    Json(state.devices.list())
}

#[derive(Debug, Deserialize, Default)]
pub struct ListJobsQuery {
    pub device_id: Option<String>,
}

pub async fn list_jobs(
    State(state): State<AppState>,
    Query(query): Query<ListJobsQuery>,
) -> Json<Vec<Job>> {
    let mut jobs = state.jobs.list();
    if let Some(device_id) = query.device_id {
        jobs.retain(|job| job.device_id == device_id);
    }
    Json(jobs)
}

pub async fn get_device(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
) -> Result<Json<device_hub_shared::DeviceDetail>, AppError> {
    let device = state
        .devices
        .get(&device_id)
        .ok_or(AppError::DeviceNotFound)?;
    Ok(Json(device.detail()))
}

pub async fn start_action(
    State(state): State<AppState>,
    Path((device_id, action)): Path<(String, String)>,
    Json(payload): Json<Option<ActionRequest>>,
) -> Result<Json<Job>, AppError> {
    let device = state
        .devices
        .get(&device_id)
        .ok_or(AppError::DeviceNotFound)?;
    validate_action(&action, &device)?;
    validate_payload(&action, payload.as_ref())?;
    let job = state.job_manager.start(device, action, payload).await?;
    Ok(Json(job))
}

pub async fn get_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<Job>, AppError> {
    let job = state.jobs.get(&job_id).ok_or(AppError::JobNotFound)?;
    Ok(Json(job))
}

pub async fn get_job_evidence(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<JobEvidence>, AppError> {
    let job = state.jobs.get(&job_id).ok_or(AppError::JobNotFound)?;
    let events = state
        .artifacts
        .read_events(&job_id)
        .map_err(|error| AppError::Internal(error.to_string()))?
        .unwrap_or_else(|| {
            state.events.snapshot(&EventFilter {
                job_id: Some(job_id.clone()),
                ..EventFilter::default()
            })
        });
    let artifacts = state
        .artifacts
        .list_job_artifacts(&job_id)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let device = state.devices.get(&job.device_id);

    Ok(Json(JobEvidence {
        summary: build_job_summary(&job, device.as_ref(), &events, &artifacts),
        job,
        artifacts,
    }))
}

pub async fn get_job_events(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<Vec<LogEvent>>, AppError> {
    if state.jobs.get(&job_id).is_none() {
        return Err(AppError::JobNotFound);
    }

    if let Some(events) = state
        .artifacts
        .read_events(&job_id)
        .map_err(|error| AppError::Internal(error.to_string()))?
    {
        return Ok(Json(events));
    }

    let filter = EventFilter {
        job_id: Some(job_id),
        ..EventFilter::default()
    };
    Ok(Json(state.events.snapshot(&filter)))
}

pub async fn cancel_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<Job>, AppError> {
    let job = state.job_manager.cancel(&job_id).await?;
    Ok(Json(job))
}

pub async fn rerun_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<Job>, AppError> {
    let previous = state.jobs.get(&job_id).ok_or(AppError::JobNotFound)?;
    let device = state
        .devices
        .get(&previous.device_id)
        .ok_or(AppError::DeviceNotFound)?;
    let payload = previous.payload.clone().map(|payload| ActionRequest {
        payload: Some(payload),
    });
    let job = state
        .job_manager
        .start(device, previous.action.clone(), payload)
        .await?;
    Ok(Json(job))
}

pub async fn download_job_artifact(
    State(state): State<AppState>,
    Path((job_id, artifact_name)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    if state.jobs.get(&job_id).is_none() {
        return Err(AppError::JobNotFound);
    }

    let path = state
        .artifacts
        .artifact_path(&job_id, &artifact_name)
        .ok_or(AppError::JobNotFound)?;
    let body = tokio::fs::read(&path)
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, artifact_content_type(&artifact_name)),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!("attachment; filename=\"{artifact_name}\""))
                    .map_err(|error| AppError::Internal(error.to_string()))?,
            ),
        ],
        body,
    ))
}

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub device_id: Option<String>,
    pub job_id: Option<String>,
    pub since: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn ws_events(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        let filter = EventFilter {
            device_id: query.device_id,
            job_id: query.job_id,
            since: query.since,
        };
        handle_socket(socket, state, filter).await;
    })
}

async fn handle_socket(socket: axum::extract::ws::WebSocket, state: AppState, filter: EventFilter) {
    let (mut sender, mut receiver) = socket.split();
    let mut subscription = state.events.subscribe();

    for event in state.events.snapshot(&filter) {
        if sender
            .send(axum::extract::ws::Message::Text(
                serde_json::to_string(&event).unwrap_or_default().into(),
            ))
            .await
            .is_err()
        {
            return;
        }
    }

    loop {
        tokio::select! {
            maybe_event = subscription.recv() => {
                match maybe_event {
                    Ok(event) if filter.matches(&event) => {
                        if sender.send(axum::extract::ws::Message::Text(
                            serde_json::to_string(&event).unwrap_or_default().into(),
                        )).await.is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            message = receiver.next() => {
                match message {
                    Some(Ok(axum::extract::ws::Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }
}

fn validate_action(action: &str, device: &device_hub_shared::Device) -> Result<(), AppError> {
    let supported = match action {
        "ssh.exec" | "ssh.tail" | "ssh.reboot" => device.ssh.is_some(),
        "serial.tail" => device.serial.is_some(),
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
        "jlink.flash" if value.is_none_or(|path| path.trim().is_empty()) => Err(
            AppError::BadRequest("action `jlink.flash` requires a firmware path payload".into()),
        ),
        _ => Ok(()),
    }
}

fn artifact_content_type(name: &str) -> HeaderValue {
    match name.rsplit('.').next() {
        Some("json") => HeaderValue::from_static("application/json"),
        Some("ndjson") => HeaderValue::from_static("application/x-ndjson"),
        _ => HeaderValue::from_static("application/octet-stream"),
    }
}

fn build_job_summary(
    job: &Job,
    device: Option<&device_hub_shared::Device>,
    events: &[LogEvent],
    artifacts: &[device_hub_shared::JobArtifact],
) -> JobSummary {
    let stdout_lines = events
        .iter()
        .filter(|event| matches!(event.channel, LogChannel::Stdout))
        .count();
    let stderr_lines = events
        .iter()
        .filter(|event| matches!(event.channel, LogChannel::Stderr))
        .count();
    let system_events = events
        .iter()
        .filter(|event| matches!(event.channel, LogChannel::System))
        .count();
    let outcome = events.iter().rev().find_map(|event| {
        (matches!(event.level, LogLevel::Error)
            || event.source == device_hub_shared::LogSource::Job)
            .then(|| event.message.clone())
    });

    let request = match job.action.as_str() {
        "ssh.exec" => job
            .payload
            .as_ref()
            .map(|payload| format!("command: {payload}")),
        "ssh.tail" => job
            .payload
            .as_ref()
            .map(|payload| format!("path: {payload}")),
        "serial.tail" => device.and_then(|device| {
            device
                .serial
                .as_ref()
                .map(|serial| format!("serial: {} @ {} baud", serial.port, serial.baud))
        }),
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
    if matches!(job.status, JobStatus::Failed | JobStatus::Cancelled) {
        if let Some(last_error) = events
            .iter()
            .rev()
            .find(|event| matches!(event.level, LogLevel::Error))
        {
            highlights.push(format!("last error: {}", last_error.message));
        }
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
        outcome,
        highlights,
    }
}
