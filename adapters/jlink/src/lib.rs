use device_hub_shared::JlinkConfig;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlashRequest {
    pub firmware_path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeInfo {
    pub serial: String,
    pub device: String,
}

#[derive(Debug, Error)]
pub enum JlinkAdapterError {
    #[error("jlink adapter is not implemented yet")]
    NotImplemented,
}

#[derive(Clone, Debug)]
pub struct JlinkAdapter;

impl JlinkAdapter {
    pub fn new() -> Self {
        Self
    }

    pub async fn probe(&self, _config: &JlinkConfig) -> Result<ProbeInfo, JlinkAdapterError> {
        Err(JlinkAdapterError::NotImplemented)
    }

    pub async fn flash(
        &self,
        _config: &JlinkConfig,
        _request: &FlashRequest,
    ) -> Result<(), JlinkAdapterError> {
        Err(JlinkAdapterError::NotImplemented)
    }

    pub async fn verify(&self, _config: &JlinkConfig) -> Result<(), JlinkAdapterError> {
        Err(JlinkAdapterError::NotImplemented)
    }

    pub async fn reset(&self, _config: &JlinkConfig) -> Result<(), JlinkAdapterError> {
        Err(JlinkAdapterError::NotImplemented)
    }

    pub async fn erase(&self, _config: &JlinkConfig) -> Result<(), JlinkAdapterError> {
        Err(JlinkAdapterError::NotImplemented)
    }
}

impl Default for JlinkAdapter {
    fn default() -> Self {
        Self::new()
    }
}
