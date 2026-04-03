use std::sync::Arc;

use device_hub_shared::{ActionRequest, Device, DiscoveredJlinkProbe, DiscoveredSerialPort, Job};
use futures::future::BoxFuture;
use thiserror::Error;

use crate::{
    events::EventBus,
    storage::{artifact_store::ArtifactStore, job_store::JobStore},
};

#[derive(Clone)]
pub struct ActionExecutionContext {
    pub jobs: JobStore,
    pub events: EventBus,
    pub artifacts: ArtifactStore,
}

impl ActionExecutionContext {
    pub fn new(jobs: JobStore, events: EventBus, artifacts: ArtifactStore) -> Self {
        Self {
            jobs,
            events,
            artifacts,
        }
    }
}

pub trait ActionRunner: Send + Sync {
    fn run(
        &self,
        context: ActionExecutionContext,
        job: Job,
        device: Device,
        action: String,
        payload: Option<ActionRequest>,
    ) -> BoxFuture<'static, ()>;
}

pub type DynActionRunner = Arc<dyn ActionRunner>;

#[derive(Debug, Error)]
pub enum DeviceRegistryError {
    #[error("device not found")]
    NotFound,
    #[error("device `{0}` already exists")]
    AlreadyExists(String),
    #[error("device registry persistence failed: {0}")]
    Persistence(String),
}

pub trait DeviceRegistryPort: Send + Sync {
    fn list(&self) -> Vec<Device>;
    fn get(&self, id: &str) -> Option<Device>;
    fn create(&self, device: Device) -> Result<Device, DeviceRegistryError>;
    fn update(&self, device: Device) -> Result<Device, DeviceRegistryError>;
    fn delete(&self, id: &str) -> Result<Device, DeviceRegistryError>;
}

pub type DynDeviceRegistry = Arc<dyn DeviceRegistryPort>;

#[derive(Debug, Error)]
pub enum HostDiscoveryError {
    #[error("failed to enumerate serial ports: {0}")]
    Serial(String),
    #[error("failed to scan J-Link probes: {0}")]
    Jlink(String),
}

pub trait HostDiscoveryPort: Send + Sync {
    fn list_serial_ports(
        &self,
    ) -> BoxFuture<'static, Result<Vec<DiscoveredSerialPort>, HostDiscoveryError>>;
    fn list_jlink_probes(
        &self,
    ) -> BoxFuture<'static, Result<Vec<DiscoveredJlinkProbe>, HostDiscoveryError>>;
}

pub type DynHostDiscovery = Arc<dyn HostDiscoveryPort>;
