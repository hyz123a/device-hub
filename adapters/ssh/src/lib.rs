use device_hub_shared::SshConfig;
use serde::{Deserialize, Serialize};
use thiserror::Error;

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

#[derive(Debug, Error)]
pub enum SshAdapterError {
    #[error("ssh adapter is not implemented yet")]
    NotImplemented,
}

#[derive(Clone, Debug)]
pub struct SshAdapter;

impl SshAdapter {
    pub fn new() -> Self {
        Self
    }

    pub async fn connect_test(&self, _config: &SshConfig) -> Result<(), SshAdapterError> {
        Err(SshAdapterError::NotImplemented)
    }

    pub async fn exec(
        &self,
        _config: &SshConfig,
        _command: &SshCommand,
    ) -> Result<SshCommandResult, SshAdapterError> {
        Err(SshAdapterError::NotImplemented)
    }

    pub async fn reboot(&self, _config: &SshConfig) -> Result<(), SshAdapterError> {
        Err(SshAdapterError::NotImplemented)
    }

    pub async fn tail_logs(&self, _config: &SshConfig, _path: &str) -> Result<(), SshAdapterError> {
        Err(SshAdapterError::NotImplemented)
    }
}

impl Default for SshAdapter {
    fn default() -> Self {
        Self::new()
    }
}
