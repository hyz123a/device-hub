use axum::{
    Json,
    extract::{Path, Query, State, WebSocketUpgrade},
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;

use crate::{app::AppState, error::AppError};
use device_hub_shared::{ActionRequest, EventFilter, Job, LogEvent};

pub async fn root() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "name": "device-hub",
        "status": "ok"
    }))
}

pub async fn list_devices(State(state): State<AppState>) -> Json<Vec<device_hub_shared::Device>> {
    Json(state.devices.list())
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

pub async fn get_job_events(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<Vec<LogEvent>>, AppError> {
    if state.jobs.get(&job_id).is_none() {
        return Err(AppError::JobNotFound);
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
        _ => Ok(()),
    }
}
