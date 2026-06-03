use device_hub_shared::{ActionRequest, Job, JobStatus};

use crate::helpers::spawn_app;

#[tokio::test]
async fn start_action_creates_a_job_for_supported_device_action() {
    let app = spawn_app().await;

    let response = app
        .post_action(
            "board-alpha",
            "ssh.exec",
            Some(&ActionRequest {
                payload: Some("uname -a".into()),
            }),
        )
        .await;

    assert!(response.status().is_success());
    let job: Job = response.json().await.expect("invalid job json");
    assert_eq!(job.device_id, "board-alpha");
    assert_eq!(job.action, "ssh.exec");
    assert_eq!(job.status, JobStatus::Pending);
    assert!(job.id.starts_with("job_"));
}

#[tokio::test]
async fn start_action_returns_404_for_unknown_device() {
    let app = spawn_app().await;

    let response = app.post_action("missing-board", "ssh.exec", None).await;

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(body["error"], "device not found");
}

#[tokio::test]
async fn start_action_rejects_unsupported_action() {
    let app = spawn_app().await;

    let response = app.post_action("board-alpha", "ssh.unknown", None).await;

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(
        body["error"],
        "bad request: unsupported action `ssh.unknown`"
    );
}

#[tokio::test]
async fn start_action_rejects_action_not_supported_by_device_capabilities() {
    let app = spawn_app().await;

    let response = app.post_action("board-zeta", "ssh.exec", None).await;

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(
        body["error"],
        "bad request: device `board-zeta` does not support action `ssh.exec`"
    );
}

#[tokio::test]
async fn start_action_rejects_missing_payload_for_ssh_exec() {
    let app = spawn_app().await;

    let response = app.post_action("board-alpha", "ssh.exec", None).await;

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(
        body["error"],
        "bad request: action `ssh.exec` requires a command payload"
    );
}

#[tokio::test]
async fn start_action_allows_missing_payload_for_ssh_tail_when_device_has_default_log_path() {
    let app = spawn_app().await;

    let response = app.post_action("board-alpha", "ssh.tail", None).await;

    assert!(response.status().is_success());
    let job: Job = response.json().await.expect("invalid job json");
    assert_eq!(job.device_id, "board-alpha");
    assert_eq!(job.action, "ssh.tail");
    assert_eq!(job.payload, None);
}

#[tokio::test]
async fn start_action_creates_a_job_for_serial_tail() {
    let app = spawn_app().await;

    let response = app.post_action("board-alpha", "serial.tail", None).await;

    assert!(response.status().is_success());
    let job: Job = response.json().await.expect("invalid job json");
    assert_eq!(job.device_id, "board-alpha");
    assert_eq!(job.action, "serial.tail");
    assert_eq!(job.status, JobStatus::Pending);
}

#[tokio::test]
async fn start_action_creates_a_job_for_adb_logcat() {
    let app = spawn_app().await;

    let response = app.post_action("board-alpha", "adb.logcat", None).await;

    assert!(response.status().is_success());
    let job: Job = response.json().await.expect("invalid job json");
    assert_eq!(job.device_id, "board-alpha");
    assert_eq!(job.action, "adb.logcat");
    assert_eq!(job.status, JobStatus::Pending);
}

#[tokio::test]
async fn start_action_rejects_adb_logcat_for_device_without_adb() {
    let app = spawn_app().await;

    let response = app.post_action("board-zeta", "adb.logcat", None).await;

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(
        body["error"],
        "bad request: device `board-zeta` does not support action `adb.logcat`"
    );
}

#[tokio::test]
async fn start_action_creates_a_job_for_serial_write() {
    let app = spawn_app().await;

    let response = app
        .post_action(
            "board-alpha",
            "serial.write",
            Some(&ActionRequest {
                payload: Some("help\n".into()),
            }),
        )
        .await;

    assert!(response.status().is_success());
    let job: Job = response.json().await.expect("invalid job json");
    assert_eq!(job.device_id, "board-alpha");
    assert_eq!(job.action, "serial.write");
    assert_eq!(job.payload.as_deref(), Some("help\n"));
    assert_eq!(job.status, JobStatus::Pending);
}

#[tokio::test]
async fn start_action_creates_a_job_for_serial_exec() {
    let app = spawn_app().await;

    let response = app
        .post_action(
            "board-alpha",
            "serial.exec",
            Some(&ActionRequest {
                payload: Some("ip route".into()),
            }),
        )
        .await;

    assert!(response.status().is_success());
    let job: Job = response.json().await.expect("invalid job json");
    assert_eq!(job.device_id, "board-alpha");
    assert_eq!(job.action, "serial.exec");
    assert_eq!(job.payload.as_deref(), Some("ip route"));
    assert_eq!(job.status, JobStatus::Pending);
}

#[tokio::test]
async fn start_action_rejects_missing_payload_for_serial_exec() {
    let app = spawn_app().await;

    let response = app.post_action("board-alpha", "serial.exec", None).await;

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(
        body["error"],
        "bad request: action `serial.exec` requires a command payload"
    );
}

#[tokio::test]
async fn start_action_rejects_missing_payload_for_serial_write() {
    let app = spawn_app().await;

    let response = app.post_action("board-alpha", "serial.write", None).await;

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(
        body["error"],
        "bad request: action `serial.write` requires a payload"
    );
}

#[tokio::test]
async fn start_action_rejects_serial_tail_for_device_without_serial() {
    let app = spawn_app().await;

    let response = app.post_action("board-zeta", "serial.tail", None).await;

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(
        body["error"],
        "bad request: device `board-zeta` does not support action `serial.tail`"
    );
}

#[tokio::test]
async fn start_action_rejects_serial_write_for_device_without_serial() {
    let app = spawn_app().await;

    let response = app
        .post_action(
            "board-zeta",
            "serial.write",
            Some(&ActionRequest {
                payload: Some("help".into()),
            }),
        )
        .await;

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(
        body["error"],
        "bad request: device `board-zeta` does not support action `serial.write`"
    );
}
