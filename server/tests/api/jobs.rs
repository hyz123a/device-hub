use device_hub_shared::{Job, JobStatus};

use crate::helpers::spawn_app;

#[tokio::test]
async fn get_job_returns_404_for_unknown_job() {
    let app = spawn_app().await;

    let response = app.get_job("job_missing").await;

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(body["error"], "job not found");
}

#[tokio::test]
async fn get_job_eventually_returns_a_succeeded_job() {
    let app = spawn_app().await;

    let created: Job = app
        .post_action("board-alpha", "ssh.exec", None)
        .await
        .json()
        .await
        .expect("invalid job json");

    let job = app
        .wait_for_job_status(&created.id, JobStatus::Succeeded)
        .await;

    assert_eq!(job.id, created.id);
    assert_eq!(job.device_id, "board-alpha");
    assert_eq!(job.action, "ssh.exec");
    assert_eq!(job.status, JobStatus::Succeeded);
}

#[tokio::test]
async fn cancel_job_marks_running_job_as_cancelled() {
    let app = spawn_app().await;

    let created: Job = app
        .post_action("board-alpha", "ssh.exec", None)
        .await
        .json()
        .await
        .expect("invalid job json");

    let _ = app
        .wait_for_job_status(&created.id, JobStatus::Running)
        .await;

    let response = app.cancel_job(&created.id).await;

    assert!(response.status().is_success());
    let cancelled: Job = response.json().await.expect("invalid cancelled job json");
    assert_eq!(cancelled.id, created.id);
    assert_eq!(cancelled.status, JobStatus::Cancelled);

    let fetched: Job = app
        .get_job(&created.id)
        .await
        .json()
        .await
        .expect("invalid fetched job json");
    assert_eq!(fetched.status, JobStatus::Cancelled);
}

#[tokio::test]
async fn cancel_job_returns_404_for_unknown_job() {
    let app = spawn_app().await;

    let response = app.cancel_job("job_missing").await;

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(body["error"], "job not found");
}
