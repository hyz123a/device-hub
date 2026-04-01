use std::sync::Arc;

use device_hub_shared::{ActionRequest, Device, Job};
use futures::future::BoxFuture;

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
