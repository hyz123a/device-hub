use std::{env, fs, path::Path};

use device_hub_shared::Device;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdapterEnablement {
    #[serde(default = "enabled_by_default")]
    pub serial: bool,
    #[serde(default = "enabled_by_default")]
    pub ssh: bool,
    #[serde(default = "enabled_by_default")]
    pub jlink: bool,
    #[serde(default = "enabled_by_default")]
    pub adb: bool,
    #[serde(default)]
    pub serial_reconnect: SerialReconnectConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SerialReconnectConfig {
    #[serde(default)]
    pub max_attempts: Option<u32>,
    #[serde(default = "default_reconnect_initial_delay_ms")]
    pub initial_delay_ms: u64,
    #[serde(default = "default_reconnect_max_delay_ms")]
    pub max_delay_ms: u64,
    #[serde(default = "enabled_by_default")]
    pub prevent_system_sleep: bool,
}

impl Default for SerialReconnectConfig {
    fn default() -> Self {
        Self {
            max_attempts: None,
            initial_delay_ms: default_reconnect_initial_delay_ms(),
            max_delay_ms: default_reconnect_max_delay_ms(),
            prevent_system_sleep: true,
        }
    }
}

fn default_reconnect_initial_delay_ms() -> u64 {
    100
}

fn default_reconnect_max_delay_ms() -> u64 {
    2_000
}

impl Default for AdapterEnablement {
    fn default() -> Self {
        Self {
            serial: true,
            ssh: true,
            jlink: true,
            adb: true,
            serial_reconnect: SerialReconnectConfig::default(),
        }
    }
}

fn enabled_by_default() -> bool {
    true
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub devices: Vec<Device>,
    #[serde(default)]
    pub device_registry_path: Option<String>,
    #[serde(default)]
    pub ssh_program: Option<String>,
    #[serde(default)]
    pub jlink_program: Option<String>,
    #[serde(default)]
    pub adb_program: Option<String>,
    #[serde(default)]
    pub artifacts_dir: Option<String>,
    #[serde(default)]
    pub adapters: AdapterEnablement,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse config: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

impl AppConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;
        let substituted = substitute_env_vars(&content);
        Ok(serde_yaml::from_str(&substituted)?)
    }
}

fn substitute_env_vars(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut idx = 0;

    while idx < chars.len() {
        if chars[idx] == '$' {
            idx += 1;
            let start = idx;
            while idx < chars.len() && (chars[idx].is_ascii_alphanumeric() || chars[idx] == '_') {
                idx += 1;
            }
            if start == idx {
                output.push('$');
                continue;
            }
            let name: String = chars[start..idx].iter().collect();
            if let Ok(value) = env::var(&name) {
                output.push_str(&value);
            } else {
                output.push('$');
                output.push_str(&name);
            }
        } else {
            output.push(chars[idx]);
            idx += 1;
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, substitute_env_vars};

    #[test]
    fn keeps_unknown_variable() {
        let value = substitute_env_vars("key: $UNKNOWN_DEVICE_HUB_VAR");
        assert_eq!(value, "key: $UNKNOWN_DEVICE_HUB_VAR");
    }

    #[test]
    fn adapter_enablement_defaults_to_enabled_for_compiled_feature_compatibility() {
        let config: AppConfig = serde_yaml::from_str("devices: []").expect("config");

        assert!(config.adapters.serial);
        assert!(config.adapters.ssh);
        assert!(config.adapters.jlink);
        assert!(config.adapters.adb);
        assert_eq!(config.adapters.serial_reconnect.max_attempts, None);
        assert!(config.adapters.serial_reconnect.prevent_system_sleep);
    }

    #[test]
    fn adapter_enablement_can_disable_individual_adapters() {
        let config: AppConfig = serde_yaml::from_str(
            "devices: []\nadapters:\n  serial: true\n  ssh: false\n  jlink: false\n  adb: false\n",
        )
        .expect("config");

        assert!(config.adapters.serial);
        assert!(!config.adapters.ssh);
        assert!(!config.adapters.jlink);
        assert!(!config.adapters.adb);
    }
}
