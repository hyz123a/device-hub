use device_hub_adapter_ssh::{
    SshAdapter, SshAdapterError, SshCommand, SshLogChunk, SshStreamChannel,
};
use device_hub_shared::SshConfig;

fn config_with_key(key: impl Into<String>) -> SshConfig {
    SshConfig {
        host: "board.local".into(),
        user: "root".into(),
        key: key.into(),
        default_log_path: "/tmp/sdcard/debug_log".into(),
    }
}

fn fixture() -> std::path::PathBuf {
    env!("CARGO_BIN_EXE_ssh-process-fixture").into()
}

#[tokio::test]
async fn exec_runs_real_command_and_returns_output() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let key_path = tempdir.path().join("test_key");
    let adapter = SshAdapter::with_program(fixture());

    let result = adapter
        .exec(
            &config_with_key(key_path.display().to_string()),
            &SshCommand {
                command: "uname -a".into(),
            },
        )
        .await
        .expect("ssh result");

    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "remote stdout");
    assert_eq!(result.stderr, "remote stderr");
}

#[tokio::test]
async fn exec_returns_remote_exit_code_without_failing() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let adapter = SshAdapter::with_program(fixture());

    let result = adapter
        .exec(
            &config_with_key(tempdir.path().join("test_key").display().to_string()),
            &SshCommand {
                command: "false".into(),
            },
        )
        .await
        .expect("ssh result");

    assert_eq!(result.exit_code, 7);
    assert_eq!(result.stdout, "partial output");
    assert_eq!(result.stderr, "command failed");
}

#[tokio::test]
async fn connect_test_surfaces_remote_failures() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let adapter = SshAdapter::with_program(fixture());

    let error = adapter
        .connect_test(&config_with_key(
            tempdir.path().join("test_key").display().to_string(),
        ))
        .await
        .expect_err("connect should fail");
    assert!(matches!(
        error,
        SshAdapterError::CommandFailed { exit_code: 255, .. }
    ));
}

#[tokio::test]
async fn reboot_accepts_expected_disconnect() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let adapter = SshAdapter::with_program(fixture());

    adapter
        .reboot(&config_with_key(
            tempdir.path().join("test_key").display().to_string(),
        ))
        .await
        .expect("expected reboot disconnect");
}

#[tokio::test]
async fn tail_logs_stream_yields_stdout_and_stderr_lines() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let adapter = SshAdapter::with_program(fixture());

    let mut session = adapter
        .tail_logs_stream(
            &config_with_key(tempdir.path().join("test_key").display().to_string()),
            "/var/log/messages",
        )
        .await
        .expect("tail session");

    let mut chunks = Vec::new();
    while let Some(chunk) = session.next_chunk().await.expect("tail chunk") {
        chunks.push(chunk);
    }

    assert_eq!(chunks.len(), 3);
    assert!(chunks.contains(&SshLogChunk {
        channel: SshStreamChannel::Stdout,
        line: "line one".into(),
    }));
    assert!(chunks.contains(&SshLogChunk {
        channel: SshStreamChannel::Stdout,
        line: "line two".into(),
    }));
    assert!(chunks.contains(&SshLogChunk {
        channel: SshStreamChannel::Stderr,
        line: "warn one".into(),
    }));
}
