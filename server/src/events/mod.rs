use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use device_hub_shared::{EventFilter, LogEvent};
use tokio::sync::broadcast;

use crate::ports::{DynEventPersistence, EventPersistenceError};

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<LogEvent>,
    buffer: Arc<Mutex<VecDeque<LogEvent>>>,
    capacity: usize,
    persistence: Option<DynEventPersistence>,
    persistence_error: Arc<Mutex<Option<EventPersistenceError>>>,
}

impl EventBus {
    pub fn new(capacity: usize, persistence: Option<DynEventPersistence>) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
            persistence,
            persistence_error: Arc::new(Mutex::new(None)),
        }
    }

    pub fn publish(&self, event: LogEvent) {
        if let Some(persistence) = &self.persistence
            && let Err(error) = persistence.append_event(event.clone())
        {
            let mut persistence_error = self
                .persistence_error
                .lock()
                .expect("event persistence error state poisoned");
            if persistence_error.is_none() {
                *persistence_error = Some(error);
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

    pub fn flush_job(&self, job_id: &str) -> Result<(), EventPersistenceError> {
        if let Some(error) = self
            .persistence_error
            .lock()
            .expect("event persistence error state poisoned")
            .clone()
        {
            return Err(error);
        }
        if let Some(persistence) = &self.persistence {
            persistence.flush_job(job_id)?;
        }
        Ok(())
    }

    pub fn shutdown_persistence(&self) -> Result<(), EventPersistenceError> {
        if let Some(persistence) = &self.persistence {
            persistence.shutdown()?;
        }
        Ok(())
    }
}
