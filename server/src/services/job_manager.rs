use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::Utc;
use device_hub_adapter_jlink::{
    FlashRequest, JlinkAdapter, JlinkAdapterError, JlinkArtifact, JlinkExecution,
};
use device_hub_adapter_serial::SerialAdapter;
use device_hub_adapter_ssh::{SshAdapter, SshCommand, SshStreamChannel};
use device_hub_shared::{
    ActionRequest, Device, Job, JobStatus, LogChannel, LogEvent, LogLevel, LogSource,
};
use serde::Serialize;
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
    jlink_adapter: JlinkAdapter,
    serial_adapter: SerialAdapter,
    policy_provider: JobPolicyProvider,
    tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    device_locks: Arc<Mutex<HashMap<String, String>>>,
}

#[derive(Clone)]
pub struct JobPolicyProvider {
    resolver: Arc<dyn Fn(&str) -> ActionPolicy + Send + Sync>,
}

impl JobManager {
    pub fn new(
        jobs: JobStore,
        events: EventBus,
        artifacts: ArtifactStore,
        ssh_adapter: SshAdapter,
        jlink_adapter: JlinkAdapter,
        serial_adapter: SerialAdapter,
    ) -> Self {
        Self::new_with_policy(
            jobs,
            events,
            artifacts,
            ssh_adapter,
            jlink_adapter,
            serial_adapter,
            JobPolicyProvider::default(),
        )
    }

    pub fn new_with_policy(
        jobs: JobStore,
        events: EventBus,
        artifacts: ArtifactStore,
        ssh_adapter: SshAdapter,
        jlink_adapter: JlinkAdapter,
        serial_adapter: SerialAdapter,
        policy_provider: JobPolicyProvider,
    ) -> Self {
        Self {
            jobs,
            events,
            artifacts,
            ssh_adapter,
            jlink_adapter,
            serial_adapter,
            policy_provider,
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
        let policy = self.policy_provider.resolve(&action);
        let job = Job {
            id: format!("job_{}", Uuid::new_v4().simple()),
            device_id: device.id.clone(),
            action: action.clone(),
            payload: payload.as_ref().and_then(|request| request.payload.clone()),
            timeout_secs: policy.timeout.as_secs(),
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
        let jlink_adapter = self.jlink_adapter.clone();
        let serial_adapter = self.serial_adapter.clone();
        let tasks = self.tasks.clone();
        let device_locks = self.device_locks.clone();
        let job_id = job.id.clone();
        let handle = tokio::spawn(async move {
            update_job_status(&jobs, &artifacts, &job_id, JobStatus::Running);
            if let Some(running) = jobs.get(&job_id) {
                events.publish(job_event(&running, "job running"));
                events.publish(adapter_event_with_source(
                    &running,
                    LogSource::System,
                    LogChannel::System,
                    LogLevel::Info,
                    format!(
                        "timeout policy: {}s; teardown policy: {}",
                        policy.timeout.as_secs(),
                        policy.teardown_note
                    ),
                ));
            }

            let timed_out = tokio::time::timeout(
                policy.timeout,
                execute_action(
                    &jobs,
                    &events,
                    &artifacts,
                    &ssh_adapter,
                    &jlink_adapter,
                    &serial_adapter,
                    &job_id,
                    &device,
                    &action,
                    payload.as_ref(),
                ),
            )
            .await
            .is_err();

            if timed_out {
                if let Some(running) = jobs.get(&job_id) {
                    events.publish(adapter_event_with_source(
                        &running,
                        LogSource::System,
                        LogChannel::System,
                        LogLevel::Warn,
                        format!("action timed out after {}s", policy.timeout.as_secs()),
                    ));
                    events.publish(adapter_event_with_source(
                        &running,
                        LogSource::System,
                        LogChannel::System,
                        LogLevel::Info,
                        policy.teardown_note,
                    ));
                    fail_job(
                        &jobs,
                        &events,
                        &artifacts,
                        &running,
                        format!("action timed out after {}s", policy.timeout.as_secs()),
                    );
                }
            }

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

        let policy = self.policy_provider.resolve(&job.action);
        let handle = self
            .tasks
            .lock()
            .expect("job task registry poisoned")
            .remove(job_id);

        if let Some(handle) = handle {
            self.events.publish(adapter_event_with_source(
                &job,
                LogSource::System,
                LogChannel::System,
                LogLevel::Info,
                format!("cancellation requested; {}", policy.teardown_note),
            ));
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

#[derive(Clone, Copy)]
pub struct ActionPolicy {
    timeout: Duration,
    teardown_note: &'static str,
}

impl ActionPolicy {
    pub fn new(timeout: Duration, teardown_note: &'static str) -> Self {
        Self {
            timeout,
            teardown_note,
        }
    }
}

impl JobPolicyProvider {
    pub fn new(resolver: impl Fn(&str) -> ActionPolicy + Send + Sync + 'static) -> Self {
        Self {
            resolver: Arc::new(resolver),
        }
    }

    pub fn resolve(&self, action: &str) -> ActionPolicy {
        (self.resolver)(action)
    }
}

impl Default for JobPolicyProvider {
    fn default() -> Self {
        Self::new(default_action_policy)
    }
}

fn default_action_policy(action: &str) -> ActionPolicy {
    match action {
        "ssh.exec" => ActionPolicy::new(
            Duration::from_secs(30),
            "teardown: drop the SSH process, release the device lock, and persist collected evidence",
        ),
        "ssh.reboot" => ActionPolicy::new(
            Duration::from_secs(20),
            "teardown: treat the reboot request as fire-and-forget, release the device lock, and rely on reconnect for the next action",
        ),
        "ssh.tail" => ActionPolicy::new(
            Duration::from_secs(3600),
            "teardown: stop the SSH tail process, release stream resources, and keep captured logs on disk",
        ),
        "serial.tail" => ActionPolicy::new(
            Duration::from_secs(3600),
            "teardown: close the serial stream handle and keep captured logs on disk",
        ),
        _ => ActionPolicy::new(
            Duration::from_secs(60),
            "teardown: stop the active adapter task and persist the final job state",
        ),
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
    !matches!(action, "ssh.tail" | "serial.tail")
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
    jlink_adapter: &JlinkAdapter,
    serial_adapter: &SerialAdapter,
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
        "jlink.probe" => {
            execute_jlink_probe(jobs, events, artifacts, jlink_adapter, &job, device).await
        }
        "jlink.flash" => {
            execute_jlink_flash(
                jobs,
                events,
                artifacts,
                jlink_adapter,
                &job,
                device,
                payload,
            )
            .await
        }
        "jlink.verify" => {
            execute_jlink_verify(jobs, events, artifacts, jlink_adapter, &job, device).await
        }
        "jlink.reset" => {
            execute_jlink_reset(jobs, events, artifacts, jlink_adapter, &job, device).await
        }
        "jlink.erase" => {
            execute_jlink_erase(jobs, events, artifacts, jlink_adapter, &job, device).await
        }
        "serial.tail" => {
            execute_serial_tail(jobs, events, artifacts, serial_adapter, &job, device).await
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

async fn execute_serial_tail(
    jobs: &JobStore,
    events: &EventBus,
    artifacts: &ArtifactStore,
    serial_adapter: &SerialAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.serial.as_ref() else {
        fail_job(
            jobs,
            events,
            artifacts,
            job,
            "device is missing serial configuration",
        );
        return;
    };

    events.publish(adapter_event_with_source(
        job,
        LogSource::Serial,
        LogChannel::System,
        LogLevel::Info,
        format!(
            "tailing serial logs from {} @ {} baud",
            config.port, config.baud
        ),
    ));

    match serial_adapter.tail_logs_stream(config).await {
        Ok(mut session) => loop {
            match session.next_chunk().await {
                Ok(Some(chunk)) => events.publish(LogEvent {
                    ts: Utc::now(),
                    device_id: job.device_id.clone(),
                    job_id: job.id.clone(),
                    source: LogSource::Serial,
                    channel: LogChannel::Stdout,
                    level: LogLevel::Info,
                    message: chunk.line,
                }),
                Ok(None) => {
                    succeed_job(jobs, events, artifacts, job, "serial.tail completed");
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

async fn execute_jlink_probe(
    jobs: &JobStore,
    events: &EventBus,
    artifacts: &ArtifactStore,
    jlink_adapter: &JlinkAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.jlink.as_ref() else {
        fail_job(
            jobs,
            events,
            artifacts,
            job,
            "device is missing jlink configuration",
        );
        return;
    };

    events.publish(adapter_event_with_source(
        job,
        LogSource::Jlink,
        LogChannel::System,
        LogLevel::Info,
        format!("probing J-Link {} for {}", config.serial, config.device),
    ));

    match jlink_adapter.probe(config).await {
        Ok(execution) => {
            persist_jlink_execution(events, artifacts, job, &execution);
            succeed_job(jobs, events, artifacts, job, "jlink.probe completed");
        }
        Err(error) => fail_job_with_source(
            jobs,
            events,
            artifacts,
            job,
            LogSource::Jlink,
            persist_jlink_failure(events, artifacts, job, error),
        ),
    }
}

async fn execute_jlink_flash(
    jobs: &JobStore,
    events: &EventBus,
    artifacts: &ArtifactStore,
    jlink_adapter: &JlinkAdapter,
    job: &Job,
    device: &Device,
    payload: Option<&ActionRequest>,
) {
    let Some(config) = device.jlink.as_ref() else {
        fail_job(
            jobs,
            events,
            artifacts,
            job,
            "device is missing jlink configuration",
        );
        return;
    };
    let firmware_path = payload
        .and_then(|request| request.payload.as_deref())
        .map(str::trim)
        .unwrap_or_default();

    events.publish(adapter_event_with_source(
        job,
        LogSource::Jlink,
        LogChannel::System,
        LogLevel::Info,
        format!("flashing {} via J-Link {}", firmware_path, config.serial),
    ));

    match jlink_adapter
        .flash(
            config,
            &FlashRequest {
                firmware_path: firmware_path.to_owned(),
            },
        )
        .await
    {
        Ok(execution) => {
            persist_jlink_execution(events, artifacts, job, &execution);
            succeed_job(jobs, events, artifacts, job, "jlink.flash completed");
        }
        Err(error) => fail_job_with_source(
            jobs,
            events,
            artifacts,
            job,
            LogSource::Jlink,
            persist_jlink_failure(events, artifacts, job, error),
        ),
    }
}

async fn execute_jlink_verify(
    jobs: &JobStore,
    events: &EventBus,
    artifacts: &ArtifactStore,
    jlink_adapter: &JlinkAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.jlink.as_ref() else {
        fail_job(
            jobs,
            events,
            artifacts,
            job,
            "device is missing jlink configuration",
        );
        return;
    };

    events.publish(adapter_event_with_source(
        job,
        LogSource::Jlink,
        LogChannel::System,
        LogLevel::Info,
        format!(
            "verifying target {} via J-Link {}",
            config.device, config.serial
        ),
    ));

    match jlink_adapter.verify(config).await {
        Ok(execution) => {
            persist_jlink_execution(events, artifacts, job, &execution);
            succeed_job(jobs, events, artifacts, job, "jlink.verify completed");
        }
        Err(error) => fail_job_with_source(
            jobs,
            events,
            artifacts,
            job,
            LogSource::Jlink,
            persist_jlink_failure(events, artifacts, job, error),
        ),
    }
}

async fn execute_jlink_reset(
    jobs: &JobStore,
    events: &EventBus,
    artifacts: &ArtifactStore,
    jlink_adapter: &JlinkAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.jlink.as_ref() else {
        fail_job(
            jobs,
            events,
            artifacts,
            job,
            "device is missing jlink configuration",
        );
        return;
    };

    events.publish(adapter_event_with_source(
        job,
        LogSource::Jlink,
        LogChannel::System,
        LogLevel::Info,
        format!(
            "resetting target {} via J-Link {}",
            config.device, config.serial
        ),
    ));

    match jlink_adapter.reset(config).await {
        Ok(execution) => {
            persist_jlink_execution(events, artifacts, job, &execution);
            succeed_job(jobs, events, artifacts, job, "jlink.reset completed");
        }
        Err(error) => fail_job_with_source(
            jobs,
            events,
            artifacts,
            job,
            LogSource::Jlink,
            persist_jlink_failure(events, artifacts, job, error),
        ),
    }
}

async fn execute_jlink_erase(
    jobs: &JobStore,
    events: &EventBus,
    artifacts: &ArtifactStore,
    jlink_adapter: &JlinkAdapter,
    job: &Job,
    device: &Device,
) {
    let Some(config) = device.jlink.as_ref() else {
        fail_job(
            jobs,
            events,
            artifacts,
            job,
            "device is missing jlink configuration",
        );
        return;
    };

    events.publish(adapter_event_with_source(
        job,
        LogSource::Jlink,
        LogChannel::System,
        LogLevel::Info,
        format!(
            "erasing target {} via J-Link {}",
            config.device, config.serial
        ),
    ));

    match jlink_adapter.erase(config).await {
        Ok(execution) => {
            persist_jlink_execution(events, artifacts, job, &execution);
            succeed_job(jobs, events, artifacts, job, "jlink.erase completed");
        }
        Err(error) => fail_job(jobs, events, artifacts, job, error.to_string()),
    }
}

fn publish_stream(events: &EventBus, job: &Job, channel: LogChannel, output: &str) {
    for line in output.lines().filter(|line| !line.is_empty()) {
        events.publish(adapter_event(job, channel.clone(), LogLevel::Info, line));
    }
}

fn persist_jlink_execution<T: Serialize>(
    events: &EventBus,
    artifacts: &ArtifactStore,
    job: &Job,
    execution: &JlinkExecution<T>,
) {
    for artifact in &execution.artifacts {
        publish_jlink_artifact_logs(events, job, artifact);
        if let Err(error) =
            artifacts.write_named_artifact(&job.id, &artifact.name, &artifact.contents)
        {
            eprintln!(
                "failed to persist job {} artifact {}: {error}",
                job.id, artifact.name
            );
        }
    }

    match serde_json::to_vec_pretty(&execution.result) {
        Ok(result_json) => {
            if let Err(error) = artifacts.write_named_artifact(&job.id, "result.json", &result_json)
            {
                eprintln!("failed to persist job {} result.json: {error}", job.id);
            }
        }
        Err(error) => eprintln!("failed to serialize job {} result payload: {error}", job.id),
    }
}

fn publish_jlink_artifact_logs(events: &EventBus, job: &Job, artifact: &JlinkArtifact) {
    let (channel, level) = match artifact.name.as_str() {
        "jlink.stdout.log" => (LogChannel::Stdout, LogLevel::Info),
        "jlink.stderr.log" => (LogChannel::Stderr, LogLevel::Warn),
        _ => return,
    };
    let text = String::from_utf8_lossy(&artifact.contents);
    for line in text.lines().filter(|line| !line.is_empty()) {
        events.publish(adapter_event_with_source(
            job,
            LogSource::Jlink,
            channel.clone(),
            level.clone(),
            line,
        ));
    }
}

fn persist_jlink_failure(
    events: &EventBus,
    artifacts: &ArtifactStore,
    job: &Job,
    error: JlinkAdapterError,
) -> String {
    if let Some(jlink_artifacts) = error.artifacts() {
        for artifact in jlink_artifacts {
            publish_jlink_artifact_logs(events, job, artifact);
            if let Err(write_error) =
                artifacts.write_named_artifact(&job.id, &artifact.name, &artifact.contents)
            {
                eprintln!(
                    "failed to persist job {} artifact {}: {write_error}",
                    job.id, artifact.name
                );
            }
        }
    }

    error.to_string()
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
    fail_job_with_source(jobs, events, artifacts, job, LogSource::Ssh, message);
}

fn fail_job_with_source(
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

fn adapter_event(
    job: &Job,
    channel: LogChannel,
    level: LogLevel,
    message: impl Into<String>,
) -> LogEvent {
    adapter_event_with_source(job, LogSource::Ssh, channel, level, message)
}

fn adapter_event_with_source(
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
