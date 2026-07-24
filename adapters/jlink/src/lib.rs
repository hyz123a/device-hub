use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Stdio,
    time::{SystemTime, UNIX_EPOCH},
};

use device_hub_shared::{DiscoveredJlinkProbe, JlinkConfig};
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
            program: PathBuf::from(default_jlink_program()),
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

    pub async fn scan_probes(&self) -> Result<Vec<DiscoveredJlinkProbe>, JlinkAdapterError> {
        let output = self.run_scan_script(&["ShowEmuList", "exit"]).await?;
        Ok(parse_scanned_probes(&output.stdout))
    }

    pub async fn flash(
        &self,
        config: &JlinkConfig,
        request: &FlashRequest,
    ) -> Result<JlinkExecution<FlashResult>, JlinkAdapterError> {
        let firmware = normalize_firmware_path(&request.firmware_path);
        let firmware_command_path = quote_jlink_path(&firmware);
        let output = self
            .run_script(
                config,
                &[
                    &format!("loadfile {firmware_command_path}"),
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

        Ok(CompletedExecution { stdout, artifacts })
    }

    async fn run_scan_script(
        &self,
        commands: &[&str],
    ) -> Result<CompletedExecution, JlinkAdapterError> {
        let script_path = temp_script_path();
        let command_script = format!("{}\n", commands.join("\n"));
        fs::write(&script_path, &command_script).await?;

        let output = Command::new(&self.program)
            .kill_on_drop(true)
            .arg("-NoGui")
            .arg("1")
            .arg("-AutoConnect")
            .arg("0")
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

        Ok(CompletedExecution { stdout, artifacts })
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
    stdout: String,
    artifacts: Vec<JlinkArtifact>,
}

fn parse_scanned_probes(stdout: &str) -> Vec<DiscoveredJlinkProbe> {
    let mut seen = HashSet::new();
    let mut probes = Vec::new();

    for line in stdout.lines() {
        let Some(serial) = extract_scan_field(line, &["Serial number", "Serial Number", "SN"])
        else {
            continue;
        };
        if !seen.insert(serial.clone()) {
            continue;
        }
        probes.push(DiscoveredJlinkProbe {
            serial,
            product: extract_scan_field(line, &["ProductName", "Product"]),
        });
    }

    probes.sort_by(|left, right| left.serial.cmp(&right.serial));
    probes
}

fn extract_scan_field(line: &str, labels: &[&str]) -> Option<String> {
    for label in labels {
        for delimiter in [":", "="] {
            let marker = format!("{label}{delimiter}");
            let Some((_, value)) = line.split_once(&marker) else {
                continue;
            };
            let value = value.split([',', ';']).next().unwrap_or_default().trim();
            if value.is_empty() {
                continue;
            }
            let normalized = if *label == "SN" || label.starts_with("Serial") {
                value
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_owned()
            } else {
                value.to_owned()
            };
            if !normalized.is_empty() {
                return Some(normalized);
            }
        }
    }

    None
}

fn normalize_firmware_path(path: &str) -> PathBuf {
    Path::new(path.trim()).to_path_buf()
}

fn quote_jlink_path(path: &Path) -> String {
    format!("\"{}\"", path.display().to_string().replace('"', "\\\""))
}

#[cfg(windows)]
fn default_jlink_program() -> &'static str {
    "JLink.exe"
}

#[cfg(not(windows))]
fn default_jlink_program() -> &'static str {
    "JLinkExe"
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
    use super::*;

    #[test]
    fn parses_scanned_probe_output() {
        let probes = parse_scanned_probes(
            "J-Link[0]: ProductName: J-Link EDU Mini, Serial number: 123456789\nJ-Link[1]: SN: 987654321, ProductName: J-Link PLUS\n",
        );

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

    #[test]
    fn firmware_paths_are_quoted_for_jlink_command_files() {
        assert_eq!(
            quote_jlink_path(Path::new("C:\\Program Files\\firmware image.bin")),
            "\"C:\\Program Files\\firmware image.bin\""
        );
    }

    #[test]
    fn default_program_matches_the_host_platform() {
        #[cfg(windows)]
        assert_eq!(default_jlink_program(), "JLink.exe");
        #[cfg(not(windows))]
        assert_eq!(default_jlink_program(), "JLinkExe");
    }
}
