use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub label: String,
    pub ssh: Option<SshConfig>,
    pub jlink: Option<JlinkConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceDetail {
    pub device: Device,
    pub capabilities: DeviceConnectionSummary,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceConnectionSummary {
    pub has_ssh: bool,
    pub has_jlink: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SshConfig {
    pub host: String,
    pub user: String,
    pub key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct JlinkConfig {
    pub serial: String,
    pub device: String,
}

impl Device {
    pub fn detail(&self) -> DeviceDetail {
        DeviceDetail {
            device: self.clone(),
            capabilities: DeviceConnectionSummary {
                has_ssh: self.ssh.is_some(),
                has_jlink: self.jlink.is_some(),
            },
        }
    }
}
