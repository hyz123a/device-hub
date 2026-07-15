use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use chrono::Utc;
use device_hub_shared::{Job, JobStatus};

#[derive(Clone, Default)]
pub struct JobStore {
    jobs: Arc<Mutex<HashMap<String, Job>>>,
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
        let mut jobs = self.jobs.lock().expect("job store poisoned");
        let job = jobs.get_mut(id)?;
        job.status = status;
        job.updated_at = Utc::now();
        Some(job.clone())
    }
}
