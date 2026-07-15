use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
};

use device_hub_shared::{LogChannel, LogEvent, LogLevel, LogSource};

use crate::ports::{EventPersistenceError, EventPersistencePort, PersistedEventSummary};

#[derive(Clone)]
pub struct BufferedEventWriter {
    sender: SyncSender<WriterCommand>,
    metrics: Arc<WriterMetrics>,
}

#[derive(Default)]
struct WriterMetrics {
    enqueued_events: AtomicUsize,
    persisted_events: AtomicUsize,
    outstanding_events: AtomicUsize,
    peak_outstanding_events: AtomicUsize,
    flushes: AtomicUsize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventWriterMetrics {
    pub enqueued_events: usize,
    pub persisted_events: usize,
    pub outstanding_events: usize,
    pub peak_outstanding_events: usize,
    pub flushes: usize,
}

enum WriterCommand {
    Append(LogEvent),
    Flush {
        job_id: String,
        reply: SyncSender<Result<(), String>>,
    },
    Shutdown {
        reply: SyncSender<Result<(), String>>,
    },
}

struct JobWriters {
    events: BufWriter<File>,
    raw_stream: Option<BufWriter<File>>,
    job_dir: PathBuf,
    summary: PersistedEventSummary,
}

impl BufferedEventWriter {
    pub fn start(root: impl Into<PathBuf>, queue_capacity: usize) -> io::Result<Self> {
        Self::start_with_gate(root.into(), queue_capacity, None)
    }

    fn start_with_gate(
        root: PathBuf,
        queue_capacity: usize,
        start_gate: Option<Receiver<()>>,
    ) -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(queue_capacity.max(1));
        let metrics = Arc::new(WriterMetrics::default());
        let worker_metrics = metrics.clone();
        thread::Builder::new()
            .name("device-hub-artifact-writer".into())
            .spawn(move || {
                if let Some(start_gate) = start_gate {
                    let _ = start_gate.recv();
                }
                writer_loop(root, receiver, worker_metrics);
            })?;
        Ok(Self { sender, metrics })
    }

    #[cfg(test)]
    fn start_paused(
        root: impl Into<PathBuf>,
        queue_capacity: usize,
    ) -> io::Result<(Self, SyncSender<()>)> {
        let (release, gate) = mpsc::sync_channel(0);
        let writer = Self::start_with_gate(root.into(), queue_capacity, Some(gate))?;
        Ok((writer, release))
    }

    fn request(
        &self,
        command: impl FnOnce(SyncSender<Result<(), String>>) -> WriterCommand,
    ) -> Result<(), EventPersistenceError> {
        let (reply, response) = mpsc::sync_channel(1);
        self.sender
            .send(command(reply))
            .map_err(|error| EventPersistenceError(error.to_string()))?;
        response
            .recv()
            .map_err(|error| EventPersistenceError(error.to_string()))?
            .map_err(EventPersistenceError)
    }

    pub fn metrics(&self) -> EventWriterMetrics {
        EventWriterMetrics {
            enqueued_events: self.metrics.enqueued_events.load(Ordering::Relaxed),
            persisted_events: self.metrics.persisted_events.load(Ordering::Relaxed),
            outstanding_events: self.metrics.outstanding_events.load(Ordering::Relaxed),
            peak_outstanding_events: self.metrics.peak_outstanding_events.load(Ordering::Relaxed),
            flushes: self.metrics.flushes.load(Ordering::Relaxed),
        }
    }
}

impl EventPersistencePort for BufferedEventWriter {
    fn append_event(&self, event: LogEvent) -> Result<(), EventPersistenceError> {
        let outstanding = self
            .metrics
            .outstanding_events
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        self.metrics.enqueued_events.fetch_add(1, Ordering::Relaxed);
        self.metrics
            .peak_outstanding_events
            .fetch_max(outstanding, Ordering::Relaxed);
        if let Err(error) = self.sender.send(WriterCommand::Append(event)) {
            self.metrics
                .outstanding_events
                .fetch_sub(1, Ordering::Relaxed);
            return Err(EventPersistenceError(error.to_string()));
        }
        Ok(())
    }

    fn flush_job(&self, job_id: &str) -> Result<(), EventPersistenceError> {
        self.request(|reply| WriterCommand::Flush {
            job_id: job_id.to_owned(),
            reply,
        })
    }

    fn shutdown(&self) -> Result<(), EventPersistenceError> {
        self.request(|reply| WriterCommand::Shutdown { reply })
    }
}

fn writer_loop(root: PathBuf, receiver: Receiver<WriterCommand>, metrics: Arc<WriterMetrics>) {
    let mut writers = HashMap::<String, JobWriters>::new();
    let mut errors = HashMap::<String, String>::new();

    while let Ok(command) = receiver.recv() {
        match command {
            WriterCommand::Append(event) => {
                if errors.contains_key(&event.job_id) {
                    metrics.outstanding_events.fetch_sub(1, Ordering::Relaxed);
                    continue;
                }
                let job_id = event.job_id.clone();
                if let Err(error) = append_event(&root, &mut writers, &event) {
                    writers.remove(&job_id);
                    errors.insert(job_id, error.to_string());
                } else {
                    metrics.persisted_events.fetch_add(1, Ordering::Relaxed);
                }
                metrics.outstanding_events.fetch_sub(1, Ordering::Relaxed);
            }
            WriterCommand::Flush { job_id, reply } => {
                metrics.flushes.fetch_add(1, Ordering::Relaxed);
                let result = errors
                    .get(&job_id)
                    .cloned()
                    .map_or_else(|| flush_job(&mut writers, &job_id), Err);
                if let Err(error) = &result {
                    errors.entry(job_id).or_insert_with(|| error.clone());
                }
                let _ = reply.send(result);
            }
            WriterCommand::Shutdown { reply } => {
                let mut result = errors.values().next().cloned().map_or(Ok(()), Err);
                for (job_id, writer) in &mut writers {
                    if let Err(error) = flush_writers(writer) {
                        let message = error.to_string();
                        errors.entry(job_id.clone()).or_insert(message.clone());
                        if result.is_ok() {
                            result = Err(message);
                        }
                    }
                }
                let _ = reply.send(result);
                break;
            }
        }
    }
}

fn append_event(
    root: &Path,
    writers: &mut HashMap<String, JobWriters>,
    event: &LogEvent,
) -> io::Result<()> {
    if !writers.contains_key(&event.job_id) {
        writers.insert(event.job_id.clone(), open_job_writers(root, &event.job_id)?);
    }
    let writer = writers
        .get_mut(&event.job_id)
        .expect("writer inserted for job");
    serde_json::to_writer(&mut writer.events, event).map_err(io::Error::other)?;
    writer.events.write_all(b"\n")?;

    if matches!(event.channel, LogChannel::Stdout | LogChannel::Stderr) {
        if writer.raw_stream.is_none() {
            writer.raw_stream = Some(BufWriter::new(open_append(
                &writer.job_dir.join("raw-stream.log"),
            )?));
        }
        let raw_stream = writer.raw_stream.as_mut().expect("raw writer initialized");
        raw_stream.write_all(event.message.as_bytes())?;
        raw_stream.write_all(b"\n")?;
    }
    record_event_summary(&mut writer.summary, event);
    Ok(())
}

fn record_event_summary(summary: &mut PersistedEventSummary, event: &LogEvent) {
    match event.channel {
        LogChannel::Stdout => summary.stdout_events += 1,
        LogChannel::Stderr => summary.stderr_events += 1,
        LogChannel::System => summary.system_events += 1,
    }
    if matches!(event.level, LogLevel::Error) {
        summary.last_error = Some(event.message.clone());
    }
    if matches!(event.level, LogLevel::Error) || event.source == LogSource::Job {
        summary.outcome = Some(event.message.clone());
    }
}

fn open_job_writers(root: &Path, job_id: &str) -> io::Result<JobWriters> {
    let job_dir = root.join("jobs").join(job_id);
    fs::create_dir_all(&job_dir)?;
    Ok(JobWriters {
        events: BufWriter::new(open_append(&job_dir.join("events.ndjson"))?),
        raw_stream: None,
        job_dir,
        summary: PersistedEventSummary::default(),
    })
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn flush_job(writers: &mut HashMap<String, JobWriters>, job_id: &str) -> Result<(), String> {
    writers
        .get_mut(job_id)
        .map(flush_writers)
        .transpose()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn flush_writers(writers: &mut JobWriters) -> io::Result<()> {
    writers.events.flush()?;
    if let Some(raw_stream) = &mut writers.raw_stream {
        raw_stream.flush()?;
    }
    let summary = serde_json::to_vec_pretty(&writers.summary).map_err(io::Error::other)?;
    fs::write(writers.job_dir.join("event-summary.json"), summary)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    use chrono::Utc;
    use device_hub_shared::{LogChannel, LogEvent, LogLevel, LogSource};

    use super::BufferedEventWriter;
    use crate::{ports::EventPersistencePort, storage::artifact_store::ArtifactStore};

    fn event(job_id: &str, index: usize, channel: LogChannel) -> LogEvent {
        LogEvent {
            ts: Utc::now(),
            device_id: "board-alpha".into(),
            job_id: job_id.into(),
            source: LogSource::Serial,
            channel,
            level: LogLevel::Info,
            message: format!("line-{index}"),
        }
    }

    #[test]
    fn artifact_writer_preserves_event_order_across_batches() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let writer = BufferedEventWriter::start(tempdir.path(), 4).expect("start writer");
        for index in 0..20 {
            writer
                .append_event(event("job-a", index, LogChannel::Stdout))
                .expect("enqueue event");
        }
        writer.flush_job("job-a").expect("flush job");

        let events = ArtifactStore::new(tempdir.path())
            .read_events("job-a")
            .expect("read events")
            .expect("persisted events");
        assert_eq!(
            events
                .iter()
                .map(|event| event.message.as_str())
                .collect::<Vec<_>>(),
            (0..20)
                .map(|index| format!("line-{index}"))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn artifact_writer_writes_raw_stream_only_for_stdout_and_stderr() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let writer = BufferedEventWriter::start(tempdir.path(), 4).expect("start writer");
        writer
            .append_event(event("job-a", 1, LogChannel::System))
            .expect("system event");
        writer
            .append_event(event("job-a", 2, LogChannel::Stdout))
            .expect("stdout event");
        writer
            .append_event(event("job-a", 3, LogChannel::Stderr))
            .expect("stderr event");
        writer.flush_job("job-a").expect("flush job");

        let raw = std::fs::read_to_string(tempdir.path().join("jobs/job-a/raw-stream.log"))
            .expect("raw stream");
        assert_eq!(raw, "line-2\nline-3\n");
    }

    #[test]
    fn artifact_writer_persists_bounded_event_summary() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let writer = BufferedEventWriter::start(tempdir.path(), 4).expect("start writer");
        writer
            .append_event(event("job-a", 1, LogChannel::Stdout))
            .expect("stdout event");
        let mut error = event("job-a", 2, LogChannel::Stderr);
        error.level = LogLevel::Error;
        writer.append_event(error).expect("error event");
        let mut completed = event("job-a", 3, LogChannel::System);
        completed.source = LogSource::Job;
        writer.append_event(completed).expect("completion event");
        writer.flush_job("job-a").expect("flush job");

        let summary = ArtifactStore::new(tempdir.path())
            .read_event_summary("job-a")
            .expect("read summary")
            .expect("persisted summary");
        assert_eq!(summary.stdout_events, 1);
        assert_eq!(summary.stderr_events, 1);
        assert_eq!(summary.system_events, 1);
        assert_eq!(summary.last_error.as_deref(), Some("line-2"));
        assert_eq!(summary.outcome.as_deref(), Some("line-3"));
    }

    #[test]
    fn artifact_writer_isolates_concurrent_jobs() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let writer = BufferedEventWriter::start(tempdir.path(), 2).expect("start writer");
        for index in 0..10 {
            for job_id in ["job-a", "job-b"] {
                writer
                    .append_event(event(job_id, index, LogChannel::Stdout))
                    .expect("enqueue event");
            }
        }
        writer.flush_job("job-a").expect("flush a");
        writer.flush_job("job-b").expect("flush b");

        let store = ArtifactStore::new(tempdir.path());
        for job_id in ["job-a", "job-b"] {
            let events = store
                .read_events(job_id)
                .expect("read events")
                .expect("persisted events");
            assert_eq!(events.len(), 10);
            assert!(events.iter().all(|event| event.job_id == job_id));
        }
    }

    #[test]
    fn artifact_writer_flushes_and_closes_on_shutdown() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let writer = BufferedEventWriter::start(tempdir.path(), 4).expect("start writer");
        writer
            .append_event(event("job-a", 1, LogChannel::Stdout))
            .expect("enqueue event");

        writer.shutdown().expect("shutdown writer");

        let events = ArtifactStore::new(tempdir.path())
            .read_events("job-a")
            .expect("read events")
            .expect("persisted events");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn artifact_writer_surfaces_open_write_and_flush_errors() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root_file = tempdir.path().join("not-a-directory");
        std::fs::write(&root_file, b"file").expect("create root file");
        let writer = BufferedEventWriter::start(&root_file, 1).expect("start writer");
        writer
            .append_event(event("job-a", 1, LogChannel::Stdout))
            .expect("queue accepts event");

        let error = writer
            .flush_job("job-a")
            .expect_err("flush must surface error");
        assert!(error.to_string().contains("event persistence failed"));
    }

    #[test]
    fn artifact_writer_applies_bounded_backpressure_without_dropping_events() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let (writer, release) =
            BufferedEventWriter::start_paused(tempdir.path(), 1).expect("start paused writer");
        writer
            .append_event(event("job-a", 1, LogChannel::Stdout))
            .expect("fill queue");

        let blocked_writer = writer.clone();
        let (completed, completion) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = blocked_writer.append_event(event("job-a", 2, LogChannel::Stdout));
            let _ = completed.send(result);
        });
        assert!(completion.recv_timeout(Duration::from_millis(50)).is_err());

        release.send(()).expect("release writer");
        completion
            .recv_timeout(Duration::from_secs(1))
            .expect("blocked append completes")
            .expect("append succeeds");
        writer.flush_job("job-a").expect("flush job");

        let events = ArtifactStore::new(tempdir.path())
            .read_events("job-a")
            .expect("read events")
            .expect("persisted events");
        assert_eq!(events.len(), 2);
    }

    #[test]
    #[ignore = "repeatable multi-device performance profile"]
    fn multi_device_log_pipeline_performance_profile() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let writer = BufferedEventWriter::start(tempdir.path(), 4_096).expect("start writer");
        let jobs = ["camera-1", "camera-2", "homebase-1", "phone-1"];
        let events_per_job = 25_000;
        let started = Instant::now();

        for index in 0..events_per_job {
            for job_id in jobs {
                writer
                    .append_event(event(job_id, index, LogChannel::Stdout))
                    .expect("enqueue event");
            }
        }
        for job_id in jobs {
            writer.flush_job(job_id).expect("flush job");
        }
        let write_elapsed = started.elapsed();
        let metrics = writer.metrics();
        let store = ArtifactStore::new(tempdir.path());
        let tail_started = Instant::now();
        let tail = store
            .read_tail_events(
                "camera-1",
                5_000,
                &device_hub_shared::EventFilter::default(),
            )
            .expect("tail read")
            .expect("tail events");
        let tail_elapsed = tail_started.elapsed();
        let artifact_bytes = jobs
            .iter()
            .map(|job_id| {
                std::fs::metadata(
                    tempdir
                        .path()
                        .join("jobs")
                        .join(job_id)
                        .join("events.ndjson"),
                )
                .expect("artifact metadata")
                .len()
            })
            .sum::<u64>();
        let rss_kib = current_rss_kib().unwrap_or_default();

        assert_eq!(metrics.enqueued_events, events_per_job * jobs.len());
        assert_eq!(metrics.persisted_events, metrics.enqueued_events);
        assert_eq!(metrics.outstanding_events, 0);
        assert_eq!(tail.len(), 5_000);
        eprintln!(
            "events={} artifact_bytes={} write_ms={} writer_peak_outstanding={} flushes={} tail_events={} tail_ms={} rss_kib={rss_kib}",
            metrics.enqueued_events,
            artifact_bytes,
            write_elapsed.as_millis(),
            metrics.peak_outstanding_events,
            metrics.flushes,
            tail.len(),
            tail_elapsed.as_millis(),
        );
    }

    fn current_rss_kib() -> Option<usize> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        status.lines().find_map(|line| {
            line.strip_prefix("VmRSS:")?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
    }
}
