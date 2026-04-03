use axum::{
    Router,
    routing::{get, post},
};
use device_hub_adapter_jlink::JlinkAdapter;
use device_hub_adapter_serial::SerialAdapter;
use device_hub_adapter_ssh::SshAdapter;
use std::{path::PathBuf, sync::Arc};
use tower_http::cors::CorsLayer;

use crate::{
    api, board_profiles,
    config::AppConfig,
    events::EventBus,
    frontend::{self, FrontendAssets},
    ports::{DynHostDiscovery, HostDiscoveryPort},
    services::{
        adapter_action_runner::AdapterActionRunner,
        adapter_host_discovery::AdapterHostDiscovery,
        job_manager::{JobManager, JobPolicyProvider},
    },
    storage::{
        artifact_store::ArtifactStore, device_registry::DeviceRegistry, job_store::JobStore,
    },
    use_cases::AppUseCases,
};

#[derive(Clone)]
pub struct AppState {
    pub use_cases: AppUseCases,
    pub events: EventBus,
    pub artifacts: ArtifactStore,
    pub frontend: FrontendAssets,
}

pub async fn build_router(config: AppConfig) -> Router {
    build_router_with_job_policy_and_board_profiles(
        config,
        JobPolicyProvider::default(),
        Vec::new(),
    )
    .await
}

pub async fn build_router_with_job_policy(
    config: AppConfig,
    policy_provider: JobPolicyProvider,
) -> Router {
    build_router_with_job_policy_and_board_profiles(config, policy_provider, Vec::new()).await
}

pub async fn build_router_with_job_policy_and_board_profiles(
    config: AppConfig,
    policy_provider: JobPolicyProvider,
    board_profiles: Vec<device_hub_shared::BoardProfile>,
) -> Router {
    build_router_with_components(
        config,
        policy_provider,
        SerialAdapter::new(),
        None,
        board_profiles,
    )
    .await
}

pub async fn build_router_with_job_policy_and_serial_stream_opener(
    config: AppConfig,
    policy_provider: JobPolicyProvider,
    max_message_bytes: usize,
    opener: impl Fn(
        &device_hub_shared::SerialConfig,
    ) -> Result<tokio_serial::SerialStream, std::io::Error>
    + Send
    + Sync
    + 'static,
) -> Router {
    let serial_adapter = SerialAdapter::with_stream_opener(max_message_bytes, move |config| {
        opener(config).map_err(device_hub_adapter_serial::SerialAdapterError::Io)
    });
    build_router_with_components(config, policy_provider, serial_adapter, None, Vec::new()).await
}

pub async fn build_router_with_job_policy_and_host_discovery(
    config: AppConfig,
    policy_provider: JobPolicyProvider,
    host_discovery: impl HostDiscoveryPort + 'static,
) -> Router {
    build_router_with_components(
        config,
        policy_provider,
        SerialAdapter::new(),
        Some(Arc::new(host_discovery)),
        Vec::new(),
    )
    .await
}

async fn build_router_with_components(
    config: AppConfig,
    policy_provider: JobPolicyProvider,
    serial_adapter: SerialAdapter,
    host_discovery: Option<DynHostDiscovery>,
    board_profiles: Vec<device_hub_shared::BoardProfile>,
) -> Router {
    let AppConfig {
        devices: configured_devices,
        device_registry_path,
        ssh_program,
        jlink_program,
        artifacts_dir,
    } = config;
    let artifacts = ArtifactStore::new(artifacts_dir.unwrap_or_else(|| ".artifacts".into()));
    let events = EventBus::new(1_000, Some(artifacts.clone()));
    let devices = Arc::new(
        DeviceRegistry::new(configured_devices, device_registry_path.map(PathBuf::from))
            .expect("failed to initialize device registry"),
    );
    let jobs = JobStore::new();
    let ssh_adapter = match ssh_program {
        Some(program) => SshAdapter::with_program(program),
        None => SshAdapter::new(),
    };
    let jlink_adapter = match jlink_program {
        Some(program) => JlinkAdapter::with_program(program),
        None => JlinkAdapter::new(),
    };
    let host_discovery = host_discovery.unwrap_or_else(|| {
        Arc::new(AdapterHostDiscovery::new(
            serial_adapter.clone(),
            jlink_adapter.clone(),
        ))
    });
    let action_runner = Arc::new(AdapterActionRunner::new(
        ssh_adapter,
        jlink_adapter,
        serial_adapter,
    ));
    let frontend = FrontendAssets::load_from_embedded_bundle()
        .expect("failed to load embedded frontend assets");
    let job_manager = JobManager::new_with_policy(
        jobs.clone(),
        events.clone(),
        artifacts.clone(),
        action_runner,
        policy_provider,
    );
    let use_cases = AppUseCases::new(
        board_profiles,
        devices.clone(),
        host_discovery,
        jobs.clone(),
        job_manager.clone(),
    );

    let state = AppState {
        use_cases,
        events,
        artifacts,
        frontend,
    };

    let api_router = Router::new()
        .route("/", get(api::root))
        .route("/board-profiles", get(api::list_board_profiles))
        .route("/devices", get(api::list_devices).post(api::create_device))
        .route("/host/serial-ports", get(api::list_serial_ports))
        .route("/host/jlink-probes", get(api::list_jlink_probes))
        .route(
            "/devices/{device_id}",
            get(api::get_device)
                .put(api::update_device)
                .delete(api::delete_device),
        )
        .route("/jobs", get(api::list_jobs))
        .route(
            "/devices/{device_id}/actions/{action}",
            post(api::start_action),
        )
        .route("/jobs/{job_id}", get(api::get_job).delete(api::cancel_job))
        .route("/jobs/{job_id}/evidence", get(api::get_job_evidence))
        .route("/jobs/{job_id}/events", get(api::get_job_events))
        .route("/jobs/{job_id}/rerun", post(api::rerun_job))
        .route(
            "/jobs/{job_id}/artifacts/{artifact_name}",
            get(api::download_job_artifact),
        )
        .route("/ws", get(api::ws_events))
        .fallback(frontend::api_not_found);

    Router::new()
        .nest("/api", api_router)
        .fallback(frontend::serve_embedded_asset)
        .layer(CorsLayer::permissive())
        .with_state(state)
}

pub fn load_default_board_profiles() -> Vec<device_hub_shared::BoardProfile> {
    board_profiles::load("board-profiles.yaml").unwrap_or_default()
}
