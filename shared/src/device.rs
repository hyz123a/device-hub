use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub label: String,
    pub ssh: Option<SshConfig>,
    pub serial: Option<SerialConfig>,
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
    pub has_serial: bool,
    pub has_jlink: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SshConfig {
    pub host: String,
    pub user: String,
    pub key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SerialConfig {
    pub port: String,
    pub baud: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct JlinkConfig {
    pub serial: String,
    pub device: String,
    #[serde(default = "default_jlink_interface")]
    pub interface: String,
    #[serde(default = "default_jlink_speed_khz")]
    pub speed_khz: u32,
    #[serde(default = "default_jlink_autoconnect")]
    pub autoconnect: bool,
}

impl Device {
    pub fn detail(&self) -> DeviceDetail {
        DeviceDetail {
            device: self.clone(),
            capabilities: DeviceConnectionSummary {
                has_ssh: self.ssh.is_some(),
                has_serial: self.serial.is_some(),
                has_jlink: self.jlink.is_some(),
            },
        }
    }
}

fn default_jlink_interface() -> String {
    "SWD".into()
}

fn default_jlink_speed_khz() -> u32 {
    4000
}

fn default_jlink_autoconnect() -> bool {
    true
}
