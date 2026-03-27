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

use crate::{error::AppError, events::EventBus, storage::job_store::JobStore};

#[derive(Clone)]
pub struct JobManager {
    jobs: JobStore,
    events: EventBus,
    ssh_adapter: SshAdapter,
    tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
}

impl JobManager {
    pub fn new(jobs: JobStore, events: EventBus, ssh_adapter: SshAdapter) -> Self {
        Self {
            jobs,
            events,
            ssh_adapter,
            tasks: Arc::new(Mutex::new(HashMap::new())),
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

        self.jobs.insert(job.clone());
        self.events.publish(job_event(&job, "job accepted"));

        let jobs = self.jobs.clone();
        let events = self.events.clone();
        let ssh_adapter = self.ssh_adapter.clone();
        let tasks = self.tasks.clone();
        let job_id = job.id.clone();
        let handle = tokio::spawn(async move {
            jobs.update_status(&job_id, JobStatus::Running);
            if let Some(running) = jobs.get(&job_id) {
                events.publish(job_event(&running, "job running"));
            }

            execute_action(
                &jobs,
                &events,
                &ssh_adapter,
                &job_id,
                &device,
                &action,
                payload.as_ref(),
            )
            .await;

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
        let handle = self
            .tasks
            .lock()
            .expect("job task registry poisoned")
            .remove(job_id);

        if let Some(handle) = handle {
            handle.abort();
        }

        let job = self
            .jobs
            .update_status(job_id, JobStatus::Cancelled)
            .ok_or(AppError::JobNotFound)?;
        self.events.publish(job_event(&job, "job cancelled"));
        Ok(job)
    }
}

async fn execute_action(
    jobs: &JobStore,
    events: &EventBus,
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
        "ssh.exec" => execute_ssh_exec(jobs, events, ssh_adapter, &job, device, payload).await,
        "ssh.reboot" => execute_ssh_reboot(jobs, events, ssh_adapter, &job, device).await,
        "ssh.tail" => execute_ssh_tail(jobs, events, ssh_adapter, &job, device, payload).await,
        _ => fail_job(
            jobs,
            events,
            &job,
            format!("unsupported action `{action}` reached executor"),
        ),
    }
}

async fn execute_ssh_exec(
    jobs: &JobStore,
    events: &EventBus,
    ssh_adapter: &SshAdapter,
    job: &Job,
    device: &Device,
    payload: Option<&ActionRequest>,
) {
    let Some(config) = device.ssh.as_ref() else {
        fail_job(jobs, events, job, "device is missing ssh configuration");
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
                succeed_job(jobs, events, job, "ssh.exec completed");
            } else {
                fail_job(
                    jobs,
                    events,
                    job,
                    format!("ssh.exec exited with status {}", result.exit_code),
                );
            }
        }
        Err(error) => fail_job(jobs, events, job, error.to_string()),
    }
}

async fn execute_ssh_reboot(
    jobs: &JobStore,
    events: &EventBus,
    ssh_adapter: &SshAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.ssh.as_ref() else {
        fail_job(jobs, events, job, "device is missing ssh configuration");
        return;
    };

    events.publish(adapter_event(
        job,
        LogChannel::System,
        LogLevel::Info,
        "reboot requested",
    ));

    match ssh_adapter.reboot(config).await {
        Ok(()) => succeed_job(jobs, events, job, "ssh.reboot completed"),
        Err(error) => fail_job(jobs, events, job, error.to_string()),
    }
}

async fn execute_ssh_tail(
    jobs: &JobStore,
    events: &EventBus,
    ssh_adapter: &SshAdapter,
    job: &Job,
    device: &Device,
    payload: Option<&ActionRequest>,
) {
    let Some(config) = device.ssh.as_ref() else {
        fail_job(jobs, events, job, "device is missing ssh configuration");
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
                    succeed_job(jobs, events, job, "ssh.tail completed");
                    break;
                }
                Err(error) => {
                    fail_job(jobs, events, job, error.to_string());
                    break;
                }
            }
        },
        Err(error) => fail_job(jobs, events, job, error.to_string()),
    }
}

fn publish_stream(events: &EventBus, job: &Job, channel: LogChannel, output: &str) {
    for line in output.lines().filter(|line| !line.is_empty()) {
        events.publish(adapter_event(job, channel.clone(), LogLevel::Info, line));
    }
}

fn succeed_job(jobs: &JobStore, events: &EventBus, job: &Job, message: impl Into<String>) {
    jobs.update_status(&job.id, JobStatus::Succeeded);
    if let Some(done) = jobs.get(&job.id) {
        events.publish(job_event(&done, message));
    }
}

fn fail_job(jobs: &JobStore, events: &EventBus, job: &Job, message: impl Into<String>) {
    let message = message.into();
    events.publish(adapter_event(
        job,
        LogChannel::System,
        LogLevel::Error,
        &message,
    ));
    jobs.update_status(&job.id, JobStatus::Failed);
    if let Some(done) = jobs.get(&job.id) {
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
