use std::{path::PathBuf, time::Duration};

use device_hub_server::{
    app::{
        build_router_with_job_policy, build_router_with_job_policy_and_board_profiles,
        build_router_with_job_policy_and_host_discovery,
        build_router_with_job_policy_and_serial_stream_opener,
    },
    config::AppConfig,
    ports::{HostDiscoveryError, HostDiscoveryPort},
    services::job_manager::JobPolicyProvider,
};
use device_hub_shared::{
    ActionRequest, AdbConfig, BoardProfile, Device, DiscoveredJlinkProbe, DiscoveredSerialPort,
    JlinkConfig, Job, JobStatus, LogEvent, SerialConfig, SerialExecRequest, SshConfig,
};
use futures::{StreamExt, future::BoxFuture};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

pub struct TestApp {
    pub address: String,
    pub api_client: reqwest::Client,
    pub artifacts_dir: Option<PathBuf>,
}

impl TestApp {
    pub fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.address, path)
    }

    pub fn ws_endpoint(&self, path: &str) -> String {
        self.endpoint(path).replacen("http://", "ws://", 1)
    }

    pub async fn post_action(
        &self,
        device_id: &str,
        action: &str,
        body: Option<&ActionRequest>,
    ) -> reqwest::Response {
        let request = self
            .api_client
            .post(self.endpoint(&format!("/api/devices/{device_id}/actions/{action}")));

        let request = match body {
            Some(body) => request.json(body),
            None => request.json(&Option::<ActionRequest>::None),
        };

        request.send().await.expect("failed to execute request")
    }

    pub async fn exec_serial(
        &self,
        device_id: &str,
        body: &SerialExecRequest,
    ) -> reqwest::Response {
        self.api_client
            .post(self.endpoint(&format!("/api/devices/{device_id}/serial/exec")))
            .json(body)
            .send()
            .await
            .expect("failed to execute request")
    }

    pub async fn get_job(&self, job_id: &str) -> reqwest::Response {
        self.api_client
            .get(self.endpoint(&format!("/api/jobs/{job_id}")))
            .send()
            .await
            .expect("failed to execute request")
    }

    pub async fn create_device(&self, device: &Device) -> reqwest::Response {
        self.api_client
            .post(self.endpoint("/api/devices"))
            .json(device)
            .send()
            .await
            .expect("failed to execute request")
    }

    pub async fn update_device(&self, device_id: &str, device: &Device) -> reqwest::Response {
        self.api_client
            .put(self.endpoint(&format!("/api/devices/{device_id}")))
            .json(device)
            .send()
            .await
            .expect("failed to execute request")
    }

    pub async fn delete_device(&self, device_id: &str) -> reqwest::Response {
        self.api_client
            .delete(self.endpoint(&format!("/api/devices/{device_id}")))
            .send()
            .await
            .expect("failed to execute request")
    }

    pub async fn get_job_evidence(&self, job_id: &str) -> reqwest::Response {
        self.api_client
            .get(self.endpoint(&format!("/api/jobs/{job_id}/evidence")))
            .send()
            .await
            .expect("failed to execute request")
    }

    pub async fn cancel_job(&self, job_id: &str) -> reqwest::Response {
        self.api_client
            .delete(self.endpoint(&format!("/api/jobs/{job_id}")))
            .send()
            .await
            .expect("failed to execute request")
    }

    pub async fn rerun_job(&self, job_id: &str) -> reqwest::Response {
        self.api_client
            .post(self.endpoint(&format!("/api/jobs/{job_id}/rerun")))
            .send()
            .await
            .expect("failed to execute request")
    }

    pub async fn download_job_artifact(
        &self,
        job_id: &str,
        artifact_name: &str,
    ) -> reqwest::Response {
        self.api_client
            .get(self.endpoint(&format!("/api/jobs/{job_id}/artifacts/{artifact_name}")))
            .send()
            .await
            .expect("failed to execute request")
    }

    pub async fn get_job_events(&self, job_id: &str) -> reqwest::Response {
        self.api_client
            .get(self.endpoint(&format!("/api/jobs/{job_id}/events")))
            .send()
            .await
            .expect("failed to execute request")
    }

    pub async fn get_events(&self, query: &str) -> reqwest::Response {
        let suffix = if query.trim().is_empty() {
            String::new()
        } else {
            format!("?{query}")
        };
        self.api_client
            .get(self.endpoint(&format!("/api/events{suffix}")))
            .send()
            .await
            .expect("failed to execute request")
    }

    pub async fn clear_event_cache(&self) -> reqwest::Response {
        self.api_client
            .delete(self.endpoint("/api/events"))
            .send()
            .await
            .expect("failed to execute request")
    }

    pub async fn get_serial_ports(&self) -> reqwest::Response {
        self.api_client
            .get(self.endpoint("/api/host/serial-ports"))
            .send()
            .await
            .expect("failed to execute request")
    }

    pub async fn get_jlink_probes(&self) -> reqwest::Response {
        self.api_client
            .get(self.endpoint("/api/host/jlink-probes"))
            .send()
            .await
            .expect("failed to execute request")
    }

    pub async fn get_board_profiles(&self) -> reqwest::Response {
        self.api_client
            .get(self.endpoint("/api/board-profiles"))
            .send()
            .await
            .expect("failed to execute request")
    }

    pub async fn wait_for_job_status(&self, job_id: &str, status: JobStatus) -> Job {
        for _ in 0..20 {
            let response = self.get_job(job_id).await;
            if response.status().is_success() {
                let job: Job = response.json().await.expect("invalid job json");
                if job.status == status {
                    return job;
                }
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        panic!("job {job_id} did not reach expected status");
    }
}

pub type TestWsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub async fn spawn_app() -> TestApp {
    spawn_app_with_config(test_config()).await
}

pub async fn spawn_app_with_config(config: AppConfig) -> TestApp {
    spawn_app_with_config_and_job_policy(config, JobPolicyProvider::default()).await
}

pub async fn spawn_app_with_config_and_host_discovery(
    config: AppConfig,
    host_discovery: StaticHostDiscovery,
) -> TestApp {
    let artifacts_dir = config.artifacts_dir.clone().map(PathBuf::from);
    let app = build_router_with_job_policy_and_host_discovery(
        config,
        JobPolicyProvider::default(),
        host_discovery,
    )
    .await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind random port");
    let port = listener
        .local_addr()
        .expect("failed to read local addr")
        .port();

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("test server crashed");
    });

    TestApp {
        address: format!("http://127.0.0.1:{port}"),
        api_client: reqwest::Client::new(),
        artifacts_dir,
    }
}

pub async fn spawn_app_with_config_and_board_profiles(
    config: AppConfig,
    board_profiles: Vec<BoardProfile>,
) -> TestApp {
    let artifacts_dir = config.artifacts_dir.clone().map(PathBuf::from);
    let app = build_router_with_job_policy_and_board_profiles(
        config,
        JobPolicyProvider::default(),
        board_profiles,
    )
    .await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind random port");
    let port = listener
        .local_addr()
        .expect("failed to read local addr")
        .port();

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("test server crashed");
    });

    TestApp {
        address: format!("http://127.0.0.1:{port}"),
        api_client: reqwest::Client::new(),
        artifacts_dir,
    }
}

pub async fn spawn_app_with_config_and_job_policy(
    config: AppConfig,
    policy_provider: JobPolicyProvider,
) -> TestApp {
    let artifacts_dir = config.artifacts_dir.clone().map(PathBuf::from);
    let app = build_router_with_job_policy(config, policy_provider).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind random port");
    let port = listener
        .local_addr()
        .expect("failed to read local addr")
        .port();

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("test server crashed");
    });

    TestApp {
        address: format!("http://127.0.0.1:{port}"),
        api_client: reqwest::Client::new(),
        artifacts_dir,
    }
}

pub async fn spawn_app_with_config_job_policy_and_serial_stream_opener(
    config: AppConfig,
    policy_provider: JobPolicyProvider,
    max_message_bytes: usize,
    opener: impl Fn(
        &device_hub_shared::SerialConfig,
    ) -> Result<tokio_serial::SerialStream, std::io::Error>
    + Send
    + Sync
    + 'static,
) -> TestApp {
    let artifacts_dir = config.artifacts_dir.clone().map(PathBuf::from);
    let app = build_router_with_job_policy_and_serial_stream_opener(
        config,
        policy_provider,
        max_message_bytes,
        opener,
    )
    .await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind random port");
    let port = listener
        .local_addr()
        .expect("failed to read local addr")
        .port();

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("test server crashed");
    });

    TestApp {
        address: format!("http://127.0.0.1:{port}"),
        api_client: reqwest::Client::new(),
        artifacts_dir,
    }
}

fn test_config() -> AppConfig {
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
                adb: None,
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
                adb: Some(AdbConfig {
                    serial: "08211FDD4002KE".into(),
                    transport: None,
                    default_logcat_args: vec!["-v".into(), "time".into()],
                }),
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
                adb: None,
            },
        ],
        device_registry_path: None,
        ssh_program: None,
        jlink_program: None,
        adb_program: None,
        artifacts_dir: None,
    }
}

pub async fn wait_for_job_events(app: &TestApp, job_id: &str, expected: usize) -> Vec<LogEvent> {
    for _ in 0..20 {
        let response = app.get_job_events(job_id).await;
        if response.status().is_success() {
            let events: Vec<LogEvent> = response.json().await.expect("invalid job events json");
            if events.len() >= expected {
                return events;
            }
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    panic!("job {job_id} did not emit expected events");
}

pub async fn connect_ws(app: &TestApp, path: &str) -> TestWsStream {
    let (stream, _) = connect_async(app.ws_endpoint(path))
        .await
        .expect("failed to connect websocket");
    stream
}

pub async fn recv_ws_event(stream: &mut TestWsStream) -> LogEvent {
    for _ in 0..20 {
        if let Ok(Some(message)) =
            tokio::time::timeout(Duration::from_millis(250), stream.next()).await
        {
            match message.expect("websocket receive error") {
                Message::Text(text) => {
                    return serde_json::from_str(&text).expect("invalid websocket log event json");
                }
                Message::Binary(_) | Message::Ping(_) | Message::Pong(_) => continue,
                Message::Close(frame) => panic!("websocket closed unexpectedly: {frame:?}"),
                Message::Frame(_) => continue,
            }
        }
    }

    panic!("websocket did not emit expected event");
}

#[derive(Clone, Default)]
pub struct StaticHostDiscovery {
    pub serial_ports: Vec<DiscoveredSerialPort>,
    pub jlink_probes: Vec<DiscoveredJlinkProbe>,
}

impl HostDiscoveryPort for StaticHostDiscovery {
    fn list_serial_ports(
        &self,
    ) -> BoxFuture<'static, Result<Vec<DiscoveredSerialPort>, HostDiscoveryError>> {
        let serial_ports = self.serial_ports.clone();
        Box::pin(async move { Ok(serial_ports) })
    }

    fn list_jlink_probes(
        &self,
    ) -> BoxFuture<'static, Result<Vec<DiscoveredJlinkProbe>, HostDiscoveryError>> {
        let jlink_probes = self.jlink_probes.clone();
        Box::pin(async move { Ok(jlink_probes) })
    }
}
