use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use device_hub_server::{
    app::build_router_with_job_policy_and_serial_stream_opener, config::AppConfig,
    services::job_manager::JobPolicyProvider,
};
use device_hub_shared::{ActionRequest, Device, Job, JobStatus, LogEvent, SerialConfig};

async fn spawn(max_attempts: Option<u32>, opens: Arc<AtomicUsize>) -> (String, reqwest::Client) {
    let artifacts = tempfile::tempdir().expect("artifacts").keep();
    let mut config = AppConfig {
        devices: vec![Device {
            id: "serial-device".into(),
            label: "Serial device".into(),
            ssh: None,
            serial: Some(SerialConfig {
                port: if cfg!(windows) {
                    "COM404".into()
                } else {
                    "/dev/missing-device-hub-port".into()
                },
                baud: 115200,
                selector: None,
            }),
            jlink: None,
            adb: None,
        }],
        artifacts_dir: Some(artifacts.display().to_string()),
        ..Default::default()
    };
    config.adapters.serial_reconnect.max_attempts = max_attempts;
    config.adapters.serial_reconnect.initial_delay_ms = 10;
    config.adapters.serial_reconnect.max_delay_ms = 10;
    let app = build_router_with_job_policy_and_serial_stream_opener(
        config,
        JobPolicyProvider::default(),
        4096,
        move |_| {
            opens.fetch_add(1, Ordering::Relaxed);
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "fixture port absent",
            ))
        },
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

async fn start_tail(client: &reqwest::Client, address: &str) -> Job {
    client
        .post(format!(
            "{address}/api/devices/serial-device/actions/serial.tail"
        ))
        .json(&Option::<ActionRequest>::None)
        .send()
        .await
        .expect("start tail")
        .json()
        .await
        .expect("tail response")
}

async fn job(client: &reqwest::Client, address: &str, job_id: &str) -> Job {
    client
        .get(format!("{address}/api/jobs/{job_id}"))
        .send()
        .await
        .expect("job request")
        .json()
        .await
        .expect("job response")
}

async fn events(client: &reqwest::Client, address: &str, job_id: &str) -> Vec<LogEvent> {
    client
        .get(format!("{address}/api/jobs/{job_id}/events"))
        .send()
        .await
        .expect("events request")
        .json()
        .await
        .expect("events response")
}

#[tokio::test]
async fn cancellation_stops_an_unbounded_wait_for_device() {
    let opens = Arc::new(AtomicUsize::new(0));
    let (address, client) = spawn(None, opens.clone()).await;
    let tail = start_tail(&client, &address).await;

    for _ in 0..40 {
        if opens.load(Ordering::Relaxed) >= 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(opens.load(Ordering::Relaxed) >= 2);

    let cancelled: Job = client
        .delete(format!("{address}/api/jobs/{}", tail.id))
        .send()
        .await
        .expect("cancel tail")
        .json()
        .await
        .expect("cancel response");

    assert_eq!(cancelled.status, JobStatus::Cancelled);
    assert!(
        events(&client, &address, &tail.id)
            .await
            .iter()
            .any(|event| event.message.contains("attempt 1/unbounded"))
    );
    assert!(
        events(&client, &address, &tail.id)
            .await
            .iter()
            .any(|event| event
                .message
                .contains("serial reconnecting: attempt 1/unbounded"))
    );
}

#[tokio::test]
async fn finite_retry_policy_fails_with_observable_exhaustion() {
    let opens = Arc::new(AtomicUsize::new(0));
    let (address, client) = spawn(Some(1), opens.clone()).await;
    let tail = start_tail(&client, &address).await;

    let failed = loop {
        let current = job(&client, &address, &tail.id).await;
        if current.status == JobStatus::Failed {
            break current;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    };

    assert_eq!(failed.status, JobStatus::Failed);
    assert_eq!(opens.load(Ordering::Relaxed), 2);
    let events = events(&client, &address, &tail.id).await;
    assert!(events.iter().any(|event| {
        event
            .message
            .contains("reconnect exhausted after 1 attempts")
    }));
    assert!(
        !events
            .iter()
            .any(|event| event.message == "serial.tail completed")
    );
}
