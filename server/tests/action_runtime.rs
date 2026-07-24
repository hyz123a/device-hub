use device_hub_server::{
    app::build_router_with_job_policy_and_action_runtime,
    config::AppConfig,
    ports::ActionExecutionContext,
    services::{
        adapter_action_runner::{AdapterActionHandler, CapabilityRegistry},
        job_manager::JobPolicyProvider,
        job_runtime::{adapter_event_with_source, succeed_job},
    },
};
use device_hub_shared::{
    ActionRequest, AdapterCapability, CapabilitySnapshot, Device, Job, JobEvidence, JobStatus,
    LogChannel, LogLevel, LogSource, SerialConfig,
};
use futures::future::BoxFuture;

#[derive(Clone)]
struct FakeSerialHandler;

impl AdapterActionHandler for FakeSerialHandler {
    fn run(
        &self,
        context: ActionExecutionContext,
        job: Job,
        _device: Device,
        action: String,
        payload: Option<ActionRequest>,
    ) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            if action == "serial.tail" {
                futures::future::pending::<()>().await;
                return;
            }
            context.events.publish(adapter_event_with_source(
                &job,
                LogSource::Serial,
                LogChannel::Stdout,
                LogLevel::Info,
                format!(
                    "fake serial output: {}",
                    payload
                        .and_then(|request| request.payload)
                        .unwrap_or_default()
                ),
            ));
            succeed_job(
                &context.jobs,
                &context.events,
                &context.artifacts,
                &job,
                "fake serial action completed",
            );
        })
    }
}

fn fake_runtime() -> CapabilityRegistry {
    let mut registry = CapabilityRegistry::new(vec![AdapterCapability {
        adapter_id: "serial".into(),
        compiled: true,
        enabled: true,
        available: false,
        actions: vec![
            "serial.tail".into(),
            "serial.write".into(),
            "serial.exec".into(),
        ],
        unavailable_reason: Some("not_registered".into()),
    }]);
    registry
        .register(
            "serial",
            &["serial.tail", "serial.write", "serial.exec"],
            FakeSerialHandler,
        )
        .expect("fake serial registration");
    registry
}

async fn spawn() -> (String, reqwest::Client) {
    let artifacts = tempfile::tempdir().expect("artifacts").keep();
    let config = AppConfig {
        devices: vec![Device {
            id: "serial-device".into(),
            label: "Serial device".into(),
            ssh: None,
            serial: Some(SerialConfig {
                port: "fixture".into(),
                baud: 115200,
                selector: None,
            }),
            jlink: None,
            adb: None,
        }],
        artifacts_dir: Some(artifacts.display().to_string()),
        ..Default::default()
    };
    let app = build_router_with_job_policy_and_action_runtime(
        config,
        JobPolicyProvider::default(),
        fake_runtime(),
    )
    .await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = format!("http://{}", listener.local_addr().expect("address"));
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("test server");
    });
    (address, reqwest::Client::new())
}

async fn wait_for_status(
    client: &reqwest::Client,
    address: &str,
    job_id: &str,
    expected: JobStatus,
) -> Job {
    for _ in 0..40 {
        let job: Job = client
            .get(format!("{address}/api/jobs/{job_id}"))
            .send()
            .await
            .expect("job request")
            .json()
            .await
            .expect("job response");
        if job.status == expected {
            return job;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("job {job_id} did not reach {expected:?}");
}

#[tokio::test]
async fn injected_action_runtime_preserves_start_events_and_rerun() {
    let (address, client) = spawn().await;

    let created: Job = client
        .post(format!(
            "{address}/api/devices/serial-device/actions/serial.write"
        ))
        .json(&Some(ActionRequest {
            payload: Some("hello".into()),
        }))
        .send()
        .await
        .expect("start action")
        .json()
        .await
        .expect("start response");
    wait_for_status(&client, &address, &created.id, JobStatus::Succeeded).await;

    let events: Vec<device_hub_shared::LogEvent> = client
        .get(format!("{address}/api/jobs/{}/events", created.id))
        .send()
        .await
        .expect("events request")
        .json()
        .await
        .expect("events response");
    assert!(
        events
            .iter()
            .any(|event| event.message == "fake serial output: hello")
    );
    let evidence: JobEvidence = client
        .get(format!("{address}/api/jobs/{}/evidence", created.id))
        .send()
        .await
        .expect("evidence request")
        .json()
        .await
        .expect("evidence response");
    assert_eq!(evidence.job.status, JobStatus::Succeeded);
    assert!(
        evidence
            .artifacts
            .iter()
            .any(|artifact| artifact.name == "job.json")
    );
    assert!(
        evidence
            .artifacts
            .iter()
            .any(|artifact| artifact.name == "events.ndjson")
    );

    let rerun: Job = client
        .post(format!("{address}/api/jobs/{}/rerun", created.id))
        .send()
        .await
        .expect("rerun request")
        .json()
        .await
        .expect("rerun response");
    assert_ne!(rerun.id, created.id);
    assert_eq!(rerun.payload.as_deref(), Some("hello"));
    wait_for_status(&client, &address, &rerun.id, JobStatus::Succeeded).await;
}

#[tokio::test]
async fn injected_long_running_action_is_cancelled_by_the_job_manager() {
    let (address, client) = spawn().await;
    let created: Job = client
        .post(format!(
            "{address}/api/devices/serial-device/actions/serial.tail"
        ))
        .json(&Option::<ActionRequest>::None)
        .send()
        .await
        .expect("start tail")
        .json()
        .await
        .expect("tail response");
    wait_for_status(&client, &address, &created.id, JobStatus::Running).await;

    let cancelled: Job = client
        .delete(format!("{address}/api/jobs/{}", created.id))
        .send()
        .await
        .expect("cancel request")
        .json()
        .await
        .expect("cancel response");

    assert_eq!(cancelled.status, JobStatus::Cancelled);
}

#[tokio::test]
async fn injected_runtime_is_the_capability_api_source() {
    let (address, client) = spawn().await;

    let snapshot: CapabilitySnapshot = client
        .get(format!("{address}/api/capabilities"))
        .send()
        .await
        .expect("capability request")
        .json()
        .await
        .expect("capability response");

    assert!(snapshot.is_action_available("serial.write"));
    assert!(!snapshot.is_action_available("ssh.exec"));
}
