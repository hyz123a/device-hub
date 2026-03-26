use axum::{Router, routing::{get, post}};
use tower_http::cors::CorsLayer;

use crate::{
    api,
    config::AppConfig,
    events::EventBus,
    services::job_manager::JobManager,
    storage::{device_registry::DeviceRegistry, job_store::JobStore},
};

#[derive(Clone)]
pub struct AppState {
    pub devices: DeviceRegistry,
    pub jobs: JobStore,
    pub events: EventBus,
    pub job_manager: JobManager,
}

pub async fn build_router(config: AppConfig) -> Router {
    let events = EventBus::new(1_000);
    let devices = DeviceRegistry::new(config.devices);
    let jobs = JobStore::new();
    let job_manager = JobManager::new(jobs.clone(), events.clone());

    let state = AppState {
        devices,
        jobs,
        events,
        job_manager,
    };

    Router::new()
        .route("/", get(api::root))
        .route("/devices", get(api::list_devices))
        .route("/devices/{device_id}", get(api::get_device))
        .route("/devices/{device_id}/actions/{action}", post(api::start_action))
        .route("/jobs/{job_id}", get(api::get_job).delete(api::cancel_job))
        .route("/jobs/{job_id}/events", get(api::get_job_events))
        .route("/ws", get(api::ws_events))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
