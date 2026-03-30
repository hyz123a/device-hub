use std::{fs, os::unix::fs::PermissionsExt, path::Path};

use chrono::SecondsFormat;
use device_hub_server::config::AppConfig;
use device_hub_shared::{ActionRequest, Device, JlinkConfig, Job, JobStatus, SshConfig};

use crate::helpers::{
    connect_ws, recv_ws_event, spawn_app, spawn_app_with_config, wait_for_job_events,
};

fn make_fake_ssh(dir: &Path, body: &str) -> std::path::PathBuf {
    let script_path = dir.join("fake-ssh.sh");
    fs::write(&script_path, format!("#!/bin/sh\nset -eu\n{body}\n")).expect("write fake ssh");
    let mut permissions = fs::metadata(&script_path)
        .expect("fake ssh metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script_path, permissions).expect("chmod fake ssh");
    script_path
}

fn ssh_test_config(ssh_program: String, key: String) -> AppConfig {
    AppConfig {
        devices: vec![
            Device {
                id: "board-zeta".into(),
                label: "J-Link Board".into(),
                ssh: None,
                jlink: Some(JlinkConfig {
                    serial: "123456789".into(),
                    device: "STM32H743".into(),
                }),
            },
            Device {
                id: "board-alpha".into(),
                label: "Linux Board".into(),
                ssh: Some(SshConfig {
                    host: "192.168.1.100".into(),
                    user: "root".into(),
                    key,
                }),
                jlink: None,
            },
        ],
        ssh_program: Some(ssh_program),
        artifacts_dir: None,
    }
}

fn multi_ssh_test_config(ssh_program: String, key: String) -> AppConfig {
    AppConfig {
        devices: vec![
            Device {
                id: "board-alpha".into(),
                label: "Linux Board".into(),
                ssh: Some(SshConfig {
                    host: "192.168.1.100".into(),
                    user: "root".into(),
                    key: key.clone(),
                }),
                jlink: None,
            },
            Device {
                id: "board-mixed".into(),
                label: "Full Dev Board".into(),
                ssh: Some(SshConfig {
                    host: "192.168.1.102".into(),
                    user: "root".into(),
                    key,
                }),
                jlink: Some(JlinkConfig {
                    serial: "987654321".into(),
                    device: "STM32H743".into(),
                }),
            },
        ],
        ssh_program: Some(ssh_program),
        artifacts_dir: None,
    }
}

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
    let tempdir = tempfile::tempdir().expect("tempdir");
    let script = make_fake_ssh(
        tempdir.path(),
        "printf 'kernel booted\\nready\\n'\nprintf 'minor warning\\n' >&2\nexit 0",
    );
    let app = spawn_app_with_config(ssh_test_config(
        script.display().to_string(),
        tempdir.path().join("id_rsa").display().to_string(),
    ))
    .await;

    let created: Job = app
        .post_action(
            "board-alpha",
            "ssh.exec",
            Some(&ActionRequest {
                payload: Some("uname -a".into()),
            }),
        )
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

    let events = wait_for_job_events(&app, &created.id, 6).await;
    assert!(events.iter().any(|event| event.message == "exec: uname -a"));
    assert!(events.iter().any(|event| event.message == "kernel booted"));
    assert!(events.iter().any(|event| event.message == "ready"));
    assert!(events.iter().any(|event| event.message == "minor warning"));
}

#[tokio::test]
async fn get_job_eventually_returns_a_failed_job_for_non_zero_ssh_exit() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let script = make_fake_ssh(
        tempdir.path(),
        "printf 'partial output\\n'\nprintf 'permission denied\\n' >&2\nexit 7",
    );
    let app = spawn_app_with_config(ssh_test_config(
        script.display().to_string(),
        tempdir.path().join("id_rsa").display().to_string(),
    ))
    .await;

    let created: Job = app
        .post_action(
            "board-alpha",
            "ssh.exec",
            Some(&ActionRequest {
                payload: Some("false".into()),
            }),
        )
        .await
        .json()
        .await
        .expect("invalid job json");

    let job = app
        .wait_for_job_status(&created.id, JobStatus::Failed)
        .await;

    assert_eq!(job.status, JobStatus::Failed);

    let events = wait_for_job_events(&app, &created.id, 5).await;
    assert!(events.iter().any(|event| event.message == "partial output"));
    assert!(
        events
            .iter()
            .any(|event| event.message == "permission denied")
    );
    assert!(
        events
            .iter()
            .any(|event| event.message.contains("ssh.exec exited with status 7"))
    );
}

#[tokio::test]
async fn ssh_tail_streams_events_until_cancelled() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let script = make_fake_ssh(
        tempdir.path(),
        "printf 'boot log line\n'\nprintf 'tail warning\n' >&2\nsleep 5",
    );
    let app = spawn_app_with_config(ssh_test_config(
        script.display().to_string(),
        tempdir.path().join("id_rsa").display().to_string(),
    ))
    .await;

    let created: Job = app
        .post_action(
            "board-alpha",
            "ssh.tail",
            Some(&ActionRequest {
                payload: Some("/var/log/syslog".into()),
            }),
        )
        .await
        .json()
        .await
        .expect("invalid job json");

    let running = app
        .wait_for_job_status(&created.id, JobStatus::Running)
        .await;
    assert_eq!(running.action, "ssh.tail");

    let events = wait_for_job_events(&app, &created.id, 5).await;
    assert!(
        events
            .iter()
            .any(|event| event.message == "tailing logs from /var/log/syslog")
    );
    assert!(events.iter().any(|event| event.message == "boot log line"));
    assert!(events.iter().any(|event| event.message == "tail warning"));

    let response = app.cancel_job(&created.id).await;
    assert!(response.status().is_success());
    let cancelled: Job = response.json().await.expect("invalid cancelled job json");
    assert_eq!(cancelled.status, JobStatus::Cancelled);
}

#[tokio::test]
async fn ssh_tail_streams_live_events_over_websocket() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let script = make_fake_ssh(
        tempdir.path(),
        "printf 'boot log line\n'\nprintf 'tail warning\n' >&2\nsleep 5",
    );
    let app = spawn_app_with_config(ssh_test_config(
        script.display().to_string(),
        tempdir.path().join("id_rsa").display().to_string(),
    ))
    .await;

    let created: Job = app
        .post_action(
            "board-alpha",
            "ssh.tail",
            Some(&ActionRequest {
                payload: Some("/var/log/syslog".into()),
            }),
        )
        .await
        .json()
        .await
        .expect("invalid job json");

    let mut ws = connect_ws(&app, &format!("/api/ws?job_id={}", created.id)).await;

    let mut messages = Vec::new();
    for _ in 0..8 {
        messages.push(recv_ws_event(&mut ws).await.message);
        if messages
            .iter()
            .any(|message| message == "tailing logs from /var/log/syslog")
            && messages.iter().any(|message| message == "boot log line")
            && messages.iter().any(|message| message == "tail warning")
        {
            break;
        }
    }

    assert!(
        messages
            .iter()
            .any(|message| message == "tailing logs from /var/log/syslog")
    );
    assert!(messages.iter().any(|message| message == "boot log line"));
    assert!(messages.iter().any(|message| message == "tail warning"));

    let response = app.cancel_job(&created.id).await;
    assert!(response.status().is_success());
}

#[tokio::test]
async fn websocket_device_filter_only_streams_matching_device_events() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let script = make_fake_ssh(
        tempdir.path(),
        "case \"$*\" in *192.168.1.100*) printf 'alpha boot\n'; printf 'alpha warn\n' >&2 ;; *192.168.1.102*) printf 'mixed boot\n'; printf 'mixed warn\n' >&2 ;; esac\nsleep 5",
    );
    let app = spawn_app_with_config(multi_ssh_test_config(
        script.display().to_string(),
        tempdir.path().join("id_rsa").display().to_string(),
    ))
    .await;

    let mut ws = connect_ws(&app, "/api/ws?device_id=board-alpha").await;

    let alpha_job: Job = app
        .post_action(
            "board-alpha",
            "ssh.tail",
            Some(&ActionRequest {
                payload: Some("/var/log/alpha.log".into()),
            }),
        )
        .await
        .json()
        .await
        .expect("invalid alpha job json");

    let mixed_job: Job = app
        .post_action(
            "board-mixed",
            "ssh.tail",
            Some(&ActionRequest {
                payload: Some("/var/log/mixed.log".into()),
            }),
        )
        .await
        .json()
        .await
        .expect("invalid mixed job json");

    let mut messages = Vec::new();
    for _ in 0..8 {
        messages.push(recv_ws_event(&mut ws).await.message);
        if messages.iter().any(|message| message == "alpha boot")
            && messages.iter().any(|message| message == "alpha warn")
        {
            break;
        }
    }

    assert!(messages.iter().any(|message| message == "alpha boot"));
    assert!(messages.iter().any(|message| message == "alpha warn"));
    assert!(!messages.iter().any(|message| message == "mixed boot"));
    assert!(!messages.iter().any(|message| message == "mixed warn"));

    assert!(app.cancel_job(&alpha_job.id).await.status().is_success());
    assert!(app.cancel_job(&mixed_job.id).await.status().is_success());
}

#[tokio::test]
async fn websocket_since_replays_only_newer_job_events() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let script = make_fake_ssh(
        tempdir.path(),
        "printf 'line one\nline two\n'\nprintf 'warn one\n' >&2\nexit 0",
    );
    let app = spawn_app_with_config(ssh_test_config(
        script.display().to_string(),
        tempdir.path().join("id_rsa").display().to_string(),
    ))
    .await;

    let created: Job = app
        .post_action(
            "board-alpha",
            "ssh.exec",
            Some(&ActionRequest {
                payload: Some("echo replay".into()),
            }),
        )
        .await
        .json()
        .await
        .expect("invalid job json");

    let _job = app
        .wait_for_job_status(&created.id, JobStatus::Succeeded)
        .await;
    let events = wait_for_job_events(&app, &created.id, 6).await;

    let since = events
        .iter()
        .find(|event| event.message == "line one")
        .expect("missing line one event")
        .ts
        .to_rfc3339_opts(SecondsFormat::Millis, true);

    let mut ws = connect_ws(
        &app,
        &format!("/api/ws?job_id={}&since={since}", created.id),
    )
    .await;

    let mut messages = Vec::new();
    for _ in 0..4 {
        messages.push(recv_ws_event(&mut ws).await.message);
    }

    assert!(!messages.iter().any(|message| message == "job accepted"));
    assert!(!messages.iter().any(|message| message == "job running"));
    assert!(messages.iter().any(|message| message == "line one"));
    assert!(messages.iter().any(|message| message == "line two"));
    assert!(messages.iter().any(|message| message == "warn one"));
}

#[tokio::test]
async fn cancel_job_marks_running_job_as_cancelled() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let script = make_fake_ssh(tempdir.path(), "sleep 5");
    let app = spawn_app_with_config(ssh_test_config(
        script.display().to_string(),
        tempdir.path().join("id_rsa").display().to_string(),
    ))
    .await;

    let created: Job = app
        .post_action(
            "board-alpha",
            "ssh.exec",
            Some(&ActionRequest {
                payload: Some("sleep 5".into()),
            }),
        )
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
async fn start_action_returns_409_when_device_is_locked_by_running_job() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let script = make_fake_ssh(tempdir.path(), "sleep 5");
    let app = spawn_app_with_config(ssh_test_config(
        script.display().to_string(),
        tempdir.path().join("id_rsa").display().to_string(),
    ))
    .await;

    let holder: Job = app
        .post_action(
            "board-alpha",
            "ssh.exec",
            Some(&ActionRequest {
                payload: Some("sleep 5".into()),
            }),
        )
        .await
        .json()
        .await
        .expect("invalid holder job json");

    let _ = app
        .wait_for_job_status(&holder.id, JobStatus::Running)
        .await;

    let response = app.post_action("board-alpha", "ssh.reboot", None).await;
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);

    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(body["error"], "device_locked");
    assert_eq!(body["held_by_job_id"], holder.id);

    assert!(app.cancel_job(&holder.id).await.status().is_success());

    let restarted = app.post_action("board-alpha", "ssh.reboot", None).await;
    assert!(restarted.status().is_success());
    let reopened: Job = restarted.json().await.expect("invalid reopened job json");
    assert_eq!(reopened.action, "ssh.reboot");

    assert!(app.cancel_job(&reopened.id).await.status().is_success());
}

#[tokio::test]
async fn cancel_job_rejects_completed_jobs_without_mutating_status() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let script = make_fake_ssh(tempdir.path(), "exit 0");
    let app = spawn_app_with_config(ssh_test_config(
        script.display().to_string(),
        tempdir.path().join("id_rsa").display().to_string(),
    ))
    .await;

    let created: Job = app
        .post_action(
            "board-alpha",
            "ssh.exec",
            Some(&ActionRequest {
                payload: Some("true".into()),
            }),
        )
        .await
        .json()
        .await
        .expect("invalid job json");

    let finished = app
        .wait_for_job_status(&created.id, JobStatus::Succeeded)
        .await;
    assert_eq!(finished.status, JobStatus::Succeeded);

    let response = app.cancel_job(&created.id).await;
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);

    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(body["error"], "conflict: job is already succeeded");

    let fetched: Job = app
        .get_job(&created.id)
        .await
        .json()
        .await
        .expect("invalid fetched job json");
    assert_eq!(fetched.status, JobStatus::Succeeded);
}

#[tokio::test]
async fn cancel_job_returns_404_for_unknown_job() {
    let app = spawn_app().await;

    let response = app.cancel_job("job_missing").await;

    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(body["error"], "job not found");
}

#[tokio::test]
async fn completed_jobs_persist_job_metadata_and_full_event_history_to_disk() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let artifacts_dir = tempdir.path().join("artifacts");
    let script = make_fake_ssh(
        tempdir.path(),
        "i=1
while [ \"$i\" -le 1105 ]; do
  printf 'line-%s\\n' \"$i\"
  i=$((i + 1))
done
exit 0",
    );
    let mut config = ssh_test_config(
        script.display().to_string(),
        tempdir.path().join("id_rsa").display().to_string(),
    );
    config.artifacts_dir = Some(artifacts_dir.display().to_string());
    let app = spawn_app_with_config(config).await;

    let created: Job = app
        .post_action(
            "board-alpha",
            "ssh.exec",
            Some(&ActionRequest {
                payload: Some("generate logs".into()),
            }),
        )
        .await
        .json()
        .await
        .expect("invalid job json");

    let finished = app
        .wait_for_job_status(&created.id, JobStatus::Succeeded)
        .await;
    assert_eq!(finished.status, JobStatus::Succeeded);

    let job_dir = app
        .artifacts_dir
        .as_ref()
        .expect("artifacts dir")
        .join("jobs")
        .join(&created.id);
    assert!(job_dir.join("job.json").exists());
    assert!(job_dir.join("events.ndjson").exists());

    let persisted_job: Job = serde_json::from_slice(
        &fs::read(job_dir.join("job.json")).expect("read persisted job metadata"),
    )
    .expect("parse persisted job metadata");
    assert_eq!(persisted_job.status, JobStatus::Succeeded);

    let response = app.get_job_events(&created.id).await;
    assert!(response.status().is_success());
    let events: Vec<device_hub_shared::LogEvent> = response
        .json()
        .await
        .expect("invalid persisted job events json");

    assert!(events.len() > 1_000);
    assert!(events.iter().any(|event| event.message == "line-1"));
    assert!(events.iter().any(|event| event.message == "line-1105"));
}
