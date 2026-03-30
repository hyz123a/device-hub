use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    path::PathBuf,
};

use device_hub_shared::{Job, LogEvent};

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
        file.write_all(b"\n")
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

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use device_hub_shared::{JobStatus, LogChannel, LogLevel, LogSource};

    use super::*;

    #[test]
    fn persists_job_metadata_and_events() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let store = ArtifactStore::new(tempdir.path());
        let job = Job {
            id: "job_123".into(),
            device_id: "board-alpha".into(),
            action: "ssh.exec".into(),
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
}
