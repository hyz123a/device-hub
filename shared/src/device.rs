use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub label: String,
    pub ssh: Option<SshConfig>,
    pub serial: Option<SerialConfig>,
    pub jlink: Option<JlinkConfig>,
    pub adb: Option<AdbConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceDetail {
    pub device: Device,
    pub capabilities: DeviceConnectionSummary,
    pub runtime: DeviceRuntimeStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceConnectionSummary {
    pub has_ssh: bool,
    pub has_serial: bool,
    pub has_jlink: bool,
    pub has_adb: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceRuntimeStatus {
    pub active_tails: Vec<ActiveTailStatus>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveTailStatus {
    pub action: String,
    pub job_id: String,
    pub active_since: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredSerialPort {
    pub path: String,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredJlinkProbe {
    pub serial: String,
    pub product: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SshConfig {
    pub host: String,
    pub user: String,
    pub key: String,
    #[serde(default = "default_ssh_log_path")]
    pub default_log_path: String,
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdbConfig {
    pub serial: String,
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default = "default_adb_logcat_args")]
    pub default_logcat_args: Vec<String>,
}

impl Device {
    pub fn detail(&self) -> DeviceDetail {
        self.detail_with_runtime(DeviceRuntimeStatus::default())
    }

    pub fn detail_with_runtime(&self, runtime: DeviceRuntimeStatus) -> DeviceDetail {
        DeviceDetail {
            device: self.clone(),
            capabilities: DeviceConnectionSummary {
                has_ssh: self.ssh.is_some(),
                has_serial: self.serial.is_some(),
                has_jlink: self.jlink.is_some(),
                has_adb: self.adb.is_some(),
            },
            runtime,
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

pub fn default_ssh_log_path() -> String {
    "/tmp/sdcard/debug_log".into()
}

pub fn default_adb_logcat_args() -> Vec<String> {
    vec!["-v".into(), "time".into()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_config_defaults_log_path_when_missing_from_payload() {
        let config: SshConfig = serde_json::from_str(
            r#"{
                "host":"192.168.1.100",
                "user":"root",
                "key":"~/.ssh/id_rsa"
            }"#,
        )
        .expect("ssh config json");

        assert_eq!(config.default_log_path, "/tmp/sdcard/debug_log");
    }
}
