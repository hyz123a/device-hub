use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use device_hub_shared::{EventFilter, LogEvent};
use tokio::sync::broadcast;

use crate::storage::artifact_store::ArtifactStore;

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<LogEvent>,
    buffer: Arc<Mutex<VecDeque<LogEvent>>>,
    capacity: usize,
    artifacts: Option<ArtifactStore>,
}

impl EventBus {
    pub fn new(capacity: usize, artifacts: Option<ArtifactStore>) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
            artifacts,
        }
    }

    pub fn publish(&self, event: LogEvent) {
        if let Some(artifacts) = &self.artifacts {
            if let Err(error) = artifacts.append_event(&event) {
                eprintln!("failed to persist event for {}: {error}", event.job_id);
            }
        }
        {
            let mut buffer = self.buffer.lock().expect("event buffer poisoned");
            if buffer.len() == self.capacity {
                buffer.pop_front();
            }
            buffer.push_back(event.clone());
        }
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LogEvent> {
        self.sender.subscribe()
    }

    pub fn snapshot(&self, filter: &EventFilter) -> Vec<LogEvent> {
        let buffer = self.buffer.lock().expect("event buffer poisoned");
        buffer
            .iter()
            .filter(|event| filter.matches(event))
            .cloned()
            .collect()
    }

    pub fn clear_cache(&self) -> usize {
        let mut buffer = self.buffer.lock().expect("event buffer poisoned");
        let cleared = buffer.len();
        buffer.clear();
        cleared
    }
}
