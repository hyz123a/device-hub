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
    ActionRequest, BoardProfile, DiscoveredJlinkProbe, DiscoveredSerialPort, EventFilter, Job,
    JobEvidence, JobStatus, JobSummary, LogChannel, LogEvent, LogLevel, LogSource,
};

pub async fn root() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "name": "device-hub",
        "status": "ok"
    }))
}

pub async fn list_devices(State(state): State<AppState>) -> Json<Vec<device_hub_shared::Device>> {
    Json(state.use_cases.list_devices())
}

pub async fn create_device(
    State(state): State<AppState>,
    Json(device): Json<device_hub_shared::Device>,
) -> Result<Json<device_hub_shared::Device>, AppError> {
    let device = state.use_cases.create_device(device)?;
    Ok(Json(device))
}

pub async fn list_board_profiles(
    State(state): State<AppState>,
) -> Result<Json<Vec<BoardProfile>>, AppError> {
    Ok(Json(state.use_cases.list_board_profiles()))
}

pub async fn list_serial_ports(
    State(state): State<AppState>,
) -> Result<Json<Vec<DiscoveredSerialPort>>, AppError> {
    Ok(Json(state.use_cases.list_serial_ports().await?))
}

pub async fn list_jlink_probes(
    State(state): State<AppState>,
) -> Result<Json<Vec<DiscoveredJlinkProbe>>, AppError> {
    Ok(Json(state.use_cases.list_jlink_probes().await?))
}

#[derive(Debug, Deserialize, Default)]
pub struct ListJobsQuery {
    pub device_id: Option<String>,
}

pub async fn list_jobs(
    State(state): State<AppState>,
    Query(query): Query<ListJobsQuery>,
) -> Json<Vec<Job>> {
    Json(state.use_cases.list_jobs(query.device_id.as_deref()))
}

pub async fn get_device(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
) -> Result<Json<device_hub_shared::DeviceDetail>, AppError> {
    let device = state.use_cases.get_device(&device_id)?;
    Ok(Json(device.detail()))
}

pub async fn update_device(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    Json(device): Json<device_hub_shared::Device>,
) -> Result<Json<device_hub_shared::Device>, AppError> {
    let device = state.use_cases.update_device(&device_id, device)?;
    Ok(Json(device))
}

pub async fn delete_device(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
) -> Result<Json<device_hub_shared::Device>, AppError> {
    let device = state.use_cases.delete_device(&device_id)?;
    Ok(Json(device))
}

pub async fn start_action(
    State(state): State<AppState>,
    Path((device_id, action)): Path<(String, String)>,
    Json(payload): Json<Option<ActionRequest>>,
) -> Result<Json<Job>, AppError> {
    let job = state
        .use_cases
        .start_action(&device_id, action, payload)
        .await?;
    Ok(Json(job))
}

pub async fn get_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<Job>, AppError> {
    let job = state.use_cases.get_job(&job_id)?;
    Ok(Json(job))
}

pub async fn get_job_evidence(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<JobEvidence>, AppError> {
    let job = state.use_cases.get_job(&job_id)?;
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
    let device = state.use_cases.get_device(&job.device_id).ok();

    Ok(Json(JobEvidence {
        summary: build_job_summary(&job, device.as_ref(), &events, &artifacts),
        job,
        artifacts,
    }))
}

pub async fn get_job_events(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    Query(query): Query<JobEventsQuery>,
) -> Result<Json<Vec<LogEvent>>, AppError> {
    state.use_cases.get_job(&job_id)?;

    if let Some(events) = state
        .artifacts
        .read_events(&job_id)
        .map_err(|error| AppError::Internal(error.to_string()))?
    {
        return Ok(Json(filter_job_events(events, &query)));
    }

    let filter = EventFilter {
        job_id: Some(job_id),
        ..EventFilter::default()
    };
    Ok(Json(filter_job_events(
        state.events.snapshot(&filter),
        &query,
    )))
}

#[derive(Debug, Deserialize, Default)]
pub struct JobEventsQuery {
    pub source: Option<LogSource>,
    pub level: Option<LogLevel>,
    pub search: Option<String>,
}

pub async fn cancel_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<Job>, AppError> {
    let job = state.use_cases.cancel_job(&job_id).await?;
    Ok(Json(job))
}

pub async fn rerun_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<Job>, AppError> {
    let job = state.use_cases.rerun_job(&job_id).await?;
    Ok(Json(job))
}

pub async fn download_job_artifact(
    State(state): State<AppState>,
    Path((job_id, artifact_name)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    state.use_cases.get_job(&job_id)?;

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

fn artifact_content_type(name: &str) -> HeaderValue {
    match name.rsplit('.').next() {
        Some("json") => HeaderValue::from_static("application/json"),
        Some("ndjson") => HeaderValue::from_static("application/x-ndjson"),
        _ => HeaderValue::from_static("application/octet-stream"),
    }
}

fn filter_job_events(events: Vec<LogEvent>, query: &JobEventsQuery) -> Vec<LogEvent> {
    let search = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);

    events
        .into_iter()
        .filter(|event| {
            query
                .source
                .as_ref()
                .is_none_or(|source| &event.source == source)
        })
        .filter(|event| {
            query
                .level
                .as_ref()
                .is_none_or(|level| &event.level == level)
        })
        .filter(|event| {
            search
                .as_deref()
                .is_none_or(|value| event_matches_search_query(event, value))
        })
        .collect()
}

fn event_matches_search_query(event: &LogEvent, query: &str) -> bool {
    [
        event.message.as_str(),
        event.device_id.as_str(),
        event.job_id.as_str(),
        log_source_label(&event.source),
        log_channel_label(&event.channel),
        log_level_label(&event.level),
    ]
    .into_iter()
    .any(|value| value.to_ascii_lowercase().contains(query))
}

fn log_source_label(source: &LogSource) -> &'static str {
    match source {
        LogSource::Ssh => "ssh",
        LogSource::Serial => "serial",
        LogSource::Jlink => "jlink",
        LogSource::System => "system",
        LogSource::Job => "job",
    }
}

fn log_channel_label(channel: &LogChannel) -> &'static str {
    match channel {
        LogChannel::Stdout => "stdout",
        LogChannel::Stderr => "stderr",
        LogChannel::System => "system",
    }
}

fn log_level_label(level: &LogLevel) -> &'static str {
    match level {
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
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
        "serial.write" => job
            .payload
            .as_ref()
            .map(|payload| format!("input: {}", payload.escape_debug())),
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
