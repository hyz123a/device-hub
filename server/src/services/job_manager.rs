use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use chrono::Utc;
use device_hub_adapter_ssh::{SshAdapter, SshCommand, SshStreamChannel};
use device_hub_shared::{
    ActionRequest, Device, Job, JobStatus, LogChannel, LogEvent, LogLevel, LogSource,
};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::{
    error::AppError,
    events::EventBus,
    storage::{artifact_store::ArtifactStore, job_store::JobStore},
};

#[derive(Clone)]
pub struct JobManager {
    jobs: JobStore,
    events: EventBus,
    artifacts: ArtifactStore,
    ssh_adapter: SshAdapter,
    tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    device_locks: Arc<Mutex<HashMap<String, String>>>,
}

impl JobManager {
    pub fn new(
        jobs: JobStore,
        events: EventBus,
        artifacts: ArtifactStore,
        ssh_adapter: SshAdapter,
    ) -> Self {
        Self {
            jobs,
            events,
            artifacts,
            ssh_adapter,
            tasks: Arc::new(Mutex::new(HashMap::new())),
            device_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn start(
        &self,
        device: Device,
        action: String,
        payload: Option<ActionRequest>,
    ) -> Result<Job, AppError> {
        let now = Utc::now();
        let job = Job {
            id: format!("job_{}", Uuid::new_v4().simple()),
            device_id: device.id.clone(),
            action: action.clone(),
            status: JobStatus::Pending,
            created_at: now,
            updated_at: now,
        };

        let has_exclusive_lock = action_requires_exclusive_lock(&action);
        if has_exclusive_lock {
            self.try_acquire_device_lock(&device.id, &job.id)?;
        }

        if let Err(error) = self.artifacts.init_job(&job) {
            if has_exclusive_lock {
                release_device_lock(&self.device_locks, &device.id, &job.id);
            }
            return Err(AppError::Internal(error.to_string()));
        }
        self.jobs.insert(job.clone());
        self.events.publish(job_event(&job, "job accepted"));

        let jobs = self.jobs.clone();
        let events = self.events.clone();
        let artifacts = self.artifacts.clone();
        let ssh_adapter = self.ssh_adapter.clone();
        let tasks = self.tasks.clone();
        let device_locks = self.device_locks.clone();
        let job_id = job.id.clone();
        let handle = tokio::spawn(async move {
            update_job_status(&jobs, &artifacts, &job_id, JobStatus::Running);
            if let Some(running) = jobs.get(&job_id) {
                events.publish(job_event(&running, "job running"));
            }

            execute_action(
                &jobs,
                &events,
                &artifacts,
                &ssh_adapter,
                &job_id,
                &device,
                &action,
                payload.as_ref(),
            )
            .await;

            if has_exclusive_lock {
                release_device_lock(&device_locks, &device.id, &job_id);
            }

            tasks
                .lock()
                .expect("job task registry poisoned")
                .remove(&job_id);
        });

        self.tasks
            .lock()
            .expect("job task registry poisoned")
            .insert(job.id.clone(), handle);

        Ok(job)
    }

    pub async fn cancel(&self, job_id: &str) -> Result<Job, AppError> {
        let job = self.jobs.get(job_id).ok_or(AppError::JobNotFound)?;
        if matches!(
            job.status,
            JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled
        ) {
            return Err(AppError::JobNotCancellable(
                job_status_label(&job.status).to_owned(),
            ));
        }

        let handle = self
            .tasks
            .lock()
            .expect("job task registry poisoned")
            .remove(job_id);

        if let Some(handle) = handle {
            handle.abort();
        }

        if action_requires_exclusive_lock(&job.action) {
            release_device_lock(&self.device_locks, &job.device_id, job_id);
        }

        let job = self
            .jobs
            .update_status(job_id, JobStatus::Cancelled)
            .ok_or(AppError::JobNotFound)?;
        self.artifacts
            .update_job(&job)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        self.events.publish(job_event(&job, "job cancelled"));
        Ok(job)
    }

    fn try_acquire_device_lock(&self, device_id: &str, job_id: &str) -> Result<(), AppError> {
        let mut locks = self
            .device_locks
            .lock()
            .expect("device lock registry poisoned");
        if let Some(held_by_job_id) = locks.get(device_id) {
            return Err(AppError::DeviceLocked {
                held_by_job_id: held_by_job_id.clone(),
            });
        }

        locks.insert(device_id.to_owned(), job_id.to_owned());
        Ok(())
    }
}

fn update_job_status(jobs: &JobStore, artifacts: &ArtifactStore, job_id: &str, status: JobStatus) {
    if let Some(job) = jobs.update_status(job_id, status) {
        if let Err(error) = artifacts.update_job(&job) {
            eprintln!("failed to persist job {} metadata: {error}", job.id);
        }
    }
}

fn action_requires_exclusive_lock(action: &str) -> bool {
    !matches!(action, "ssh.tail")
}

fn release_device_lock(
    device_locks: &Arc<Mutex<HashMap<String, String>>>,
    device_id: &str,
    job_id: &str,
) {
    let mut locks = device_locks.lock().expect("device lock registry poisoned");
    if locks
        .get(device_id)
        .is_some_and(|held_by_job_id| held_by_job_id == job_id)
    {
        locks.remove(device_id);
    }
}

fn job_status_label(status: &JobStatus) -> &'static str {
    match status {
        JobStatus::Pending => "pending",
        JobStatus::Running => "running",
        JobStatus::Succeeded => "succeeded",
        JobStatus::Failed => "failed",
        JobStatus::Cancelled => "cancelled",
    }
}

async fn execute_action(
    jobs: &JobStore,
    events: &EventBus,
    artifacts: &ArtifactStore,
    ssh_adapter: &SshAdapter,
    job_id: &str,
    device: &Device,
    action: &str,
    payload: Option<&ActionRequest>,
) {
    let Some(job) = jobs.get(job_id) else {
        return;
    };

    match action {
        "ssh.exec" => {
            execute_ssh_exec(jobs, events, artifacts, ssh_adapter, &job, device, payload).await
        }
        "ssh.reboot" => {
            execute_ssh_reboot(jobs, events, artifacts, ssh_adapter, &job, device).await
        }
        "ssh.tail" => {
            execute_ssh_tail(jobs, events, artifacts, ssh_adapter, &job, device, payload).await
        }
        _ => fail_job(
            jobs,
            events,
            artifacts,
            &job,
            format!("unsupported action `{action}` reached executor"),
        ),
    }
}

async fn execute_ssh_exec(
    jobs: &JobStore,
    events: &EventBus,
    artifacts: &ArtifactStore,
    ssh_adapter: &SshAdapter,
    job: &Job,
    device: &Device,
    payload: Option<&ActionRequest>,
) {
    let Some(config) = device.ssh.as_ref() else {
        fail_job(
            jobs,
            events,
            artifacts,
            job,
            "device is missing ssh configuration",
        );
        return;
    };
    let command = payload
        .and_then(|request| request.payload.as_ref())
        .map(|value| value.trim())
        .unwrap_or_default();

    events.publish(adapter_event(
        job,
        LogChannel::System,
        LogLevel::Info,
        format!("exec: {command}"),
    ));

    match ssh_adapter
        .exec(
            config,
            &SshCommand {
                command: command.to_owned(),
            },
        )
        .await
    {
        Ok(result) => {
            publish_stream(events, job, LogChannel::Stdout, &result.stdout);
            publish_stream(events, job, LogChannel::Stderr, &result.stderr);

            if result.exit_code == 0 {
                succeed_job(jobs, events, artifacts, job, "ssh.exec completed");
            } else {
                fail_job(
                    jobs,
                    events,
                    artifacts,
                    job,
                    format!("ssh.exec exited with status {}", result.exit_code),
                );
            }
        }
        Err(error) => fail_job(jobs, events, artifacts, job, error.to_string()),
    }
}

async fn execute_ssh_reboot(
    jobs: &JobStore,
    events: &EventBus,
    artifacts: &ArtifactStore,
    ssh_adapter: &SshAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.ssh.as_ref() else {
        fail_job(
            jobs,
            events,
            artifacts,
            job,
            "device is missing ssh configuration",
        );
        return;
    };

    events.publish(adapter_event(
        job,
        LogChannel::System,
        LogLevel::Info,
        "reboot requested",
    ));

    match ssh_adapter.reboot(config).await {
        Ok(()) => succeed_job(jobs, events, artifacts, job, "ssh.reboot completed"),
        Err(error) => fail_job(jobs, events, artifacts, job, error.to_string()),
    }
}

async fn execute_ssh_tail(
    jobs: &JobStore,
    events: &EventBus,
    artifacts: &ArtifactStore,
    ssh_adapter: &SshAdapter,
    job: &Job,
    device: &Device,
    payload: Option<&ActionRequest>,
) {
    let Some(config) = device.ssh.as_ref() else {
        fail_job(
            jobs,
            events,
            artifacts,
            job,
            "device is missing ssh configuration",
        );
        return;
    };
    let path = payload
        .and_then(|request| request.payload.as_ref())
        .map(|value| value.trim())
        .unwrap_or_default();

    events.publish(adapter_event(
        job,
        LogChannel::System,
        LogLevel::Info,
        format!("tailing logs from {path}"),
    ));

    match ssh_adapter.tail_logs_stream(config, path).await {
        Ok(mut session) => loop {
            match session.next_chunk().await {
                Ok(Some(chunk)) => events.publish(adapter_event(
                    job,
                    match chunk.channel {
                        SshStreamChannel::Stdout => LogChannel::Stdout,
                        SshStreamChannel::Stderr => LogChannel::Stderr,
                    },
                    LogLevel::Info,
                    chunk.line,
                )),
                Ok(None) => {
                    succeed_job(jobs, events, artifacts, job, "ssh.tail completed");
                    break;
                }
                Err(error) => {
                    fail_job(jobs, events, artifacts, job, error.to_string());
                    break;
                }
            }
        },
        Err(error) => fail_job(jobs, events, artifacts, job, error.to_string()),
    }
}

fn publish_stream(events: &EventBus, job: &Job, channel: LogChannel, output: &str) {
    for line in output.lines().filter(|line| !line.is_empty()) {
        events.publish(adapter_event(job, channel.clone(), LogLevel::Info, line));
    }
}

fn succeed_job(
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

fn fail_job(
    jobs: &JobStore,
    events: &EventBus,
    artifacts: &ArtifactStore,
    job: &Job,
    message: impl Into<String>,
) {
    let message = message.into();
    events.publish(adapter_event(
        job,
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

fn adapter_event(
    job: &Job,
    channel: LogChannel,
    level: LogLevel,
    message: impl Into<String>,
) -> LogEvent {
    LogEvent {
        ts: Utc::now(),
        device_id: job.device_id.clone(),
        job_id: job.id.clone(),
        source: LogSource::Ssh,
        channel,
        level,
        message: message.into(),
    }
}

fn job_event(job: &Job, message: impl Into<String>) -> LogEvent {
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
