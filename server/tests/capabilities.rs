use device_hub_server::{app::build_router, config::AppConfig};
#[cfg(not(any(feature = "ssh", feature = "jlink", feature = "adb")))]
use device_hub_shared::{AdapterCapability, CapabilitySnapshot};
use device_hub_shared::{Device, SerialConfig};

async fn spawn(config: AppConfig) -> (String, reqwest::Client) {
    let app = build_router(config).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = format!(
        "http://{}",
        listener.local_addr().expect("test server address")
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("test server");
    });
    (address, reqwest::Client::new())
}

fn serial_config(enabled: bool) -> AppConfig {
    let artifacts = tempfile::tempdir().expect("artifacts");
    let artifacts_dir = artifacts.keep();
    let mut config = AppConfig {
        devices: vec![Device {
            id: "serial-device".into(),
            label: "Serial device".into(),
            ssh: None,
            serial: Some(SerialConfig {
                port: if cfg!(windows) {
                    "COM25".into()
                } else {
                    "/dev/ttyUSB0".into()
                },
                baud: 115200,
                selector: None,
            }),
            jlink: None,
            adb: None,
        }],
        artifacts_dir: Some(artifacts_dir.display().to_string()),
        ..Default::default()
    };
    config.adapters.serial = enabled;
    config
}

#[cfg(not(any(feature = "ssh", feature = "jlink", feature = "adb")))]
fn assert_unavailable(capability: &AdapterCapability, reason: &str) {
    assert!(!capability.available);
    assert_eq!(capability.unavailable_reason.as_deref(), Some(reason));
}

#[tokio::test]
#[cfg(not(any(feature = "ssh", feature = "jlink", feature = "adb")))]
async fn serial_only_build_reports_other_adapters_as_not_compiled() {
    let (address, client) = spawn(serial_config(true)).await;

    let snapshot: CapabilitySnapshot = client
        .get(format!("{address}/api/capabilities"))
        .send()
        .await
        .expect("capability request")
        .json()
        .await
        .expect("capability response");

    let serial = snapshot.adapter("serial").expect("serial capability");
    assert!(serial.compiled);
    assert!(serial.enabled);
    assert!(serial.available);
    for adapter_id in ["ssh", "jlink", "adb"] {
        let capability = snapshot.adapter(adapter_id).expect("known capability");
        assert!(!capability.compiled);
        assert!(!capability.enabled);
        assert_unavailable(capability, "not_compiled");
    }
}

#[tokio::test]
async fn runtime_disabled_serial_action_is_rejected_before_device_execution() {
    let (address, client) = spawn(serial_config(false)).await;

    let response = client
        .post(format!(
            "{address}/api/devices/serial-device/actions/serial.tail"
        ))
        .json(&Option::<device_hub_shared::ActionRequest>::None)
        .send()
        .await
        .expect("action request");

    assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response.text().await.expect("error body"),
        r#"{"error":"capability unavailable: action `serial.tail` (disabled)"}"#
    );
}

#[tokio::test]
#[cfg(not(feature = "jlink"))]
async fn host_discovery_for_an_uncompiled_adapter_returns_capability_error() {
    let (address, client) = spawn(serial_config(true)).await;

    let response = client
        .get(format!("{address}/api/host/jlink-probes"))
        .send()
        .await
        .expect("jlink discovery request");

    assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response.text().await.expect("error body"),
        r#"{"error":"capability unavailable: adapter `jlink` (not_compiled)"}"#
    );
}
