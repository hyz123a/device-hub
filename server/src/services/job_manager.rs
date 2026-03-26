use std::{collections::HashMap, sync::{Arc, Mutex}, time::Duration};

use chrono::Utc;
use device_hub_shared::{ActionRequest, Job, JobStatus, LogChannel, LogEvent, LogLevel, LogSource};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::{error::AppError, events::EventBus, storage::job_store::JobStore};

#[derive(Clone)]
pub struct JobManager {
    jobs: JobStore,
    events: EventBus,
    tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
}

impl JobManager {
    pub fn new(jobs: JobStore, events: EventBus) -> Self {
        Self {
            jobs,
            events,
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn start(
        &self,
        device_id: String,
        action: String,
        payload: Option<ActionRequest>,
    ) -> Result<Job, AppError> {
        let now = Utc::now();
        let job = Job {
            id: format!("job_{}", Uuid::new_v4().simple()),
            device_id: device_id.clone(),
            action: action.clone(),
            status: JobStatus::Pending,
            created_at: now,
            updated_at: now,
        };

        self.jobs.insert(job.clone());
        self.events.publish(job_event(&job, "job accepted"));

        let jobs = self.jobs.clone();
        let events = self.events.clone();
        let job_id = job.id.clone();
        let handle = tokio::spawn(async move {
            jobs.update_status(&job_id, JobStatus::Running);
            if let Some(running) = jobs.get(&job_id) {
                events.publish(job_event(&running, "job running"));
                if let Some(request) = payload.as_ref().and_then(|value| value.payload.as_ref()) {
                    events.publish(LogEvent {
                        ts: Utc::now(),
                        device_id: running.device_id.clone(),
                        job_id: running.id.clone(),
                        source: LogSource::Job,
                        channel: LogChannel::Stdout,
                        level: LogLevel::Info,
                        message: format!("payload: {request}"),
                    });
                }
            }

            tokio::time::sleep(Duration::from_millis(250)).await;

            jobs.update_status(&job_id, JobStatus::Succeeded);
            if let Some(done) = jobs.get(&job_id) {
                events.publish(job_event(&done, format!("stub completed for `{action}`")));
            }
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
