use device_hub_server::config::AppConfig;
use device_hub_shared::{
    Device, DeviceConnectionSummary, DeviceDetail, DiscoveredJlinkProbe, DiscoveredSerialPort,
    JlinkConfig, Job, JobStatus, SerialConfig, SshConfig,
};

use crate::helpers::{
    StaticHostDiscovery, spawn_app, spawn_app_with_config, spawn_app_with_config_and_host_discovery,
};

fn base_config() -> AppConfig {
    AppConfig {
        devices: vec![
            Device {
                id: "board-zeta".into(),
                label: "J-Link Board".into(),
                ssh: None,
                serial: None,
                jlink: Some(JlinkConfig {
                    serial: "123456789".into(),
                    device: "STM32H743".into(),
                    interface: "SWD".into(),
                    speed_khz: 4000,
                    autoconnect: true,
                }),
            },
            Device {
                id: "board-alpha".into(),
                label: "Linux Board".into(),
                ssh: Some(SshConfig {
                    host: "192.168.1.100".into(),
                    user: "root".into(),
                    key: "~/.ssh/id_rsa".into(),
                    default_log_path: "/tmp/sdcard/debug_log".into(),
                }),
                serial: Some(SerialConfig {
                    port: "/dev/ttyUSB0".into(),
                    baud: 115200,
                }),
                jlink: None,
            },
            Device {
                id: "board-mixed".into(),
                label: "Full Dev Board".into(),
                ssh: Some(SshConfig {
                    host: "192.168.1.102".into(),
                    user: "root".into(),
                    key: "~/.ssh/id_lab".into(),
                    default_log_path: "/tmp/sdcard/debug_log".into(),
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
        ],
        device_registry_path: None,
        ssh_program: None,
        jlink_program: None,
        artifacts_dir: None,
    }
}

fn create_only_serial_device() -> Device {
    Device {
        id: "board-gamma".into(),
        label: "Bench UART".into(),
        ssh: None,
        serial: Some(SerialConfig {
            port: "/dev/ttyUSB9".into(),
            baud: 57600,
        }),
        jlink: None,
    }
}

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
                    default_log_path: "/tmp/sdcard/debug_log".into(),
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
async fn create_device_adds_it_to_the_registry() {
    let app = spawn_app().await;
    let device = create_only_serial_device();

    let response = app.create_device(&device).await;

    assert!(response.status().is_success());
    let created: Device = response.json().await.expect("invalid created device json");
    assert_eq!(created, device);

    let devices: Vec<Device> = app
        .api_client
        .get(app.endpoint("/api/devices"))
        .send()
        .await
        .expect("list devices request")
        .json()
        .await
        .expect("invalid devices json");
    assert!(devices.iter().any(|listed| listed.id == "board-gamma"));
}

#[tokio::test]
async fn update_device_replaces_connection_settings() {
    let app = spawn_app().await;
    let updated = Device {
        id: "board-alpha".into(),
        label: "Linux Board v2".into(),
        ssh: Some(SshConfig {
            host: "192.168.1.150".into(),
            user: "lab".into(),
            key: "~/.ssh/id_lab_new".into(),
            default_log_path: "/var/log/device.log".into(),
        }),
        serial: None,
        jlink: Some(JlinkConfig {
            serial: "112233445".into(),
            device: "STM32F429".into(),
            interface: "SWD".into(),
            speed_khz: 2000,
            autoconnect: false,
        }),
    };

    let response = app.update_device("board-alpha", &updated).await;

    assert!(response.status().is_success());
    let returned: Device = response.json().await.expect("invalid updated device json");
    assert_eq!(returned, updated);

    let detail: DeviceDetail = app
        .api_client
        .get(app.endpoint("/api/devices/board-alpha"))
        .send()
        .await
        .expect("get updated device")
        .json()
        .await
        .expect("invalid device detail");
    assert_eq!(detail.device, updated);
    assert_eq!(detail.capabilities.has_ssh, true);
    assert_eq!(detail.capabilities.has_serial, false);
    assert_eq!(detail.capabilities.has_jlink, true);
}

#[tokio::test]
async fn delete_device_removes_it_from_the_registry() {
    let app = spawn_app().await;

    let response = app.delete_device("board-zeta").await;

    assert!(response.status().is_success());
    let deleted: Device = response.json().await.expect("invalid deleted device json");
    assert_eq!(deleted.id, "board-zeta");

    let missing = app
        .api_client
        .get(app.endpoint("/api/devices/board-zeta"))
        .send()
        .await
        .expect("get deleted device");
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn create_device_rejects_unconfigured_device() {
    let app = spawn_app().await;
    let invalid = Device {
        id: "board-empty".into(),
        label: "Missing links".into(),
        ssh: None,
        serial: None,
        jlink: None,
    };

    let response = app.create_device(&invalid).await;

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(
        body["error"],
        "bad request: device must configure at least one connection"
    );
}

#[tokio::test]
async fn create_device_persists_registry_changes_across_server_restarts() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let registry_path = tempdir.path().join("device-registry.yaml");
    let mut config = base_config();
    config.device_registry_path = Some(registry_path.display().to_string());
    let app = spawn_app_with_config(config.clone()).await;
    let device = create_only_serial_device();

    let created = app.create_device(&device).await;
    assert!(created.status().is_success());

    let restarted = spawn_app_with_config(config).await;
    let response = restarted
        .api_client
        .get(restarted.endpoint("/api/devices/board-gamma"))
        .send()
        .await
        .expect("get persisted device");

    assert!(response.status().is_success());
    let detail: DeviceDetail = response.json().await.expect("invalid device detail json");
    assert_eq!(detail.device, device);
}

#[tokio::test]
async fn list_devices_falls_back_to_config_when_registry_file_is_empty() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let registry_path = tempdir.path().join("device-registry.yaml");
    std::fs::write(&registry_path, "devices: []\n").expect("write empty registry");

    let mut config = base_config();
    config.device_registry_path = Some(registry_path.display().to_string());
    let app = spawn_app_with_config(config).await;

    let response = app
        .api_client
        .get(app.endpoint("/api/devices"))
        .send()
        .await
        .expect("list devices request");

    assert!(response.status().is_success());
    let devices: Vec<Device> = response.json().await.expect("invalid devices json");
    let ids: Vec<_> = devices.into_iter().map(|device| device.id).collect();
    assert_eq!(ids, vec!["board-alpha", "board-mixed", "board-zeta"]);

    let persisted = std::fs::read_to_string(registry_path).expect("read registry file");
    assert!(persisted.contains("board-alpha"));
}

#[tokio::test]
async fn delete_device_rejects_devices_with_active_jobs() {
    let app = spawn_app().await;

    let created: Job = app
        .post_action(
            "board-alpha",
            "ssh.tail",
            Some(&device_hub_shared::ActionRequest {
                payload: Some("/var/log/messages".into()),
            }),
        )
        .await
        .json()
        .await
        .expect("invalid job json");
    app.wait_for_job_status(&created.id, JobStatus::Running)
        .await;

    let response = app.delete_device("board-alpha").await;

    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(
        body["error"],
        "conflict: device `board-alpha` has active jobs"
    );

    assert!(app.cancel_job(&created.id).await.status().is_success());
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

#[tokio::test]
async fn list_serial_ports_returns_discovered_host_devices() {
    let app = spawn_app_with_config_and_host_discovery(
        base_config(),
        StaticHostDiscovery {
            serial_ports: vec![
                DiscoveredSerialPort {
                    path: "/dev/ttyUSB1".into(),
                    label: Some("Segger UART".into()),
                },
                DiscoveredSerialPort {
                    path: "/dev/ttyACM0".into(),
                    label: Some("Debug console".into()),
                },
            ],
            jlink_probes: Vec::new(),
        },
    )
    .await;

    let response = app.get_serial_ports().await;

    assert!(response.status().is_success());
    let ports: Vec<DiscoveredSerialPort> =
        response.json().await.expect("invalid serial ports json");
    assert_eq!(
        ports,
        vec![
            DiscoveredSerialPort {
                path: "/dev/ttyACM0".into(),
                label: Some("Debug console".into()),
            },
            DiscoveredSerialPort {
                path: "/dev/ttyUSB1".into(),
                label: Some("Segger UART".into()),
            },
        ]
    );
}

#[tokio::test]
async fn list_jlink_probes_returns_connected_probes() {
    let app = spawn_app_with_config_and_host_discovery(
        base_config(),
        StaticHostDiscovery {
            serial_ports: Vec::new(),
            jlink_probes: vec![
                DiscoveredJlinkProbe {
                    serial: "987654321".into(),
                    product: Some("J-Link PLUS".into()),
                },
                DiscoveredJlinkProbe {
                    serial: "123456789".into(),
                    product: Some("J-Link EDU Mini".into()),
                },
            ],
        },
    )
    .await;

    let response = app.get_jlink_probes().await;

    assert!(response.status().is_success());
    let probes: Vec<DiscoveredJlinkProbe> =
        response.json().await.expect("invalid jlink probes json");
    assert_eq!(
        probes,
        vec![
            DiscoveredJlinkProbe {
                serial: "123456789".into(),
                product: Some("J-Link EDU Mini".into()),
            },
            DiscoveredJlinkProbe {
                serial: "987654321".into(),
                product: Some("J-Link PLUS".into()),
            },
        ]
    );
}
