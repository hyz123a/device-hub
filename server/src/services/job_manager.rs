use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::Utc;
use device_hub_shared::{ActionRequest, Device, Job, JobStatus, LogChannel, LogLevel, LogSource};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::{
    error::AppError,
    ports::{ActionExecutionContext, DynActionRunner},
    services::job_runtime::{adapter_event_with_source, job_event, update_job_status},
    storage::{artifact_store::ArtifactStore, job_store::JobStore},
};

#[derive(Clone)]
pub struct JobManager {
    jobs: JobStore,
    events: crate::events::EventBus,
    artifacts: ArtifactStore,
    runner: DynActionRunner,
    policy_provider: JobPolicyProvider,
    tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    device_locks: Arc<Mutex<HashMap<String, String>>>,
}

#[derive(Clone)]
pub struct JobPolicyProvider {
    resolver: Arc<dyn Fn(&str) -> ActionPolicy + Send + Sync>,
}

impl JobManager {
    pub fn new_with_policy(
        jobs: JobStore,
        events: crate::events::EventBus,
        artifacts: ArtifactStore,
        runner: DynActionRunner,
        policy_provider: JobPolicyProvider,
    ) -> Self {
        Self {
            jobs,
            events,
            artifacts,
            runner,
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
            timeout_secs: policy.timeout_secs(),
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

        let context = ActionExecutionContext::new(
            self.jobs.clone(),
            self.events.clone(),
            self.artifacts.clone(),
        );
        let runner = self.runner.clone();
        let tasks = self.tasks.clone();
        let device_locks = self.device_locks.clone();
        let job_id = job.id.clone();
        let device_id = job.device_id.clone();
        let job_for_task = job.clone();
        let handle = tokio::spawn(async move {
            update_job_status(
                &context.jobs,
                &context.artifacts,
                &job_id,
                JobStatus::Running,
            );
            if let Some(running) = context.jobs.get(&job_id) {
                context.events.publish(job_event(&running, "job running"));
                context.events.publish(adapter_event_with_source(
                    &running,
                    LogSource::System,
                    LogChannel::System,
                    LogLevel::Info,
                    format!(
                        "timeout policy: {}; teardown policy: {}",
                        policy.timeout_label(),
                        policy.teardown_note
                    ),
                ));
            }

            let run = runner.run(context.clone(), job_for_task, device, action, payload);
            let timed_out = match policy.timeout {
                Some(timeout) => tokio::time::timeout(timeout, run).await.is_err(),
                None => {
                    run.await;
                    false
                }
            };

            if timed_out {
                if let Some(running) = context.jobs.get(&job_id) {
                    let timeout_secs = policy.timeout_secs();
                    context.events.publish(adapter_event_with_source(
                        &running,
                        LogSource::System,
                        LogChannel::System,
                        LogLevel::Warn,
                        format!("action timed out after {timeout_secs}s"),
                    ));
                    context.events.publish(adapter_event_with_source(
                        &running,
                        LogSource::System,
                        LogChannel::System,
                        LogLevel::Info,
                        policy.teardown_note,
                    ));
                    crate::services::job_runtime::fail_job(
                        &context.jobs,
                        &context.events,
                        &context.artifacts,
                        &running,
                        format!("action timed out after {timeout_secs}s"),
                    );
                }
            }

            if has_exclusive_lock {
                release_device_lock(&device_locks, &device_id, &job_id);
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
    timeout: Option<Duration>,
    teardown_note: &'static str,
}

impl ActionPolicy {
    pub fn new(timeout: Duration, teardown_note: &'static str) -> Self {
        Self {
            timeout: Some(timeout),
            teardown_note,
        }
    }

    pub fn without_timeout(teardown_note: &'static str) -> Self {
        Self {
            timeout: None,
            teardown_note,
        }
    }

    fn timeout_secs(&self) -> u64 {
        self.timeout.map_or(0, |timeout| timeout.as_secs())
    }

    fn timeout_label(&self) -> String {
        self.timeout
            .map(|timeout| format!("{}s", timeout.as_secs()))
            .unwrap_or_else(|| "none".to_owned())
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
        "ssh.tail" => ActionPolicy::without_timeout(
            "teardown: stop the SSH tail process, release stream resources, and keep captured logs on disk",
        ),
        "serial.tail" => ActionPolicy::without_timeout(
            "teardown: close the serial stream handle and keep captured logs on disk",
        ),
        "serial.write" => ActionPolicy::new(
            Duration::from_secs(10),
            "teardown: close the serial write handle, release the device lock, and persist the final job state",
        ),
        _ => ActionPolicy::new(
            Duration::from_secs(60),
            "teardown: stop the active adapter task and persist the final job state",
        ),
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
