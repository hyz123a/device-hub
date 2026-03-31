use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{SystemTime, UNIX_EPOCH},
};

use device_hub_shared::JlinkConfig;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{fs, process::Command};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlashRequest {
    pub firmware_path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeResult {
    pub serial: String,
    pub device: String,
    pub connected: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlashResult {
    pub serial: String,
    pub device: String,
    pub firmware_path: String,
    pub flashed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerifyResult {
    pub serial: String,
    pub device: String,
    pub verified: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResetResult {
    pub serial: String,
    pub device: String,
    pub reset: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EraseResult {
    pub serial: String,
    pub device: String,
    pub erased: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct JlinkArtifact {
    pub name: String,
    pub contents: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct JlinkExecution<T> {
    pub result: T,
    pub artifacts: Vec<JlinkArtifact>,
}

#[derive(Debug, Error)]
pub enum JlinkAdapterError {
    #[error("failed to spawn `{program}`: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("jlink command failed with exit code {exit_code}: {stderr}")]
    CommandFailed {
        exit_code: i32,
        stdout: String,
        stderr: String,
        artifacts: Vec<JlinkArtifact>,
    },
    #[error("failed to write temporary jlink script: {0}")]
    ScriptIo(#[from] std::io::Error),
}

#[derive(Clone, Debug)]
pub struct JlinkAdapter {
    program: PathBuf,
}

impl JlinkAdapter {
    pub fn new() -> Self {
        Self {
            program: PathBuf::from("JLinkExe"),
        }
    }

    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    pub async fn probe(
        &self,
        config: &JlinkConfig,
    ) -> Result<JlinkExecution<ProbeResult>, JlinkAdapterError> {
        let output = self
            .run_script(config, &["ShowEmuList", "ShowHWStatus", "exit"])
            .await?;
        Ok(JlinkExecution {
            result: ProbeResult {
                serial: config.serial.clone(),
                device: config.device.clone(),
                connected: true,
            },
            artifacts: output.artifacts,
        })
    }

    pub async fn flash(
        &self,
        config: &JlinkConfig,
        request: &FlashRequest,
    ) -> Result<JlinkExecution<FlashResult>, JlinkAdapterError> {
        let firmware = normalize_firmware_path(&request.firmware_path);
        let output = self
            .run_script(
                config,
                &[
                    &format!("loadfile {}", firmware.display()),
                    "r",
                    "g",
                    "exit",
                ],
            )
            .await?;
        Ok(JlinkExecution {
            result: FlashResult {
                serial: config.serial.clone(),
                device: config.device.clone(),
                firmware_path: request.firmware_path.clone(),
                flashed: true,
            },
            artifacts: output.artifacts,
        })
    }

    pub async fn verify(
        &self,
        config: &JlinkConfig,
    ) -> Result<JlinkExecution<VerifyResult>, JlinkAdapterError> {
        let output = self.run_script(config, &["verifybin", "exit"]).await?;
        Ok(JlinkExecution {
            result: VerifyResult {
                serial: config.serial.clone(),
                device: config.device.clone(),
                verified: true,
            },
            artifacts: output.artifacts,
        })
    }

    pub async fn reset(
        &self,
        config: &JlinkConfig,
    ) -> Result<JlinkExecution<ResetResult>, JlinkAdapterError> {
        let output = self.run_script(config, &["r", "g", "exit"]).await?;
        Ok(JlinkExecution {
            result: ResetResult {
                serial: config.serial.clone(),
                device: config.device.clone(),
                reset: true,
            },
            artifacts: output.artifacts,
        })
    }

    pub async fn erase(
        &self,
        config: &JlinkConfig,
    ) -> Result<JlinkExecution<EraseResult>, JlinkAdapterError> {
        let output = self.run_script(config, &["erase", "exit"]).await?;
        Ok(JlinkExecution {
            result: EraseResult {
                serial: config.serial.clone(),
                device: config.device.clone(),
                erased: true,
            },
            artifacts: output.artifacts,
        })
    }

    async fn run_script(
        &self,
        config: &JlinkConfig,
        commands: &[&str],
    ) -> Result<CompletedExecution, JlinkAdapterError> {
        let script_path = temp_script_path();
        let command_script = format!("{}\n", commands.join("\n"));
        fs::write(&script_path, &command_script).await?;

        let output = Command::new(&self.program)
            .kill_on_drop(true)
            .arg("-Device")
            .arg(&config.device)
            .arg("-If")
            .arg(&config.interface)
            .arg("-Speed")
            .arg(config.speed_khz.to_string())
            .arg("-AutoConnect")
            .arg(if config.autoconnect { "1" } else { "0" })
            .arg("-SelectEmuBySN")
            .arg(&config.serial)
            .arg("-NoGui")
            .arg("1")
            .arg("-ExitOnError")
            .arg("1")
            .arg("-CommandFile")
            .arg(&script_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|source| JlinkAdapterError::Spawn {
                program: self.program.display().to_string(),
                source,
            })?;

        let _ = fs::remove_file(&script_path).await;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let artifacts = build_artifacts(&command_script, &stdout, &stderr);
        if !output.status.success() {
            return Err(JlinkAdapterError::CommandFailed {
                exit_code: output.status.code().unwrap_or(-1),
                stdout,
                stderr: stderr.trim().to_owned(),
                artifacts,
            });
        }

        Ok(CompletedExecution { artifacts })
    }
}

impl JlinkAdapterError {
    pub fn artifacts(&self) -> Option<&[JlinkArtifact]> {
        match self {
            Self::CommandFailed { artifacts, .. } => Some(artifacts),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
struct CompletedExecution {
    artifacts: Vec<JlinkArtifact>,
}

fn normalize_firmware_path(path: &str) -> PathBuf {
    Path::new(path.trim()).to_path_buf()
}

fn build_artifacts(command_script: &str, stdout: &str, stderr: &str) -> Vec<JlinkArtifact> {
    let mut artifacts = vec![JlinkArtifact {
        name: "jlink.command".into(),
        contents: command_script.as_bytes().to_vec(),
    }];
    if !stdout.is_empty() {
        artifacts.push(JlinkArtifact {
            name: "jlink.stdout.log".into(),
            contents: stdout.as_bytes().to_vec(),
        });
    }
    if !stderr.is_empty() {
        artifacts.push(JlinkArtifact {
            name: "jlink.stderr.log".into(),
            contents: stderr.as_bytes().to_vec(),
        });
    }
    artifacts
}

fn temp_script_path() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("device-hub-jlink-{nonce}.jlink"))
}

impl Default for JlinkAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt};

    use super::*;

    fn make_fake_jlink(dir: &Path, body: &str) -> PathBuf {
        let script_path = dir.join("fake-jlink.sh");
        fs::write(&script_path, format!("#!/bin/sh\nset -eu\n{body}\n")).expect("write fake jlink");
        let mut permissions = fs::metadata(&script_path)
            .expect("fake jlink metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).expect("chmod fake jlink");
        script_path
    }

    fn config() -> JlinkConfig {
        JlinkConfig {
            serial: "123456789".into(),
            device: "STM32H743".into(),
            interface: "SWD".into(),
            speed_khz: 4000,
            autoconnect: true,
        }
    }

    #[tokio::test]
    async fn probe_returns_artifacts_and_structured_result() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let program = make_fake_jlink(
            tempdir.path(),
            "script=\"\"\nwhile [ \"$#\" -gt 0 ]; do if [ \"$1\" = \"-CommandFile\" ]; then script=$2; shift 2; else shift; fi; done\ncat \"$script\"\nprintf 'probe ok\\n'",
        );
        let adapter = JlinkAdapter::with_program(program);

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
        let tempdir = tempfile::tempdir().expect("tempdir");
        let program = make_fake_jlink(
            tempdir.path(),
            "printf 'attach failed\\n'\nprintf 'dap error\\n' >&2\nexit 1",
        );
        let adapter = JlinkAdapter::with_program(program);

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
}
