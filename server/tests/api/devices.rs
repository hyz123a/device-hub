use device_hub_shared::{
    Device, DeviceConnectionSummary, DeviceDetail, JlinkConfig, SerialConfig, SshConfig,
};

use crate::helpers::spawn_app;

#[tokio::test]
async fn list_devices_returns_sorted_devices() {
    let app = spawn_app().await;

    let response = app
        .api_client
        .get(app.endpoint("/api/devices"))
        .send()
        .await
        .expect("failed to execute request");

    assert!(response.status().is_success());
    let devices: Vec<Device> = response.json().await.expect("invalid devices json");
    let ids: Vec<_> = devices.into_iter().map(|device| device.id).collect();
    assert_eq!(ids, vec!["board-alpha", "board-mixed", "board-zeta"]);
}

#[tokio::test]
async fn get_device_returns_detail_for_known_device() {
    let app = spawn_app().await;

    let response = app
        .api_client
        .get(app.endpoint("/api/devices/board-mixed"))
        .send()
        .await
        .expect("failed to execute request");

    assert!(response.status().is_success());
    let detail: DeviceDetail = response.json().await.expect("invalid device detail json");
    assert_eq!(
        detail,
        DeviceDetail {
            device: Device {
                id: "board-mixed".into(),
                label: "Full Dev Board".into(),
                ssh: Some(SshConfig {
                    host: "192.168.1.102".into(),
                    user: "root".into(),
                    key: "~/.ssh/id_lab".into(),
                }),
                serial: Some(SerialConfig {
                    port: "/dev/ttyUSB1".into(),
                    baud: 115200,
                }),
                jlink: Some(JlinkConfig {
                    serial: "987654321".into(),
                    device: "STM32H743".into(),
                    interface: "SWD".into(),
                    speed_khz: 4000,
                    autoconnect: true,
                }),
            },
            capabilities: DeviceConnectionSummary {
                has_ssh: true,
                has_serial: true,
                has_jlink: true,
            },
        }
    );
}

#[tokio::test]
async fn get_device_returns_404_for_unknown_device() {
    let app = spawn_app().await;

    let response = app
        .api_client
        .get(app.endpoint("/api/devices/unknown-board"))
        .send()
        .await
        .expect("failed to execute request");

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(body["error"], "device not found");
}
