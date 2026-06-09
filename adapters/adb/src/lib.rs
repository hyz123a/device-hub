use std::{path::PathBuf, process::Stdio};

use device_hub_shared::AdbConfig;
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
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
    stdout: BufReader<ChildStdout>,
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
            stdout: BufReader::new(stdout),
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
        let mut line = Vec::new();
        match self.stdout.read_until(b'\n', &mut line).await {
            Ok(0) => {
                let status = self.child.wait().await.map_err(AdbAdapterError::Read)?;
                if status.success() {
                    Ok(None)
                } else {
                    Err(AdbAdapterError::Exit(status))
                }
            }
            Ok(_) => {
                if line.ends_with(b"\n") {
                    line.pop();
                }
                if line.ends_with(b"\r") {
                    line.pop();
                }
                Ok(Some(String::from_utf8_lossy(&line).into_owned()))
            }
            Err(error) => Err(AdbAdapterError::Read(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn logcat_decodes_invalid_utf8_lossy_instead_of_failing() {
        use std::{fs, os::unix::fs::PermissionsExt};

        let tempdir = tempfile::tempdir().expect("tempdir");
        let adb = tempdir.path().join("fake-adb.sh");
        fs::write(
            &adb,
            "#!/bin/sh\nset -eu\nprintf 'valid\\n'\nprintf 'bad \\377\\n'\n",
        )
        .expect("write fake adb");
        let mut permissions = fs::metadata(&adb).expect("fake adb metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&adb, permissions).expect("chmod fake adb");

        let adapter = AdbAdapter::with_program(adb);
        let config = AdbConfig {
            serial: "device-1".into(),
            transport: None,
            default_logcat_args: vec!["-v".into(), "time".into()],
        };
        let mut session = adapter
            .logcat(&config, &config.default_logcat_args)
            .await
            .expect("logcat session");

        assert_eq!(
            session.next_line().await.expect("first line").as_deref(),
            Some("valid")
        );
        assert_eq!(
            session.next_line().await.expect("lossy line").as_deref(),
            Some("bad �")
        );
        assert_eq!(session.next_line().await.expect("eof"), None);
    }
}
