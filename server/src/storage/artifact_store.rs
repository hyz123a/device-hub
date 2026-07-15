use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use device_hub_shared::{EventFilter, Job, JobArtifact, LogChannel, LogEvent};

use crate::ports::{JobEvidenceStoreError, JobEvidenceStorePort, PersistedEventSummary};

const TAIL_READ_BLOCK_BYTES: usize = 64 * 1024;
const MAX_TAIL_READ_BYTES: usize = 64 * 1024 * 1024;
const MAX_PERSISTED_EVENT_LINE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn init_job(&self, job: &Job) -> io::Result<()> {
        fs::create_dir_all(self.job_dir(&job.id))?;
        self.write_job(job)
    }

    pub fn update_job(&self, job: &Job) -> io::Result<()> {
        self.write_job(job)
    }

    pub fn append_event(&self, event: &LogEvent) -> io::Result<()> {
        let job_dir = self.job_dir(&event.job_id);
        fs::create_dir_all(&job_dir)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(job_dir.join("events.ndjson"))?;
        serde_json::to_writer(&mut file, event).map_err(io::Error::other)?;
        file.write_all(b"\n")?;

        if matches!(event.channel, LogChannel::Stdout | LogChannel::Stderr) {
            let mut raw_stream = OpenOptions::new()
                .create(true)
                .append(true)
                .open(job_dir.join("raw-stream.log"))?;
            raw_stream.write_all(event.message.as_bytes())?;
            raw_stream.write_all(b"\n")?;
        }

        Ok(())
    }

    pub fn write_named_artifact(
        &self,
        job_id: &str,
        artifact_name: &str,
        contents: &[u8],
    ) -> io::Result<()> {
        let file_name = Path::new(artifact_name)
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| *value == artifact_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid artifact name"))?;
        let job_dir = self.job_dir(job_id);
        fs::create_dir_all(&job_dir)?;
        fs::write(job_dir.join(file_name), contents)
    }

    pub fn read_events(&self, job_id: &str) -> io::Result<Option<Vec<LogEvent>>> {
        let path = self.job_dir(job_id).join("events.ndjson");
        if !path.exists() {
            return Ok(None);
        }

        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let event = serde_json::from_str(&line).map_err(io::Error::other)?;
            events.push(event);
        }

        Ok(Some(events))
    }

    pub fn read_tail_events(
        &self,
        job_id: &str,
        tail: usize,
        filter: &EventFilter,
    ) -> io::Result<Option<Vec<LogEvent>>> {
        let path = self.job_dir(job_id).join("events.ndjson");
        if !path.exists() {
            return Ok(None);
        }

        let mut file = fs::File::open(path)?;
        read_tail_events_matching_from(&mut file, tail, filter).map(Some)
    }

    pub fn list_job_artifacts(&self, job_id: &str) -> io::Result<Vec<JobArtifact>> {
        let job_dir = self.job_dir(job_id);
        if !job_dir.exists() {
            return Ok(Vec::new());
        }

        let mut artifacts = fs::read_dir(&job_dir)?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let metadata = entry.metadata().ok()?;
                if !metadata.is_file() {
                    return None;
                }

                let name = entry.file_name().into_string().ok()?;
                Some(JobArtifact {
                    name: name.clone(),
                    size_bytes: metadata.len(),
                    download_url: format!("/api/jobs/{job_id}/artifacts/{name}"),
                })
            })
            .collect::<Vec<_>>();
        artifacts.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(artifacts)
    }

    pub fn read_event_summary(&self, job_id: &str) -> io::Result<Option<PersistedEventSummary>> {
        let path = self.job_dir(job_id).join("event-summary.json");
        if !path.exists() {
            return Ok(None);
        }
        serde_json::from_slice(&fs::read(path)?)
            .map(Some)
            .map_err(io::Error::other)
    }

    pub fn artifact_path(&self, job_id: &str, artifact_name: &str) -> Option<PathBuf> {
        let file_name = Path::new(artifact_name).file_name()?.to_str()?;
        if file_name != artifact_name {
            return None;
        }

        let path = self.job_dir(job_id).join(file_name);
        path.exists().then_some(path)
    }

    pub fn job_dir(&self, job_id: &str) -> PathBuf {
        self.root.join("jobs").join(job_id)
    }

    fn write_job(&self, job: &Job) -> io::Result<()> {
        let job_dir = self.job_dir(&job.id);
        fs::create_dir_all(&job_dir)?;
        let json = serde_json::to_vec_pretty(job).map_err(io::Error::other)?;
        fs::write(job_dir.join("job.json"), json)
    }
}

impl JobEvidenceStorePort for ArtifactStore {
    fn read_events(&self, job_id: &str) -> Result<Option<Vec<LogEvent>>, JobEvidenceStoreError> {
        self.read_events(job_id)
            .map_err(|error| JobEvidenceStoreError(error.to_string()))
    }

    fn read_tail_events(
        &self,
        job_id: &str,
        tail: usize,
        filter: &EventFilter,
    ) -> Result<Option<Vec<LogEvent>>, JobEvidenceStoreError> {
        self.read_tail_events(job_id, tail, filter)
            .map_err(|error| JobEvidenceStoreError(error.to_string()))
    }

    fn list_job_artifacts(&self, job_id: &str) -> Result<Vec<JobArtifact>, JobEvidenceStoreError> {
        self.list_job_artifacts(job_id)
            .map_err(|error| JobEvidenceStoreError(error.to_string()))
    }

    fn read_event_summary(
        &self,
        job_id: &str,
    ) -> Result<Option<PersistedEventSummary>, JobEvidenceStoreError> {
        self.read_event_summary(job_id)
            .map_err(|error| JobEvidenceStoreError(error.to_string()))
    }

    fn artifact_path(&self, job_id: &str, artifact_name: &str) -> Option<PathBuf> {
        self.artifact_path(job_id, artifact_name)
    }
}

#[cfg(test)]
fn read_tail_events_from<R: Read + Seek>(reader: &mut R, tail: usize) -> io::Result<Vec<LogEvent>> {
    read_tail_events_matching_from(reader, tail, &EventFilter::default())
}

fn read_tail_events_matching_from<R: Read + Seek>(
    reader: &mut R,
    tail: usize,
    filter: &EventFilter,
) -> io::Result<Vec<LogEvent>> {
    if tail == 0 {
        return Ok(Vec::new());
    }

    let mut position = reader.seek(SeekFrom::End(0))?;
    let mut carry = Vec::new();
    let mut matched = Vec::with_capacity(tail.min(5_000));
    let mut scanned_bytes = 0_usize;

    while position > 0 && matched.len() < tail {
        let chunk_len = usize::try_from(position.min(TAIL_READ_BLOCK_BYTES as u64))
            .map_err(io::Error::other)?;
        position -= chunk_len as u64;
        reader.seek(SeekFrom::Start(position))?;

        let mut chunk = vec![0_u8; chunk_len];
        reader.read_exact(&mut chunk)?;
        scanned_bytes += chunk_len;
        if scanned_bytes > MAX_TAIL_READ_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("requested event tail exceeds {MAX_TAIL_READ_BYTES} scanned bytes"),
            ));
        }

        chunk.extend_from_slice(&carry);
        let mut boundary = chunk.len();
        while matched.len() < tail {
            let Some(newline) = chunk[..boundary].iter().rposition(|byte| *byte == b'\n') else {
                break;
            };
            collect_matching_event(&chunk[newline + 1..boundary], filter, &mut matched)?;
            boundary = newline;
        }
        carry.clear();
        carry.extend_from_slice(&chunk[..boundary]);
        if carry.len() > MAX_PERSISTED_EVENT_LINE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("persisted event line exceeds {MAX_PERSISTED_EVENT_LINE_BYTES} bytes"),
            ));
        }
    }

    if position == 0 && matched.len() < tail {
        collect_matching_event(&carry, filter, &mut matched)?;
    }

    matched.reverse();
    Ok(matched)
}

fn collect_matching_event(
    line: &[u8],
    filter: &EventFilter,
    matched: &mut Vec<LogEvent>,
) -> io::Result<()> {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if line.is_empty() {
        return Ok(());
    }
    if line.len() > MAX_PERSISTED_EVENT_LINE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("persisted event line exceeds {MAX_PERSISTED_EVENT_LINE_BYTES} bytes"),
        ));
    }
    let event: LogEvent = serde_json::from_slice(line).map_err(io::Error::other)?;
    if filter.matches(&event) {
        matched.push(event);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read, Seek, SeekFrom};

    use chrono::Utc;
    use device_hub_shared::{JobStatus, LogChannel, LogLevel, LogSource};

    use super::*;

    fn sample_event(index: i64) -> LogEvent {
        LogEvent {
            ts: chrono::DateTime::from_timestamp(index, 0).expect("valid timestamp"),
            device_id: "board-alpha".into(),
            job_id: "job_123".into(),
            source: LogSource::Serial,
            channel: LogChannel::Stdout,
            level: LogLevel::Info,
            message: format!("line-{index}"),
        }
    }

    fn ndjson(events: &[LogEvent], final_newline: bool) -> Vec<u8> {
        let mut bytes = events
            .iter()
            .map(|event| serde_json::to_string(event).expect("serialize event"))
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes();
        if final_newline {
            bytes.push(b'\n');
        }
        bytes
    }

    #[test]
    fn read_tail_events_returns_last_n_in_original_order() {
        let events = (0..20).map(sample_event).collect::<Vec<_>>();
        let mut reader = Cursor::new(ndjson(&events, true));

        let tail = read_tail_events_from(&mut reader, 4).expect("read tail");

        assert_eq!(
            tail.iter()
                .map(|event| event.message.as_str())
                .collect::<Vec<_>>(),
            vec!["line-16", "line-17", "line-18", "line-19"]
        );
    }

    #[test]
    fn read_tail_events_handles_final_line_with_and_without_newline() {
        let events = (0..3).map(sample_event).collect::<Vec<_>>();

        for final_newline in [false, true] {
            let mut reader = Cursor::new(ndjson(&events, final_newline));
            let tail = read_tail_events_from(&mut reader, 2).expect("read tail");
            assert_eq!(tail.len(), 2);
            assert_eq!(tail[0].message, "line-1");
            assert_eq!(tail[1].message, "line-2");
        }
    }

    #[test]
    fn read_tail_events_handles_n_zero_and_n_larger_than_history() {
        let events = (0..3).map(sample_event).collect::<Vec<_>>();
        let bytes = ndjson(&events, true);

        assert!(
            read_tail_events_from(&mut Cursor::new(bytes.clone()), 0)
                .expect("empty tail")
                .is_empty()
        );
        assert_eq!(
            read_tail_events_from(&mut Cursor::new(bytes), 10)
                .expect("oversized tail")
                .len(),
            3
        );
    }

    #[test]
    fn read_tail_events_does_not_parse_unneeded_old_prefix() {
        let events = (8..10).map(sample_event).collect::<Vec<_>>();
        let mut bytes = b"this old prefix is deliberately not json\n".to_vec();
        bytes.extend(ndjson(&events, true));

        let tail = read_tail_events_from(&mut Cursor::new(bytes), 2).expect("read valid tail");

        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].message, "line-8");
        assert_eq!(tail[1].message, "line-9");
    }

    #[test]
    fn read_tail_events_applies_filter_before_limit() {
        let mut events = (0..12).map(sample_event).collect::<Vec<_>>();
        for (index, event) in events.iter_mut().enumerate() {
            event.level = if index % 3 == 0 {
                LogLevel::Error
            } else {
                LogLevel::Info
            };
        }
        let mut reader = Cursor::new(ndjson(&events, true));
        let filter = device_hub_shared::EventFilter {
            level: Some(LogLevel::Error),
            ..device_hub_shared::EventFilter::default()
        };

        let tail = read_tail_events_matching_from(&mut reader, 2, &filter).expect("filtered tail");

        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].message, "line-6");
        assert_eq!(tail[1].message, "line-9");
    }

    struct CountingReader {
        inner: Cursor<Vec<u8>>,
        bytes_read: usize,
    }

    impl Read for CountingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let read = self.inner.read(buffer)?;
            self.bytes_read += read;
            Ok(read)
        }
    }

    impl Seek for CountingReader {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.inner.seek(position)
        }
    }

    #[test]
    fn read_tail_events_scans_only_the_tail_window() {
        let events = (0..20_000).map(sample_event).collect::<Vec<_>>();
        let bytes = ndjson(&events, true);
        let total_bytes = bytes.len();
        let mut reader = CountingReader {
            inner: Cursor::new(bytes),
            bytes_read: 0,
        };

        let tail = read_tail_events_from(&mut reader, 5).expect("read tail");

        assert_eq!(tail.len(), 5);
        assert!(reader.bytes_read <= TAIL_READ_BLOCK_BYTES);
        assert!(reader.bytes_read < total_bytes / 10);
        eprintln!(
            "tail_events=5 scanned_bytes={} total_bytes={total_bytes}",
            reader.bytes_read
        );
    }

    #[test]
    fn read_tail_events_rejects_an_oversized_single_event() {
        let mut oversized = sample_event(1);
        oversized.message = "x".repeat(MAX_PERSISTED_EVENT_LINE_BYTES + 1);
        let mut reader = Cursor::new(ndjson(&[oversized], true));

        let error = read_tail_events_from(&mut reader, 1).expect_err("oversized event rejected");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("persisted event line exceeds"));
    }

    #[test]
    fn persists_job_metadata_and_events() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let store = ArtifactStore::new(tempdir.path());
        let job = Job {
            id: "job_123".into(),
            device_id: "board-alpha".into(),
            action: "ssh.exec".into(),
            payload: Some("uname -a".into()),
            timeout_secs: 30,
            status: JobStatus::Pending,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        store.init_job(&job).expect("init job");
        store
            .append_event(&LogEvent {
                ts: Utc::now(),
                device_id: job.device_id.clone(),
                job_id: job.id.clone(),
                source: LogSource::Job,
                channel: LogChannel::System,
                level: LogLevel::Info,
                message: "job accepted".into(),
            })
            .expect("append event");

        assert!(store.job_dir(&job.id).join("job.json").exists());
        let events = store
            .read_events(&job.id)
            .expect("read events")
            .expect("persisted events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, "job accepted");
    }

    #[test]
    fn persists_stdout_and_stderr_events_as_raw_stream_artifact() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let store = ArtifactStore::new(tempdir.path());
        let job = Job {
            id: "job_123".into(),
            device_id: "board-alpha".into(),
            action: "ssh.exec".into(),
            payload: Some("uname -a".into()),
            timeout_secs: 30,
            status: JobStatus::Pending,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        store.init_job(&job).expect("init job");

        store
            .append_event(&LogEvent {
                ts: Utc::now(),
                device_id: job.device_id.clone(),
                job_id: job.id.clone(),
                source: LogSource::Job,
                channel: LogChannel::System,
                level: LogLevel::Info,
                message: "job accepted".into(),
            })
            .expect("append system event");
        store
            .append_event(&LogEvent {
                ts: Utc::now(),
                device_id: job.device_id.clone(),
                job_id: job.id.clone(),
                source: LogSource::Ssh,
                channel: LogChannel::Stdout,
                level: LogLevel::Info,
                message: "boot ok".into(),
            })
            .expect("append stdout event");
        store
            .append_event(&LogEvent {
                ts: Utc::now(),
                device_id: job.device_id.clone(),
                job_id: job.id.clone(),
                source: LogSource::Ssh,
                channel: LogChannel::Stderr,
                level: LogLevel::Warn,
                message: "crc mismatch".into(),
            })
            .expect("append stderr event");

        let raw_stream =
            fs::read_to_string(store.job_dir(&job.id).join("raw-stream.log")).expect("raw stream");
        assert_eq!(raw_stream, "boot ok\ncrc mismatch\n");
    }
}
