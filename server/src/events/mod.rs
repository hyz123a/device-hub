use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use device_hub_shared::{EventFilter, LogEvent};
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<LogEvent>,
    buffer: Arc<Mutex<VecDeque<LogEvent>>>,
    capacity: usize,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
        }
    }

    pub fn publish(&self, event: LogEvent) {
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
}
