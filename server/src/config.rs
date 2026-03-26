use std::{env, fs, path::Path};

use device_hub_shared::Device;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub devices: Vec<Device>,
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
    use super::substitute_env_vars;

    #[test]
    fn keeps_unknown_variable() {
        let value = substitute_env_vars("key: $UNKNOWN_DEVICE_HUB_VAR");
        assert_eq!(value, "key: $UNKNOWN_DEVICE_HUB_VAR");
    }
}
