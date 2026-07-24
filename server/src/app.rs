use axum::{
    Router,
    routing::{get, post},
};
#[cfg(feature = "adb")]
use device_hub_adapter_adb::AdbAdapter;
#[cfg(feature = "jlink")]
use device_hub_adapter_jlink::JlinkAdapter;
#[cfg(feature = "serial")]
use device_hub_adapter_serial::SerialAdapter;
#[cfg(feature = "ssh")]
use device_hub_adapter_ssh::SshAdapter;
use futures::future::BoxFuture;
use std::{path::PathBuf, sync::Arc};
use tower_http::cors::CorsLayer;

#[cfg(feature = "adb")]
use crate::services::adapter_action_runner::AdbActionHandler;
#[cfg(feature = "jlink")]
use crate::services::adapter_action_runner::JlinkActionHandler;
#[cfg(feature = "ssh")]
use crate::services::adapter_action_runner::SshActionHandler;
#[cfg(feature = "serial")]
use crate::services::{
    adapter_action_runner::{SerialActionHandler, SerialReconnectPolicy},
    adapter_serial_command_executor::AdapterSerialCommandExecutor,
};
use crate::{
    api, board_profiles,
    config::{AdapterEnablement, AppConfig},
    events::EventBus,
    frontend::{self, FrontendAssets},
    ports::{
        ActionRunner, CapabilityProviderPort, DynActionRunner, DynCapabilityProvider,
        DynEventPersistence, DynHostDiscovery, DynJobEvidenceStore, DynSerialCommandExecutor,
        HostDiscoveryPort, JobEvidenceStorePort, SerialCommandError, SerialCommandExecutorPort,
    },
    services::{
        adapter_action_runner::CapabilityRegistry,
        adapter_host_discovery::AdapterHostDiscovery,
        job_manager::{JobManager, JobPolicyProvider},
    },
    storage::{
        artifact_store::ArtifactStore, buffered_event_writer::BufferedEventWriter,
        device_registry::DeviceRegistry, job_store::JobStore,
    },
    use_cases::{AppUseCaseDependencies, AppUseCases},
};
use device_hub_shared::{
    AdapterCapability, CapabilitySnapshot, Device, SerialExecRequest, SerialExecResult,
};

#[derive(Clone)]
pub struct AppState {
    pub use_cases: AppUseCases,
    pub events: EventBus,
    pub frontend: FrontendAssets,
}

#[derive(Clone)]
struct UnavailableSerialCommandExecutor;

impl SerialCommandExecutorPort for UnavailableSerialCommandExecutor {
    fn exec(
        &self,
        _device: Device,
        _request: SerialExecRequest,
    ) -> BoxFuture<'static, Result<SerialExecResult, SerialCommandError>> {
        Box::pin(async {
            Err(SerialCommandError::Failed(
                "serial capability is unavailable".into(),
            ))
        })
    }
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
        BuildOverrides::default(),
        None,
        board_profiles,
        None,
    )
    .await
}

#[cfg(feature = "serial")]
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
    build_router_with_components(
        config,
        policy_provider,
        BuildOverrides {
            serial_adapter: Some(serial_adapter),
            ..Default::default()
        },
        None,
        Vec::new(),
        None,
    )
    .await
}

pub async fn build_router_with_job_policy_and_host_discovery(
    config: AppConfig,
    policy_provider: JobPolicyProvider,
    host_discovery: impl HostDiscoveryPort + 'static,
) -> Router {
    build_router_with_components(
        config,
        policy_provider,
        BuildOverrides::default(),
        Some(Arc::new(host_discovery)),
        Vec::new(),
        None,
    )
    .await
}

pub async fn build_router_with_job_policy_and_evidence_store(
    config: AppConfig,
    policy_provider: JobPolicyProvider,
    evidence_store: impl JobEvidenceStorePort + 'static,
) -> Router {
    build_router_with_components(
        config,
        policy_provider,
        BuildOverrides::default(),
        None,
        Vec::new(),
        Some(Arc::new(evidence_store)),
    )
    .await
}

pub async fn build_router_with_job_policy_and_action_runtime<
    R: ActionRunner + CapabilityProviderPort + 'static,
>(
    config: AppConfig,
    policy_provider: JobPolicyProvider,
    action_runtime: R,
) -> Router {
    let action_runtime = Arc::new(action_runtime);
    let action_runner: DynActionRunner = action_runtime.clone();
    let capability_provider: DynCapabilityProvider = action_runtime;
    build_router_with_components(
        config,
        policy_provider,
        BuildOverrides {
            action_runner: Some(action_runner),
            capability_provider: Some(capability_provider),
            ..Default::default()
        },
        None,
        Vec::new(),
        None,
    )
    .await
}

#[derive(Default)]
struct BuildOverrides {
    #[cfg(feature = "serial")]
    serial_adapter: Option<SerialAdapter>,
    action_runner: Option<DynActionRunner>,
    capability_provider: Option<DynCapabilityProvider>,
}

async fn build_router_with_components(
    config: AppConfig,
    policy_provider: JobPolicyProvider,
    mut overrides: BuildOverrides,
    host_discovery: Option<DynHostDiscovery>,
    board_profiles: Vec<device_hub_shared::BoardProfile>,
    evidence_store: Option<DynJobEvidenceStore>,
) -> Router {
    let AppConfig {
        devices: configured_devices,
        device_registry_path,
        ssh_program,
        jlink_program,
        adb_program,
        artifacts_dir,
        adapters,
    } = config;
    let artifacts_root = PathBuf::from(artifacts_dir.unwrap_or_else(|| ".artifacts".into()));
    let artifacts = ArtifactStore::new(&artifacts_root);
    let event_persistence: DynEventPersistence = Arc::new(
        BufferedEventWriter::start(&artifacts_root, 4_096)
            .expect("failed to start buffered artifact writer"),
    );
    let events = EventBus::new(5_000, Some(event_persistence));
    let devices = Arc::new(
        DeviceRegistry::new(configured_devices, device_registry_path.map(PathBuf::from))
            .expect("failed to initialize device registry"),
    );
    let jobs = JobStore::new();
    let mut registry = CapabilityRegistry::new(capability_definitions(&adapters));
    let mut adapter_host_discovery = AdapterHostDiscovery::new();
    let mut serial_command_executor: DynSerialCommandExecutor =
        Arc::new(UnavailableSerialCommandExecutor);

    #[cfg(feature = "serial")]
    if adapters.serial {
        let serial_adapter = overrides.serial_adapter.take().unwrap_or_default();
        adapter_host_discovery = adapter_host_discovery.with_serial(serial_adapter.clone());
        serial_command_executor =
            Arc::new(AdapterSerialCommandExecutor::new(serial_adapter.clone()));
        let initial_delay_ms = adapters.serial_reconnect.initial_delay_ms.clamp(10, 60_000);
        let max_delay_ms = adapters
            .serial_reconnect
            .max_delay_ms
            .clamp(initial_delay_ms, 60_000);
        registry
            .register(
                "serial",
                &["serial.tail", "serial.write", "serial.exec"],
                SerialActionHandler::with_reconnect_policy(
                    serial_adapter,
                    serial_command_executor.clone(),
                    SerialReconnectPolicy {
                        max_attempts: adapters.serial_reconnect.max_attempts,
                        initial_delay: std::time::Duration::from_millis(initial_delay_ms),
                        max_delay: std::time::Duration::from_millis(max_delay_ms),
                        prevent_system_sleep: adapters.serial_reconnect.prevent_system_sleep,
                    },
                ),
            )
            .expect("serial capability registration must be valid");
    }

    #[cfg(feature = "ssh")]
    if adapters.ssh {
        let ssh_adapter = match ssh_program {
            Some(program) => SshAdapter::with_program(program),
            None => SshAdapter::new(),
        };
        registry
            .register(
                "ssh",
                &["ssh.exec", "ssh.tail", "ssh.reboot"],
                SshActionHandler::new(ssh_adapter),
            )
            .expect("ssh capability registration must be valid");
    }

    #[cfg(feature = "jlink")]
    if adapters.jlink {
        let jlink_adapter = match jlink_program {
            Some(program) => JlinkAdapter::with_program(program),
            None => JlinkAdapter::new(),
        };
        adapter_host_discovery = adapter_host_discovery.with_jlink(jlink_adapter.clone());
        registry
            .register(
                "jlink",
                &[
                    "jlink.probe",
                    "jlink.flash",
                    "jlink.verify",
                    "jlink.reset",
                    "jlink.erase",
                ],
                JlinkActionHandler::new(jlink_adapter),
            )
            .expect("jlink capability registration must be valid");
    }

    #[cfg(feature = "adb")]
    if adapters.adb {
        let adb_adapter = match adb_program {
            Some(program) => AdbAdapter::with_program(program),
            None => AdbAdapter::new(),
        };
        registry
            .register("adb", &["adb.logcat"], AdbActionHandler::new(adb_adapter))
            .expect("adb capability registration must be valid");
    }

    #[cfg(not(feature = "ssh"))]
    let _ = ssh_program;
    #[cfg(not(feature = "jlink"))]
    let _ = jlink_program;
    #[cfg(not(feature = "adb"))]
    let _ = adb_program;
    #[cfg(not(feature = "serial"))]
    let _ = &mut overrides;

    let host_discovery = host_discovery.unwrap_or_else(|| Arc::new(adapter_host_discovery));
    let capability_provider: DynCapabilityProvider = overrides
        .capability_provider
        .take()
        .unwrap_or_else(|| Arc::new(registry.clone()));
    eprintln!(
        "device-hub capabilities: {}",
        format_capability_snapshot(&capability_provider.snapshot())
    );
    let action_runner: DynActionRunner = overrides
        .action_runner
        .take()
        .unwrap_or_else(|| Arc::new(registry));
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
        AppUseCaseDependencies {
            devices: devices.clone(),
            host_discovery,
            jobs: jobs.clone(),
            job_manager: job_manager.clone(),
            events: events.clone(),
            evidence_store: evidence_store.unwrap_or_else(|| Arc::new(artifacts.clone())),
            serial_command_executor,
            capabilities: capability_provider,
        },
    );

    let state = AppState {
        use_cases,
        events,
        frontend,
    };

    let api_router = Router::new()
        .route("/", get(api::root))
        .route("/capabilities", get(api::list_capabilities))
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
            "/events",
            get(api::get_events).delete(api::clear_event_cache),
        )
        .route(
            "/devices/{device_id}/actions/{action}",
            post(api::start_action),
        )
        .route("/devices/{device_id}/serial/exec", post(api::exec_serial))
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

fn capability_definitions(enablement: &AdapterEnablement) -> Vec<AdapterCapability> {
    vec![
        capability(
            "serial",
            cfg!(feature = "serial"),
            enablement.serial,
            &["serial.tail", "serial.write", "serial.exec"],
        ),
        capability(
            "ssh",
            cfg!(feature = "ssh"),
            enablement.ssh,
            &["ssh.exec", "ssh.tail", "ssh.reboot"],
        ),
        capability(
            "jlink",
            cfg!(feature = "jlink"),
            enablement.jlink,
            &[
                "jlink.probe",
                "jlink.flash",
                "jlink.verify",
                "jlink.reset",
                "jlink.erase",
            ],
        ),
        capability(
            "adb",
            cfg!(feature = "adb"),
            enablement.adb,
            &["adb.logcat"],
        ),
    ]
}

fn capability(
    adapter_id: &str,
    compiled: bool,
    enabled: bool,
    actions: &[&str],
) -> AdapterCapability {
    let enabled = compiled && enabled;
    AdapterCapability {
        adapter_id: adapter_id.to_owned(),
        compiled,
        enabled,
        available: false,
        actions: actions.iter().map(|action| (*action).to_owned()).collect(),
        unavailable_reason: Some(if !compiled {
            "not_compiled".into()
        } else if !enabled {
            "disabled".into()
        } else {
            "not_registered".into()
        }),
    }
}

fn format_capability_snapshot(snapshot: &CapabilitySnapshot) -> String {
    snapshot
        .adapters
        .iter()
        .map(|capability| {
            let state = if capability.available {
                "available"
            } else {
                capability
                    .unavailable_reason
                    .as_deref()
                    .unwrap_or("unavailable")
            };
            format!(
                "{}={state}[{}]",
                capability.adapter_id,
                capability.actions.join(",")
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::format_capability_snapshot;
    use device_hub_shared::{AdapterCapability, CapabilitySnapshot};

    #[test]
    fn startup_capability_summary_reports_final_registration_state() {
        let snapshot = CapabilitySnapshot {
            adapters: vec![
                AdapterCapability {
                    adapter_id: "serial".into(),
                    compiled: true,
                    enabled: true,
                    available: true,
                    actions: vec!["serial.tail".into(), "serial.write".into()],
                    unavailable_reason: None,
                },
                AdapterCapability {
                    adapter_id: "ssh".into(),
                    compiled: false,
                    enabled: false,
                    available: false,
                    actions: vec!["ssh.exec".into()],
                    unavailable_reason: Some("not_compiled".into()),
                },
            ],
        };

        assert_eq!(
            format_capability_snapshot(&snapshot),
            "serial=available[serial.tail,serial.write]; ssh=not_compiled[ssh.exec]"
        );
    }
}
