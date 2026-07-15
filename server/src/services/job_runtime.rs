use chrono::Utc;
use device_hub_shared::{Job, JobStatus, LogChannel, LogEvent, LogLevel, LogSource};

use crate::{
    events::EventBus,
    storage::{artifact_store::ArtifactStore, job_store::JobStore},
};

pub fn update_job_status(
    jobs: &JobStore,
    artifacts: &ArtifactStore,
    job_id: &str,
    status: JobStatus,
) {
    if let Some(job) = jobs.update_status(job_id, status)
        && let Err(error) = artifacts.update_job(&job)
    {
        eprintln!("failed to persist job {} metadata: {error}", job.id);
    }
}

pub fn succeed_job(
    jobs: &JobStore,
    events: &EventBus,
    artifacts: &ArtifactStore,
    job: &Job,
    message: impl Into<String>,
) {
    let mut completion_event_job = job.clone();
    completion_event_job.status = JobStatus::Succeeded;
    events.publish(job_event(&completion_event_job, message));
    let persistence_result = events.flush_job(&job.id);

    if let Some(done) = jobs.update_status(
        &job.id,
        if persistence_result.is_ok() {
            JobStatus::Succeeded
        } else {
            JobStatus::Failed
        },
    ) && let Err(error) = artifacts.update_job(&done)
    {
        eprintln!("failed to persist job {} metadata: {error}", done.id);
    }
}

pub fn fail_job(
    jobs: &JobStore,
    events: &EventBus,
    artifacts: &ArtifactStore,
    job: &Job,
    message: impl Into<String>,
) {
    fail_job_with_source(jobs, events, artifacts, job, LogSource::Ssh, message);
}

pub fn fail_job_with_source(
    jobs: &JobStore,
    events: &EventBus,
    artifacts: &ArtifactStore,
    job: &Job,
    source: LogSource,
    message: impl Into<String>,
) {
    let message = message.into();
    events.publish(adapter_event_with_source(
        job,
        source,
        LogChannel::System,
        LogLevel::Error,
        &message,
    ));
    let mut failed_event_job = job.clone();
    failed_event_job.status = JobStatus::Failed;
    events.publish(job_event(
        &failed_event_job,
        format!("job failed: {message}"),
    ));
    let _ = events.flush_job(&job.id);

    if let Some(done) = jobs.update_status(&job.id, JobStatus::Failed)
        && let Err(error) = artifacts.update_job(&done)
    {
        eprintln!("failed to persist job {} metadata: {error}", done.id);
    }
}

pub fn adapter_event(
    job: &Job,
    channel: LogChannel,
    level: LogLevel,
    message: impl Into<String>,
) -> LogEvent {
    adapter_event_with_source(job, LogSource::Ssh, channel, level, message)
}

pub fn adapter_event_with_source(
    job: &Job,
    source: LogSource,
    channel: LogChannel,
    level: LogLevel,
    message: impl Into<String>,
) -> LogEvent {
    LogEvent {
        ts: Utc::now(),
        device_id: job.device_id.clone(),
        job_id: job.id.clone(),
        source,
        channel,
        level,
        message: message.into(),
    }
}

pub fn job_event(job: &Job, message: impl Into<String>) -> LogEvent {
    LogEvent {
        ts: Utc::now(),
        device_id: job.device_id.clone(),
        job_id: job.id.clone(),
        source: LogSource::Job,
        channel: LogChannel::System,
        level: LogLevel::Info,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, mpsc};

    use chrono::Utc;
    use device_hub_shared::{Job, JobStatus};

    use super::succeed_job;
    use crate::{
        events::EventBus,
        ports::{EventPersistenceError, EventPersistencePort},
        storage::{artifact_store::ArtifactStore, job_store::JobStore},
    };

    struct BlockingPersistence {
        flush_entered: mpsc::SyncSender<()>,
        flush_release: Mutex<mpsc::Receiver<()>>,
    }

    impl EventPersistencePort for BlockingPersistence {
        fn append_event(
            &self,
            _event: device_hub_shared::LogEvent,
        ) -> Result<(), EventPersistenceError> {
            Ok(())
        }

        fn flush_job(&self, _job_id: &str) -> Result<(), EventPersistenceError> {
            self.flush_entered
                .send(())
                .map_err(|error| EventPersistenceError(error.to_string()))?;
            self.flush_release
                .lock()
                .expect("flush release poisoned")
                .recv()
                .map_err(|error| EventPersistenceError(error.to_string()))
        }

        fn shutdown(&self) -> Result<(), EventPersistenceError> {
            Ok(())
        }
    }

    #[test]
    fn artifact_writer_flushes_before_job_completion_is_observable() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let artifacts = ArtifactStore::new(tempdir.path());
        let jobs = JobStore::new();
        let job = Job {
            id: "job-a".into(),
            device_id: "board-alpha".into(),
            action: "ssh.exec".into(),
            payload: None,
            timeout_secs: 30,
            status: JobStatus::Running,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        jobs.insert(job.clone());
        artifacts.init_job(&job).expect("init job");
        let (flush_entered, entered) = mpsc::sync_channel(1);
        let (release, flush_release) = mpsc::sync_channel(1);
        let events = EventBus::new(
            16,
            Some(Arc::new(BlockingPersistence {
                flush_entered,
                flush_release: Mutex::new(flush_release),
            })),
        );

        let worker_jobs = jobs.clone();
        let worker_events = events.clone();
        let worker_artifacts = artifacts.clone();
        let handle = std::thread::spawn(move || {
            succeed_job(
                &worker_jobs,
                &worker_events,
                &worker_artifacts,
                &job,
                "done",
            );
        });
        entered.recv().expect("flush entered");

        assert_eq!(
            jobs.get("job-a").expect("job exists").status,
            JobStatus::Running
        );
        release.send(()).expect("release flush");
        handle.join().expect("completion thread");
        assert_eq!(
            jobs.get("job-a").expect("job exists").status,
            JobStatus::Succeeded
        );
    }
}
