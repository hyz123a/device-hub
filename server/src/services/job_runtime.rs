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
    if let Some(job) = jobs.update_status(job_id, status) {
        if let Err(error) = artifacts.update_job(&job) {
            eprintln!("failed to persist job {} metadata: {error}", job.id);
        }
    }
}

pub fn succeed_job(
    jobs: &JobStore,
    events: &EventBus,
    artifacts: &ArtifactStore,
    job: &Job,
    message: impl Into<String>,
) {
    if let Some(done) = jobs.update_status(&job.id, JobStatus::Succeeded) {
        if let Err(error) = artifacts.update_job(&done) {
            eprintln!("failed to persist job {} metadata: {error}", done.id);
        }
        events.publish(job_event(&done, message));
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
    if let Some(done) = jobs.update_status(&job.id, JobStatus::Failed) {
        if let Err(error) = artifacts.update_job(&done) {
            eprintln!("failed to persist job {} metadata: {error}", done.id);
        }
        events.publish(job_event(&done, format!("job failed: {message}")));
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
