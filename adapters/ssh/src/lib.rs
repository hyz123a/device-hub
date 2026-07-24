use std::{env, path::PathBuf, process::Stdio};

use device_hub_shared::SshConfig;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::Command;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SshCommand {
    pub command: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SshCommandResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SshStreamChannel {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SshLogChunk {
    pub channel: SshStreamChannel,
    pub line: String,
}

#[derive(Debug, Error)]
pub enum SshAdapterError {
    #[error("failed to spawn `{program}`: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("ssh command failed with exit code {exit_code}: {stderr}")]
    CommandFailed { exit_code: i32, stderr: String },
    #[error("cannot expand key path `{path}` because HOME is not set")]
    MissingHomeDirectory { path: String },
    #[error("failed to read ssh stream: {0}")]
    Io(#[from] std::io::Error),
    #[error("ssh process did not provide a {stream} stream")]
    MissingStream { stream: &'static str },
}

pub struct SshTailSession {
    child: tokio::process::Child,
    stdout: Lines<BufReader<tokio::process::ChildStdout>>,
    stderr: Lines<BufReader<tokio::process::ChildStderr>>,
    stdout_done: bool,
    stderr_done: bool,
}

#[derive(Clone, Debug)]
pub struct SshAdapter {
    program: PathBuf,
}

impl SshAdapter {
    pub fn new() -> Self {
        Self {
            program: PathBuf::from(default_ssh_program()),
        }
    }

    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    pub async fn connect_test(&self, config: &SshConfig) -> Result<(), SshAdapterError> {
        self.run_status(config, "true").await?;
        Ok(())
    }

    pub async fn exec(
        &self,
        config: &SshConfig,
        command: &SshCommand,
    ) -> Result<SshCommandResult, SshAdapterError> {
        let output = self.capture_output(config, &command.command).await?;
        Ok(SshCommandResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    pub async fn reboot(&self, config: &SshConfig) -> Result<(), SshAdapterError> {
        match self
            .run_output(
                config,
                "sh -lc 'sudo -n reboot || sudo -n /sbin/reboot || reboot || /sbin/reboot'",
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(SshAdapterError::CommandFailed { exit_code, stderr })
                if reboot_disconnect_is_expected(exit_code, &stderr) =>
            {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    pub async fn tail_logs(&self, config: &SshConfig, path: &str) -> Result<(), SshAdapterError> {
        let quoted_path = shell_single_quote(path);
        self.run_status(config, &format!("tail -F -- {quoted_path}"))
            .await?;
        Ok(())
    }

    pub async fn tail_logs_stream(
        &self,
        config: &SshConfig,
        path: &str,
    ) -> Result<SshTailSession, SshAdapterError> {
        let quoted_path = shell_single_quote(path);
        let mut command = self.base_command(config)?;
        command
            .arg(format!("tail -F -- {quoted_path}"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|source| SshAdapterError::Spawn {
            program: self.program.display().to_string(),
            source,
        })?;

        let stdout = child
            .stdout
            .take()
            .ok_or(SshAdapterError::MissingStream { stream: "stdout" })?;
        let stderr = child
            .stderr
            .take()
            .ok_or(SshAdapterError::MissingStream { stream: "stderr" })?;

        Ok(SshTailSession {
            child,
            stdout: BufReader::new(stdout).lines(),
            stderr: BufReader::new(stderr).lines(),
            stdout_done: false,
            stderr_done: false,
        })
    }

    async fn capture_output(
        &self,
        config: &SshConfig,
        remote_command: &str,
    ) -> Result<std::process::Output, SshAdapterError> {
        let mut command = self.base_command(config)?;
        command.arg(remote_command);
        command
            .output()
            .await
            .map_err(|source| SshAdapterError::Spawn {
                program: self.program.display().to_string(),
                source,
            })
    }

    async fn run_output(
        &self,
        config: &SshConfig,
        remote_command: &str,
    ) -> Result<std::process::Output, SshAdapterError> {
        let output = self.capture_output(config, remote_command).await?;

        if output.status.success() {
            Ok(output)
        } else {
            Err(SshAdapterError::CommandFailed {
                exit_code: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }

    async fn run_status(
        &self,
        config: &SshConfig,
        remote_command: &str,
    ) -> Result<(), SshAdapterError> {
        let mut command = self.base_command(config)?;
        command
            .arg(remote_command)
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let output = command
            .output()
            .await
            .map_err(|source| SshAdapterError::Spawn {
                program: self.program.display().to_string(),
                source,
            })?;

        if output.status.success() {
            Ok(())
        } else {
            Err(SshAdapterError::CommandFailed {
                exit_code: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }

    fn base_command(&self, config: &SshConfig) -> Result<Command, SshAdapterError> {
        let mut command = Command::new(&self.program);
        command.kill_on_drop(true);
        command
            .arg("-i")
            .arg(expand_home(&config.key)?)
            .arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg("StrictHostKeyChecking=no")
            .arg("-o")
            .arg(format!("UserKnownHostsFile={}", null_device()))
            .arg("-o")
            .arg("LogLevel=ERROR")
            .arg("-o")
            .arg("ConnectTimeout=5")
            .arg(format!("{}@{}", config.user, config.host));
        Ok(command)
    }
}

impl Default for SshAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl SshTailSession {
    pub async fn next_chunk(&mut self) -> Result<Option<SshLogChunk>, SshAdapterError> {
        loop {
            if self.stdout_done && self.stderr_done {
                let status = self.child.wait().await?;
                return if status.success() {
                    Ok(None)
                } else {
                    Err(SshAdapterError::CommandFailed {
                        exit_code: status.code().unwrap_or(-1),
                        stderr: String::new(),
                    })
                };
            }

            tokio::select! {
                line = self.stdout.next_line(), if !self.stdout_done => {
                    match line? {
                        Some(line) => return Ok(Some(SshLogChunk { channel: SshStreamChannel::Stdout, line })),
                        None => self.stdout_done = true,
                    }
                }
                line = self.stderr.next_line(), if !self.stderr_done => {
                    match line? {
                        Some(line) => return Ok(Some(SshLogChunk { channel: SshStreamChannel::Stderr, line })),
                        None => self.stderr_done = true,
                    }
                }
            }
        }
    }
}

fn expand_home(path: &str) -> Result<PathBuf, SshAdapterError> {
    if let Some(suffix) = path.strip_prefix("~/") {
        let home = home_directory().ok_or_else(|| SshAdapterError::MissingHomeDirectory {
            path: path.to_owned(),
        })?;
        Ok(home.join(suffix))
    } else {
        Ok(PathBuf::from(path))
    }
}

#[cfg(windows)]
fn default_ssh_program() -> &'static str {
    "ssh.exe"
}

#[cfg(not(windows))]
fn default_ssh_program() -> &'static str {
    "ssh"
}

#[cfg(windows)]
fn null_device() -> &'static str {
    "NUL"
}

#[cfg(not(windows))]
fn null_device() -> &'static str {
    "/dev/null"
}

fn home_directory() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE")
            .or_else(|| env::var_os("HOME"))
            .map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn reboot_disconnect_is_expected(exit_code: i32, stderr: &str) -> bool {
    if exit_code != 255 {
        return false;
    }

    let stderr = stderr.to_ascii_lowercase();
    stderr.contains("connection reset")
        || stderr.contains("closed by remote host")
        || stderr.contains("broken pipe")
        || stderr.contains("connection to")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_defaults_match_the_platform() {
        #[cfg(windows)]
        {
            assert_eq!(default_ssh_program(), "ssh.exe");
            assert_eq!(null_device(), "NUL");
        }
        #[cfg(not(windows))]
        {
            assert_eq!(default_ssh_program(), "ssh");
            assert_eq!(null_device(), "/dev/null");
        }
    }

    #[test]
    fn base_command_uses_platform_host_options_without_spawning() {
        let adapter = SshAdapter::new();
        let command = adapter
            .base_command(&config_with_key("C:\\keys\\id_rsa"))
            .expect("ssh command");
        let arguments = command
            .as_std()
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(
            arguments
                .iter()
                .any(|argument| argument == &format!("UserKnownHostsFile={}", null_device()))
        );
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "root@board.local")
        );
    }

    fn config_with_key(key: impl Into<String>) -> SshConfig {
        SshConfig {
            host: "board.local".into(),
            user: "root".into(),
            key: key.into(),
            default_log_path: "/tmp/sdcard/debug_log".into(),
        }
    }
}
