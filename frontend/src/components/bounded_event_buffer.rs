use std::{
    collections::{HashSet, VecDeque},
    hash::{Hash, Hasher},
};

use device_hub_shared::{LogEvent, LogSource};

pub(crate) struct BoundedEventBuffer {
    entries: VecDeque<(u64, LogEvent)>,
    identities: HashSet<u64>,
    source_counts: [usize; 6],
    capacity: usize,
}

impl BoundedEventBuffer {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            identities: HashSet::with_capacity(capacity),
            source_counts: [0; 6],
            capacity,
        }
    }

    pub(crate) fn push(&mut self, event: LogEvent) -> bool {
        if self.capacity == 0 {
            return false;
        }

        let identity = event_identity_hash(&event);
        if !self.identities.insert(identity) {
            return false;
        }

        self.source_counts[source_index(&event.source)] += 1;
        self.entries.push_back((identity, event));
        self.evict_to_capacity();
        true
    }

    pub(crate) fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
        self.evict_to_capacity();
    }

    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.capacity
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.identities.clear();
        self.source_counts = [0; 6];
    }

    pub(crate) fn snapshot(&self) -> Vec<LogEvent> {
        self.entries
            .iter()
            .map(|(_, event)| event.clone())
            .collect()
    }

    pub(crate) fn available_device_sources(&self) -> Vec<LogSource> {
        [
            LogSource::Serial,
            LogSource::Ssh,
            LogSource::Jlink,
            LogSource::Adb,
        ]
        .into_iter()
        .filter(|source| self.source_counts[source_index(source)] > 0)
        .collect()
    }

    fn evict_to_capacity(&mut self) {
        while self.entries.len() > self.capacity {
            if let Some((identity, event)) = self.entries.pop_front() {
                self.identities.remove(&identity);
                self.source_counts[source_index(&event.source)] -= 1;
            }
        }
    }

    #[cfg(test)]
    fn dedup_len(&self) -> usize {
        self.identities.len()
    }
}

fn source_index(source: &LogSource) -> usize {
    match source {
        LogSource::Ssh => 0,
        LogSource::Serial => 1,
        LogSource::Jlink => 2,
        LogSource::Adb => 3,
        LogSource::System => 4,
        LogSource::Job => 5,
    }
}

pub(crate) fn event_identity_hash(event: &LogEvent) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    event.ts.hash(&mut hasher);
    event.device_id.hash(&mut hasher);
    event.job_id.hash(&mut hasher);
    event.message.hash(&mut hasher);
    std::mem::discriminant(&event.source).hash(&mut hasher);
    std::mem::discriminant(&event.channel).hash(&mut hasher);
    std::mem::discriminant(&event.level).hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use device_hub_shared::{LogChannel, LogEvent, LogLevel, LogSource};

    use super::BoundedEventBuffer;

    fn sample_event(index: i64, device_id: &str) -> LogEvent {
        LogEvent {
            ts: Utc
                .timestamp_opt(index, 0)
                .single()
                .expect("valid timestamp"),
            device_id: device_id.into(),
            job_id: format!("job-{device_id}"),
            source: LogSource::Serial,
            channel: LogChannel::Stdout,
            level: LogLevel::Info,
            message: format!("line-{index}"),
        }
    }

    #[test]
    fn bounded_event_buffer_evicts_oldest_events_at_capacity() {
        let mut buffer = BoundedEventBuffer::new(3);

        for index in 0..10 {
            assert!(buffer.push(sample_event(index, "camera")));
        }

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.len(), 3);
        assert_eq!(snapshot[0].message, "line-7");
        assert_eq!(snapshot[2].message, "line-9");
    }

    #[test]
    fn bounded_event_buffer_preserves_interleaved_device_order() {
        let mut buffer = BoundedEventBuffer::new(4);
        for (index, device_id) in ["camera", "homebase", "camera", "homebase", "camera"]
            .into_iter()
            .enumerate()
        {
            assert!(buffer.push(sample_event(index as i64, device_id)));
        }

        let snapshot = buffer.snapshot();
        let order = snapshot
            .iter()
            .map(|event| (event.device_id.as_str(), event.message.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            order,
            vec![
                ("homebase", "line-1"),
                ("camera", "line-2"),
                ("homebase", "line-3"),
                ("camera", "line-4"),
            ]
        );
    }

    #[test]
    fn bounded_event_buffer_evicts_dedup_identity_with_event() {
        let mut buffer = BoundedEventBuffer::new(2);
        let evicted = sample_event(1, "camera");

        assert!(buffer.push(evicted.clone()));
        assert!(buffer.push(sample_event(2, "camera")));
        assert!(buffer.push(sample_event(3, "camera")));
        assert_eq!(buffer.dedup_len(), 2);
        assert!(buffer.push(evicted));
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.dedup_len(), 2);
    }

    #[test]
    fn changing_buffer_cap_immediately_bounds_underlying_store() {
        let mut buffer = BoundedEventBuffer::new(5);
        for index in 0..5 {
            assert!(buffer.push(sample_event(index, "camera")));
        }

        buffer.set_capacity(2);

        assert_eq!(buffer.capacity(), 2);
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.dedup_len(), 2);
        assert_eq!(buffer.snapshot()[0].message, "line-3");
    }

    #[test]
    fn duplicate_detection_remains_correct_after_rollover() {
        let mut buffer = BoundedEventBuffer::new(3);
        for index in 0..12 {
            assert!(buffer.push(sample_event(index, "camera")));
        }

        let retained = sample_event(11, "camera");
        assert!(!buffer.push(retained));
        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.dedup_len(), 3);
    }

    #[test]
    fn clear_resets_events_and_dedup_state() {
        let mut buffer = BoundedEventBuffer::new(3);
        let event = sample_event(1, "camera");
        assert!(buffer.push(event.clone()));

        buffer.clear();

        assert!(buffer.is_empty());
        assert_eq!(buffer.dedup_len(), 0);
        assert!(buffer.push(event));
    }

    #[test]
    fn multi_device_rollover_keeps_event_and_dedup_memory_bounded() {
        let capacity = 1_024;
        let mut buffer = BoundedEventBuffer::new(capacity);
        let devices = ["camera-1", "camera-2", "homebase-1", "phone-1"];

        for index in 0..20_000 {
            assert!(buffer.push(sample_event(index, devices[index as usize % devices.len()],)));
        }

        assert_eq!(buffer.len(), capacity);
        assert_eq!(buffer.dedup_len(), capacity);
        assert_eq!(buffer.available_device_sources(), vec![LogSource::Serial]);
        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.first().expect("oldest").message, "line-18976");
        assert_eq!(snapshot.last().expect("newest").message, "line-19999");
    }

    #[test]
    #[ignore = "repeatable frontend log-buffer performance profile"]
    fn frontend_multi_device_log_buffer_performance_profile() {
        let capacity = 100_000;
        let input_events = 500_000;
        let simulated_batch_size = 128;
        let devices = ["camera-1", "camera-2", "homebase-1", "phone-1"];
        let mut buffer = BoundedEventBuffer::new(capacity);
        let started = std::time::Instant::now();
        let mut input_message_bytes = 0_usize;
        let mut max_batch = 0_usize;

        for batch_start in (0..input_events).step_by(simulated_batch_size) {
            let batch_end = (batch_start + simulated_batch_size).min(input_events);
            max_batch = max_batch.max(batch_end - batch_start);
            for index in batch_start..batch_end {
                let event = sample_event(index as i64, devices[index % devices.len()]);
                input_message_bytes += event.message.len();
                assert!(buffer.push(event));
            }
        }

        assert_eq!(buffer.len(), capacity);
        assert_eq!(buffer.dedup_len(), capacity);
        eprintln!(
            "input_events={input_events} input_message_bytes={input_message_bytes} buffer_capacity={} retained_events={} dedup_entries={} max_flush_batch={max_batch} elapsed_ms={}",
            buffer.capacity(),
            buffer.len(),
            buffer.dedup_len(),
            started.elapsed().as_millis(),
        );
    }
}
