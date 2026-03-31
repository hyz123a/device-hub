use std::{collections::VecDeque, fs, os::unix::fs::PermissionsExt, path::Path, sync::Arc};

use chrono::SecondsFormat;
use device_hub_adapter_serial::{SerialAdapter, SerialAdapterError};
use device_hub_server::{
    config::AppConfig,
    services::job_manager::{ActionPolicy, JobPolicyProvider},
};
use device_hub_shared::{
    ActionRequest, Device, JlinkConfig, Job, JobEvidence, JobStatus, SerialConfig, SshConfig,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_serial::SerialStream;

use crate::helpers::{
    connect_ws, recv_ws_event, spawn_app, spawn_app_with_config,
    spawn_app_with_config_and_job_policy, spawn_app_with_config_job_policy_and_serial_adapter,
    wait_for_job_events,
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

fn make_fake_jlink(dir: &Path, body: &str) -> std::path::PathBuf {
    let script_path = dir.join("fake-jlink.sh");
    fs::write(&script_path, format!("#!/bin/sh\nset -eu\n{body}\n")).expect("write fake jlink");
    let mut permissions = fs::metadata(&script_path)
        .expect("fake jlink metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script_path, permissions).expect("chmod fake jlink");
    script_path
}

fn ssh_test_config(ssh_program: String, key: String) -> AppConfig {
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
                    key,
                }),
                serial: None,
                jlink: None,
            },
        ],
        ssh_program: Some(ssh_program),
        jlink_program: None,
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
                serial: None,
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
                serial: None,
                jlink: Some(JlinkConfig {
                    serial: "987654321".into(),
                    device: "STM32H743".into(),
                    interface: "SWD".into(),
                    speed_khz: 4000,
                    autoconnect: true,
                }),
            },
        ],
        ssh_program: Some(ssh_program),
        jlink_program: None,
        artifacts_dir: None,
    }
}

fn jlink_test_config(jlink_program: String) -> AppConfig {
    AppConfig {
        devices: vec![Device {
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
        }],
        ssh_program: None,
        jlink_program: Some(jlink_program),
        artifacts_dir: None,
    }
}

fn serial_test_config() -> AppConfig {
    AppConfig {
        devices: vec![Device {
            id: "board-alpha".into(),
            label: "Linux Board".into(),
            ssh: None,
            serial: Some(SerialConfig {
                port: "/dev/test-serial".into(),
                baud: 115200,
            }),
            jlink: None,
        }],
        ssh_program: None,
        jlink_program: None,
        artifacts_dir: None,
    }
}

#[cfg(unix)]
async fn spawn_serial_test_app(pair_count: usize) -> (crate::helpers::TestApp, Vec<SerialStream>) {
    let mut masters = VecDeque::new();
    let mut slaves = Vec::with_capacity(pair_count);
    for _ in 0..pair_count {
        let (master, slave) = SerialStream::pair().expect("serial pair");
        masters.push_back(master);
        slaves.push(slave);
    }
    let masters = Arc::new(std::sync::Mutex::new(masters));
    let serial_adapter = SerialAdapter::with_stream_opener(4096, {
        let masters = masters.clone();
        move |_| {
            masters
                .lock()
                .expect("serial master registry poisoned")
                .pop_front()
                .ok_or_else(|| {
                    SerialAdapterError::Io(std::io::Error::other("serial test port already opened"))
                })
        }
    });

    let app = spawn_app_with_config_job_policy_and_serial_adapter(
        serial_test_config(),
        JobPolicyProvider::default(),
        serial_adapter,
    )
    .await;
    (app, slaves)
}

#[cfg(unix)]
async fn cancel_job_if_running(app: &crate::helpers::TestApp, job_id: &str) {
    let response = app.get_job(job_id).await;
    let job: Job = response.json().await.expect("invalid job json");
    if job.status == JobStatus::Running {
        let cancelled = app.cancel_job(job_id).await;
        assert!(cancelled.status().is_success());
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
async fn list_jobs_can_filter_by_device() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let script = make_fake_ssh(
        tempdir.path(),
        "case \"$*\" in *192.168.1.100*) sleep 5 ;; *192.168.1.102*) sleep 5 ;; esac",
    );
    let app = spawn_app_with_config(multi_ssh_test_config(
        script.display().to_string(),
        tempdir.path().join("id_rsa").display().to_string(),
    ))
    .await;

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

    let response = app
        .api_client
        .get(app.endpoint("/api/jobs?device_id=board-alpha"))
        .send()
        .await
        .expect("failed to execute request");

    assert!(response.status().is_success());
    let jobs: Vec<Job> = response.json().await.expect("invalid jobs json");
    assert!(jobs.iter().any(|job| job.id == alpha_job.id));
    assert!(!jobs.iter().any(|job| job.id == mixed_job.id));

    assert!(app.cancel_job(&alpha_job.id).await.status().is_success());
    assert!(app.cancel_job(&mixed_job.id).await.status().is_success());
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

#[cfg(unix)]
#[tokio::test]
async fn serial_tail_streams_device_output() {
    let (app, mut slaves) = spawn_serial_test_app(1).await;
    let mut slave = slaves.pop().expect("missing serial slave");

    let tail: Job = app
        .post_action("board-alpha", "serial.tail", None)
        .await
        .json()
        .await
        .expect("invalid serial tail job json");
    app.wait_for_job_status(&tail.id, JobStatus::Running).await;

    slave
        .write_all(b"boot ok\n")
        .await
        .expect("write serial output");

    let tail_events = wait_for_job_events(&app, &tail.id, 5).await;
    assert!(tail_events.iter().any(|event| event.message == "boot ok"));

    cancel_job_if_running(&app, &tail.id).await;
}

#[cfg(unix)]
#[tokio::test]
async fn serial_write_without_active_tail_still_succeeds() {
    let (app, _slaves) = spawn_serial_test_app(1).await;

    let write: Job = app
        .post_action(
            "board-alpha",
            "serial.write",
            Some(&ActionRequest {
                payload: Some("help\n".into()),
            }),
        )
        .await
        .json()
        .await
        .expect("invalid serial write job json");
    let finished_write = app
        .wait_for_job_status(&write.id, JobStatus::Succeeded)
        .await;
    assert_eq!(finished_write.action, "serial.write");

    let write_events = wait_for_job_events(&app, &write.id, 6).await;
    assert!(
        write_events
            .iter()
            .any(|event| event.message.contains("wrote 5 bytes"))
    );
}

#[cfg(unix)]
#[tokio::test]
async fn serial_write_succeeds_while_serial_tail_is_running() {
    let (app, mut slaves) = spawn_serial_test_app(1).await;
    let mut slave = slaves.pop().expect("missing serial slave");

    let tail: Job = app
        .post_action("board-alpha", "serial.tail", None)
        .await
        .json()
        .await
        .expect("invalid serial tail job json");
    let running_tail = app.wait_for_job_status(&tail.id, JobStatus::Running).await;
    assert_eq!(running_tail.action, "serial.tail");

    let device_task = tokio::spawn(async move {
        let mut command = [0_u8; 5];
        slave
            .read_exact(&mut command)
            .await
            .expect("read serial command");
        assert_eq!(&command, b"help\n");
        slave
            .write_all(b"ok\n")
            .await
            .expect("write serial response");
    });

    let write: Job = app
        .post_action(
            "board-alpha",
            "serial.write",
            Some(&ActionRequest {
                payload: Some("help\n".into()),
            }),
        )
        .await
        .json()
        .await
        .expect("invalid serial write job json");
    let finished_write = app
        .wait_for_job_status(&write.id, JobStatus::Succeeded)
        .await;
    assert_eq!(finished_write.action, "serial.write");

    device_task.await.expect("join device task");

    let write_events = wait_for_job_events(&app, &write.id, 6).await;
    assert!(
        write_events
            .iter()
            .any(|event| event.message.contains("wrote 5 bytes"))
    );

    let tail_events = wait_for_job_events(&app, &tail.id, 5).await;
    assert!(tail_events.iter().any(|event| event.message == "ok"));

    cancel_job_if_running(&app, &tail.id).await;
}

#[cfg(unix)]
#[tokio::test]
async fn serial_tail_can_be_cancelled_cleanly() {
    let (app, _slaves) = spawn_serial_test_app(1).await;

    let tail: Job = app
        .post_action("board-alpha", "serial.tail", None)
        .await
        .json()
        .await
        .expect("invalid serial tail job json");
    app.wait_for_job_status(&tail.id, JobStatus::Running).await;

    let cancelled = app.cancel_job(&tail.id).await;
    assert!(cancelled.status().is_success());

    let done = app
        .wait_for_job_status(&tail.id, JobStatus::Cancelled)
        .await;
    assert_eq!(done.action, "serial.tail");
}

#[cfg(unix)]
#[tokio::test]
async fn serial_tail_releases_session_after_cancel() {
    let (app, mut slaves) = spawn_serial_test_app(2).await;
    let mut second_slave = slaves.pop().expect("missing second serial slave");

    let first_tail: Job = app
        .post_action("board-alpha", "serial.tail", None)
        .await
        .json()
        .await
        .expect("invalid first serial tail job json");
    app.wait_for_job_status(&first_tail.id, JobStatus::Running)
        .await;
    let cancelled = app.cancel_job(&first_tail.id).await;
    assert!(cancelled.status().is_success());
    app.wait_for_job_status(&first_tail.id, JobStatus::Cancelled)
        .await;

    let second_tail: Job = app
        .post_action("board-alpha", "serial.tail", None)
        .await
        .json()
        .await
        .expect("invalid second serial tail job json");
    app.wait_for_job_status(&second_tail.id, JobStatus::Running)
        .await;

    second_slave
        .write_all(b"second session\n")
        .await
        .expect("write second serial output");

    let second_tail_events = wait_for_job_events(&app, &second_tail.id, 5).await;
    assert!(
        second_tail_events
            .iter()
            .any(|event| event.message == "second session")
    );

    cancel_job_if_running(&app, &second_tail.id).await;
}

#[tokio::test]
async fn jlink_flash_requires_a_firmware_path_payload() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let script = make_fake_jlink(tempdir.path(), "printf 'unused\\n'");
    let app = spawn_app_with_config(jlink_test_config(script.display().to_string())).await;

    let response = app.post_action("board-zeta", "jlink.flash", None).await;

    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json().await.expect("invalid error body");
    assert_eq!(
        body["error"],
        "bad request: action `jlink.flash` requires a firmware path payload"
    );
}

#[tokio::test]
async fn jlink_probe_persists_structured_result_and_artifacts() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let artifacts_dir = tempdir.path().join("artifacts");
    let script = make_fake_jlink(
        tempdir.path(),
        "script=\"\"\nwhile [ \"$#\" -gt 0 ]; do if [ \"$1\" = \"-CommandFile\" ]; then script=$2; shift 2; else shift; fi; done\nprintf 'probe header\\n'\ncat \"$script\"",
    );
    let mut config = jlink_test_config(script.display().to_string());
    config.artifacts_dir = Some(artifacts_dir.display().to_string());
    let app = spawn_app_with_config(config).await;

    let created: Job = app
        .post_action("board-zeta", "jlink.probe", None)
        .await
        .json()
        .await
        .expect("invalid job json");

    let job = app
        .wait_for_job_status(&created.id, JobStatus::Succeeded)
        .await;
    assert_eq!(job.action, "jlink.probe");

    let events = wait_for_job_events(&app, &created.id, 5).await;
    assert!(
        events
            .iter()
            .any(|event| event.message.contains("probing J-Link 123456789"))
    );
    assert!(events.iter().any(|event| event.message == "probe header"));

    let evidence: JobEvidence = app
        .get_job_evidence(&created.id)
        .await
        .json()
        .await
        .expect("invalid evidence json");
    assert!(
        evidence
            .artifacts
            .iter()
            .any(|artifact| artifact.name == "result.json")
    );
    assert!(
        evidence
            .artifacts
            .iter()
            .any(|artifact| artifact.name == "jlink.command")
    );
    assert!(
        evidence
            .artifacts
            .iter()
            .any(|artifact| artifact.name == "jlink.stdout.log")
    );

    let result_json = fs::read_to_string(
        app.artifacts_dir
            .as_ref()
            .expect("artifacts dir")
            .join("jobs")
            .join(&created.id)
            .join("result.json"),
    )
    .expect("read result.json");
    let result_value: serde_json::Value =
        serde_json::from_str(&result_json).expect("parse result json");
    assert_eq!(result_value["serial"], "123456789");
    assert_eq!(result_value["device"], "STM32H743");
    assert_eq!(result_value["connected"], true);
}

#[tokio::test]
async fn jlink_reset_failure_still_persists_jlink_logs() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let artifacts_dir = tempdir.path().join("artifacts");
    let script = make_fake_jlink(
        tempdir.path(),
        "printf 'reset start\\n'\nprintf 'dap failed\\n' >&2\nexit 1",
    );
    let mut config = jlink_test_config(script.display().to_string());
    config.artifacts_dir = Some(artifacts_dir.display().to_string());
    let app = spawn_app_with_config(config).await;

    let created: Job = app
        .post_action("board-zeta", "jlink.reset", None)
        .await
        .json()
        .await
        .expect("invalid job json");

    let job = app
        .wait_for_job_status(&created.id, JobStatus::Failed)
        .await;
    assert_eq!(job.action, "jlink.reset");

    let evidence: JobEvidence = app
        .get_job_evidence(&created.id)
        .await
        .json()
        .await
        .expect("invalid evidence json");
    assert!(
        evidence
            .artifacts
            .iter()
            .any(|artifact| artifact.name == "jlink.command")
    );
    assert!(
        evidence
            .artifacts
            .iter()
            .any(|artifact| artifact.name == "jlink.stdout.log")
    );
    assert!(
        evidence
            .artifacts
            .iter()
            .any(|artifact| artifact.name == "jlink.stderr.log")
    );

    let events = wait_for_job_events(&app, &created.id, 7).await;
    assert!(events.iter().any(|event| {
        event.source == device_hub_shared::LogSource::Jlink && event.message == "reset start"
    }));
    assert!(events.iter().any(|event| {
        event.source == device_hub_shared::LogSource::Jlink && event.message == "dap failed"
    }));
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

#[tokio::test]
async fn get_job_evidence_returns_summary_and_downloadable_artifacts() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let artifacts_dir = tempdir.path().join("artifacts");
    let script = make_fake_ssh(
        tempdir.path(),
        "printf 'boot ok\\n'\nprintf 'crc mismatch\\n' >&2\nexit 7",
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
                payload: Some("check-firmware".into()),
            }),
        )
        .await
        .json()
        .await
        .expect("invalid job json");

    let finished = app
        .wait_for_job_status(&created.id, JobStatus::Failed)
        .await;
    assert_eq!(finished.payload.as_deref(), Some("check-firmware"));
    assert_eq!(finished.timeout_secs, 30);

    let response = app.get_job_evidence(&created.id).await;
    assert!(response.status().is_success());
    let evidence: JobEvidence = response.json().await.expect("invalid evidence json");
    assert!(evidence.summary.headline.contains("ssh.exec failed"));
    assert_eq!(
        evidence.summary.request.as_deref(),
        Some("command: check-firmware")
    );
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

    let download = app.download_job_artifact(&created.id, "job.json").await;
    assert!(download.status().is_success());
    let content_disposition = download
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .expect("content disposition");
    assert!(
        content_disposition
            .to_str()
            .expect("header string")
            .contains("job.json")
    );
}

#[tokio::test]
async fn rerun_job_reuses_previous_action_payload_and_device() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let script = make_fake_ssh(tempdir.path(), "printf 'rerun ready\\n'\nexit 0");
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
                payload: Some("echo rerun".into()),
            }),
        )
        .await
        .json()
        .await
        .expect("invalid job json");
    app.wait_for_job_status(&created.id, JobStatus::Succeeded)
        .await;

    let rerun_response = app.rerun_job(&created.id).await;
    assert!(rerun_response.status().is_success());
    let rerun: Job = rerun_response.json().await.expect("invalid rerun job json");
    assert_ne!(rerun.id, created.id);
    assert_eq!(rerun.device_id, created.device_id);
    assert_eq!(rerun.action, created.action);
    assert_eq!(rerun.payload, created.payload);

    let finished = app
        .wait_for_job_status(&rerun.id, JobStatus::Succeeded)
        .await;
    assert_eq!(finished.payload.as_deref(), Some("echo rerun"));
}

#[tokio::test]
async fn ssh_exec_times_out_and_records_timeout_policy_events() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let script = make_fake_ssh(tempdir.path(), "sleep 1");
    let policy_provider = JobPolicyProvider::new(|action| match action {
        "ssh.exec" => ActionPolicy::new(
            std::time::Duration::from_millis(150),
            "teardown: drop the SSH process, release the device lock, and persist collected evidence",
        ),
        "ssh.reboot" => ActionPolicy::new(
            std::time::Duration::from_secs(20),
            "teardown: treat the reboot request as fire-and-forget, release the device lock, and rely on reconnect for the next action",
        ),
        "ssh.tail" => ActionPolicy::new(
            std::time::Duration::from_secs(3600),
            "teardown: stop the SSH tail process, release stream resources, and keep captured logs on disk",
        ),
        "serial.tail" => ActionPolicy::new(
            std::time::Duration::from_secs(3600),
            "teardown: close the serial stream handle and keep captured logs on disk",
        ),
        _ => ActionPolicy::new(
            std::time::Duration::from_secs(60),
            "teardown: stop the active adapter task and persist the final job state",
        ),
    });
    let app = spawn_app_with_config_and_job_policy(
        ssh_test_config(
            script.display().to_string(),
            tempdir.path().join("id_rsa").display().to_string(),
        ),
        policy_provider,
    )
    .await;

    let created: Job = app
        .post_action(
            "board-alpha",
            "ssh.exec",
            Some(&ActionRequest {
                payload: Some("long operation".into()),
            }),
        )
        .await
        .json()
        .await
        .expect("invalid job json");

    let failed = loop {
        let response = app.get_job(&created.id).await;
        let job: Job = response.json().await.expect("invalid job json");
        if job.status == JobStatus::Failed {
            break job;
        }

        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    };
    assert_eq!(failed.timeout_secs, 0);

    let events = wait_for_job_events(&app, &created.id, 5).await;
    assert!(
        events
            .iter()
            .any(|event| event.message.contains("timeout policy: 0s"))
    );
    assert!(
        events
            .iter()
            .any(|event| event.message.contains("action timed out after 0s"))
    );
    assert!(events.iter().any(|event| {
        event
            .message
            .contains("drop the SSH process, release the device lock")
    }));
}
