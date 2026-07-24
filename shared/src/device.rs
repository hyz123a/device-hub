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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_endpoint: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredSerialPort {
    pub path: String,
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vid: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_location: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<SerialDeviceSelector>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SerialDeviceSelector {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vid: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_location: Option<String>,
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

    #[test]
    fn serial_config_without_selector_remains_backward_compatible() {
        let config: SerialConfig =
            serde_json::from_str(r#"{"port":"COM25","baud":115200}"#).expect("serial config");

        assert_eq!(config.port, "COM25");
        assert_eq!(config.selector, None);
    }

    #[test]
    fn discovered_serial_port_omits_unknown_usb_metadata() {
        let port = DiscoveredSerialPort {
            path: "/dev/ttyS0".into(),
            label: None,
            ..Default::default()
        };

        assert_eq!(
            serde_json::to_value(port).expect("serialize port"),
            serde_json::json!({"path":"/dev/ttyS0","label":null})
        );
    }
}
