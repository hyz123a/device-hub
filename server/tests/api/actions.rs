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
    assert_eq!(body["error"], "bad request: unsupported action `ssh.unknown`");
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
