use device_hub_adapter_jlink::{JlinkAdapter, JlinkAdapterError};
use device_hub_shared::JlinkConfig;

fn config() -> JlinkConfig {
    JlinkConfig {
        serial: "123456789".into(),
        device: "STM32H743".into(),
        interface: "SWD".into(),
        speed_khz: 4000,
        autoconnect: true,
    }
}

fn fixture() -> std::path::PathBuf {
    env!("CARGO_BIN_EXE_jlink-process-fixture").into()
}

#[tokio::test]
async fn probe_returns_artifacts_and_structured_result() {
    let adapter = JlinkAdapter::with_program(fixture());

    let execution = adapter.probe(&config()).await.expect("probe result");

    assert!(execution.result.connected);
    assert_eq!(execution.result.serial, "123456789");
    assert!(
        execution
            .artifacts
            .iter()
            .any(|artifact| artifact.name == "jlink.command")
    );
    assert!(
        execution
            .artifacts
            .iter()
            .any(|artifact| artifact.name == "jlink.stdout.log")
    );
}

#[tokio::test]
async fn failing_command_keeps_stdout_artifacts() {
    let adapter = JlinkAdapter::with_program(fixture());

    let error = adapter
        .reset(&config())
        .await
        .expect_err("reset should fail");

    match error {
        JlinkAdapterError::CommandFailed {
            stdout,
            stderr,
            artifacts,
            ..
        } => {
            assert!(stdout.contains("attach failed"));
            assert_eq!(stderr, "dap error");
            assert!(
                artifacts
                    .iter()
                    .any(|artifact| artifact.name == "jlink.command")
            );
            assert!(
                artifacts
                    .iter()
                    .any(|artifact| artifact.name == "jlink.stdout.log")
            );
            assert!(
                artifacts
                    .iter()
                    .any(|artifact| artifact.name == "jlink.stderr.log")
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
