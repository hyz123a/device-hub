use std::{collections::HashMap, sync::Arc};

use device_hub_shared::Device;

#[derive(Clone)]
pub struct DeviceRegistry {
    devices: Arc<HashMap<String, Device>>,
}

impl DeviceRegistry {
    pub fn new(devices: Vec<Device>) -> Self {
        let devices = devices
            .into_iter()
            .map(|device| (device.id.clone(), device))
            .collect();
        Self {
            devices: Arc::new(devices),
        }
    }

    pub fn list(&self) -> Vec<Device> {
        let mut devices: Vec<_> = self.devices.values().cloned().collect();
        devices.sort_by(|left, right| left.id.cmp(&right.id));
        devices
    }

    pub fn get(&self, id: &str) -> Option<Device> {
        self.devices.get(id).cloned()
    }
}
