use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use chrono::Utc;
use device_hub_shared::{Job, JobStatus};

#[derive(Clone, Default)]
pub struct JobStore {
    jobs: Arc<Mutex<HashMap<String, Job>>>,
    runtime_endpoints: Arc<Mutex<HashMap<String, String>>>,
}

impl JobStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, job: Job) {
        self.jobs
            .lock()
            .expect("job store poisoned")
            .insert(job.id.clone(), job);
    }

    pub fn get(&self, id: &str) -> Option<Job> {
        self.jobs
            .lock()
            .expect("job store poisoned")
            .get(id)
            .cloned()
    }

    pub fn list(&self) -> Vec<Job> {
        let mut jobs = self
            .jobs
            .lock()
            .expect("job store poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        jobs.sort_by_key(|job| std::cmp::Reverse(job.created_at));
        jobs
    }

    pub fn update_status(&self, id: &str, status: JobStatus) -> Option<Job> {
        let terminal = matches!(
            status,
            JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled
        );
        let mut jobs = self.jobs.lock().expect("job store poisoned");
        let job = jobs.get_mut(id)?;
        job.status = status;
        job.updated_at = Utc::now();
        let job = job.clone();
        drop(jobs);
        if terminal {
            self.runtime_endpoints
                .lock()
                .expect("job runtime endpoint store poisoned")
                .remove(id);
        }
        Some(job)
    }

    pub fn set_runtime_endpoint(&self, id: &str, endpoint: String) {
        self.runtime_endpoints
            .lock()
            .expect("job runtime endpoint store poisoned")
            .insert(id.to_owned(), endpoint);
    }

    pub fn clear_runtime_endpoint(&self, id: &str) {
        self.runtime_endpoints
            .lock()
            .expect("job runtime endpoint store poisoned")
            .remove(id);
    }

    pub fn runtime_endpoint(&self, id: &str) -> Option<String> {
        self.runtime_endpoints
            .lock()
            .expect("job runtime endpoint store poisoned")
            .get(id)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[test]
    fn terminal_status_clears_runtime_endpoint() {
        let store = JobStore::new();
        let now = Utc::now();
        store.insert(Job {
            id: "job-1".into(),
            device_id: "device-1".into(),
            action: "serial.tail".into(),
            payload: None,
            timeout_secs: 0,
            status: JobStatus::Running,
            created_at: now,
            updated_at: now,
        });
        store.set_runtime_endpoint("job-1", "COM24".into());

        assert_eq!(store.runtime_endpoint("job-1").as_deref(), Some("COM24"));
        store.update_status("job-1", JobStatus::Cancelled);
        assert_eq!(store.runtime_endpoint("job-1"), None);
    }

    #[test]
    fn disconnected_runtime_can_clear_endpoint_without_finishing_job() {
        let store = JobStore::new();
        let now = Utc::now();
        store.insert(Job {
            id: "job-1".into(),
            device_id: "device-1".into(),
            action: "serial.tail".into(),
            payload: None,
            timeout_secs: 0,
            status: JobStatus::Running,
            created_at: now,
            updated_at: now,
        });
        store.set_runtime_endpoint("job-1", "COM24".into());

        store.clear_runtime_endpoint("job-1");

        assert_eq!(store.runtime_endpoint("job-1"), None);
        assert_eq!(store.get("job-1").expect("job").status, JobStatus::Running);
    }
}
