use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use device_hub_shared::Device;
use serde::{Deserialize, Serialize};

use crate::ports::{DeviceRegistryError, DeviceRegistryPort};

#[derive(Clone)]
pub struct DeviceRegistry {
    state: Arc<Mutex<DeviceRegistryState>>,
}

struct DeviceRegistryState {
    devices: HashMap<String, Device>,
    path: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedDeviceRegistry {
    devices: Vec<Device>,
}

impl DeviceRegistry {
    pub fn new(
        configured_devices: Vec<Device>,
        path: Option<PathBuf>,
    ) -> Result<Self, DeviceRegistryError> {
        let configured_devices = index_devices(configured_devices);
        let devices = match path.as_deref() {
            Some(path) if path.exists() => {
                let persisted_devices = read_registry(path)?;
                if persisted_devices.is_empty() && !configured_devices.is_empty() {
                    configured_devices.clone()
                } else {
                    persisted_devices
                }
            }
            _ => configured_devices,
        };
        let registry = Self {
            state: Arc::new(Mutex::new(DeviceRegistryState { devices, path })),
        };
        registry.persist()?;
        Ok(registry)
    }

    fn persist(&self) -> Result<(), DeviceRegistryError> {
        let state = self.state.lock().expect("device registry poisoned");
        persist_registry(&state)
    }
}

impl DeviceRegistryPort for DeviceRegistry {
    fn list(&self) -> Vec<Device> {
        let state = self.state.lock().expect("device registry poisoned");
        sorted_devices(&state.devices)
    }

    fn get(&self, id: &str) -> Option<Device> {
        self.state
            .lock()
            .expect("device registry poisoned")
            .devices
            .get(id)
            .cloned()
    }

    fn create(&self, device: Device) -> Result<Device, DeviceRegistryError> {
        let mut state = self.state.lock().expect("device registry poisoned");
        if state.devices.contains_key(&device.id) {
            return Err(DeviceRegistryError::AlreadyExists(device.id.clone()));
        }

        state.devices.insert(device.id.clone(), device.clone());
        persist_registry(&state)?;
        Ok(device)
    }

    fn update(&self, device: Device) -> Result<Device, DeviceRegistryError> {
        let mut state = self.state.lock().expect("device registry poisoned");
        if !state.devices.contains_key(&device.id) {
            return Err(DeviceRegistryError::NotFound);
        }

        state.devices.insert(device.id.clone(), device.clone());
        persist_registry(&state)?;
        Ok(device)
    }

    fn delete(&self, id: &str) -> Result<Device, DeviceRegistryError> {
        let mut state = self.state.lock().expect("device registry poisoned");
        let removed = state
            .devices
            .remove(id)
            .ok_or(DeviceRegistryError::NotFound)?;
        persist_registry(&state)?;
        Ok(removed)
    }
}

fn index_devices(devices: Vec<Device>) -> HashMap<String, Device> {
    devices
        .into_iter()
        .map(|device| (device.id.clone(), device))
        .collect()
}

fn sorted_devices(devices: &HashMap<String, Device>) -> Vec<Device> {
    let mut devices: Vec<_> = devices.values().cloned().collect();
    devices.sort_by(|left, right| left.id.cmp(&right.id));
    devices
}

fn read_registry(path: &Path) -> Result<HashMap<String, Device>, DeviceRegistryError> {
    let content = fs::read_to_string(path)
        .map_err(|error| DeviceRegistryError::Persistence(error.to_string()))?;
    let registry: PersistedDeviceRegistry = serde_yaml::from_str(&content)
        .map_err(|error| DeviceRegistryError::Persistence(error.to_string()))?;
    Ok(index_devices(registry.devices))
}

fn persist_registry(state: &DeviceRegistryState) -> Result<(), DeviceRegistryError> {
    let Some(path) = state.path.as_ref() else {
        return Ok(());
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| DeviceRegistryError::Persistence(error.to_string()))?;
    }
    let body = serde_yaml::to_string(&PersistedDeviceRegistry {
        devices: sorted_devices(&state.devices),
    })
    .map_err(|error| DeviceRegistryError::Persistence(error.to_string()))?;
    fs::write(path, body).map_err(|error| DeviceRegistryError::Persistence(error.to_string()))
}

#[cfg(test)]
mod tests {
    use device_hub_shared::{JlinkConfig, SerialConfig, SshConfig};

    use super::*;

    fn sample_device() -> Device {
        Device {
            id: "board-alpha".into(),
            label: "Linux Board".into(),
            ssh: Some(SshConfig {
                host: "192.168.1.100".into(),
                user: "root".into(),
                key: "~/.ssh/id_rsa".into(),
            }),
            serial: Some(SerialConfig {
                port: "/dev/ttyUSB0".into(),
                baud: 115200,
            }),
            jlink: Some(JlinkConfig {
                serial: "123456789".into(),
                device: "STM32H743".into(),
                interface: "SWD".into(),
                speed_khz: 4000,
                autoconnect: true,
            }),
        }
    }

    #[test]
    fn persists_changes_and_reloads_from_disk() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join("device-registry.yaml");
        let registry =
            DeviceRegistry::new(vec![sample_device()], Some(path.clone())).expect("registry");

        registry
            .create(Device {
                id: "board-beta".into(),
                label: "Probe".into(),
                ssh: None,
                serial: Some(SerialConfig {
                    port: "/dev/ttyUSB1".into(),
                    baud: 9600,
                }),
                jlink: None,
            })
            .expect("create device");

        let reloaded = DeviceRegistry::new(Vec::new(), Some(path)).expect("reload registry");
        let devices = reloaded.list();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, "board-alpha");
        assert_eq!(devices[1].id, "board-beta");
    }

    #[test]
    fn seeds_from_config_when_persisted_registry_is_empty() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join("device-registry.yaml");
        fs::write(&path, "devices: []\n").expect("write empty registry");

        let registry =
            DeviceRegistry::new(vec![sample_device()], Some(path.clone())).expect("registry");

        let devices = registry.list();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "board-alpha");

        let persisted = fs::read_to_string(path).expect("read registry");
        assert!(persisted.contains("board-alpha"));
    }
}
