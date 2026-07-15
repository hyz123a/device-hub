use std::{path::PathBuf, sync::Arc};

use device_hub_shared::{
    ActionRequest, Device, DiscoveredJlinkProbe, DiscoveredSerialPort, EventFilter, Job,
    JobArtifact, LogEvent, SerialExecRequest, SerialExecResult,
};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
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
pub enum SerialCommandError {
    #[error("{0}")]
    Failed(String),
}

pub trait SerialCommandExecutorPort: Send + Sync {
    fn exec(
        &self,
        device: Device,
        request: SerialExecRequest,
    ) -> BoxFuture<'static, Result<SerialExecResult, SerialCommandError>>;
}

pub type DynSerialCommandExecutor = Arc<dyn SerialCommandExecutorPort>;

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

#[derive(Debug, Error)]
#[error("job evidence storage failed: {0}")]
pub struct JobEvidenceStoreError(pub String);

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PersistedEventSummary {
    pub stdout_events: usize,
    pub stderr_events: usize,
    pub system_events: usize,
    pub outcome: Option<String>,
    pub last_error: Option<String>,
}

pub trait JobEvidenceStorePort: Send + Sync {
    fn read_events(&self, job_id: &str) -> Result<Option<Vec<LogEvent>>, JobEvidenceStoreError>;
    fn read_tail_events(
        &self,
        job_id: &str,
        tail: usize,
        filter: &EventFilter,
    ) -> Result<Option<Vec<LogEvent>>, JobEvidenceStoreError>;
    fn read_event_summary(
        &self,
        job_id: &str,
    ) -> Result<Option<PersistedEventSummary>, JobEvidenceStoreError>;
    fn list_job_artifacts(&self, job_id: &str) -> Result<Vec<JobArtifact>, JobEvidenceStoreError>;
    fn artifact_path(&self, job_id: &str, artifact_name: &str) -> Option<PathBuf>;
}

pub type DynJobEvidenceStore = Arc<dyn JobEvidenceStorePort>;

#[derive(Debug, Clone, Error)]
#[error("event persistence failed: {0}")]
pub struct EventPersistenceError(pub String);

pub trait EventPersistencePort: Send + Sync {
    fn append_event(&self, event: LogEvent) -> Result<(), EventPersistenceError>;
    fn flush_job(&self, job_id: &str) -> Result<(), EventPersistenceError>;
    fn shutdown(&self) -> Result<(), EventPersistenceError>;
}

pub type DynEventPersistence = Arc<dyn EventPersistencePort>;
