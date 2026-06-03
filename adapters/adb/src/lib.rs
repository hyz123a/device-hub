use std::{path::PathBuf, process::Stdio};

use device_hub_shared::AdbConfig;
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, BufReader, Lines},
    process::{Child, ChildStdout, Command},
};

#[derive(Clone, Debug)]
pub struct AdbAdapter {
    program: PathBuf,
}

#[derive(Debug, Error)]
pub enum AdbAdapterError {
    #[error("unsupported adb transport `{0}`")]
    UnsupportedTransport(String),
    #[error("failed to spawn adb: {0}")]
    Spawn(std::io::Error),
    #[error("adb logcat did not expose stdout")]
    MissingStdout,
    #[error("failed to read adb logcat output: {0}")]
    Read(std::io::Error),
    #[error("adb logcat exited with status {0}")]
    Exit(std::process::ExitStatus),
}

pub struct AdbLogcatSession {
    child: Child,
    lines: Lines<BufReader<ChildStdout>>,
}

impl AdbAdapter {
    pub fn new() -> Self {
        Self {
            program: "adb".into(),
        }
    }

    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    pub async fn logcat(
        &self,
        config: &AdbConfig,
        args: &[String],
    ) -> Result<AdbLogcatSession, AdbAdapterError> {
        let mut command = Command::new(&self.program);
        add_device_selector(&mut command, config)?;
        command
            .arg("logcat")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = command.spawn().map_err(AdbAdapterError::Spawn)?;
        let stdout = child.stdout.take().ok_or(AdbAdapterError::MissingStdout)?;

        Ok(AdbLogcatSession {
            child,
            lines: BufReader::new(stdout).lines(),
        })
    }
}

fn add_device_selector(command: &mut Command, config: &AdbConfig) -> Result<(), AdbAdapterError> {
    match config.transport.as_deref().unwrap_or("serial") {
        "serial" => {
            command.arg("-s").arg(&config.serial);
            Ok(())
        }
        "usb" => {
            command.arg("-d");
            Ok(())
        }
        "emulator" => {
            command.arg("-e");
            Ok(())
        }
        transport => Err(AdbAdapterError::UnsupportedTransport(transport.to_owned())),
    }
}

impl Default for AdbAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AdbLogcatSession {
    pub async fn next_line(&mut self) -> Result<Option<String>, AdbAdapterError> {
        match self.lines.next_line().await {
            Ok(Some(line)) => Ok(Some(line)),
            Ok(None) => {
                let status = self.child.wait().await.map_err(AdbAdapterError::Read)?;
                if status.success() {
                    Ok(None)
                } else {
                    Err(AdbAdapterError::Exit(status))
                }
            }
            Err(error) => Err(AdbAdapterError::Read(error)),
        }
    }
}
