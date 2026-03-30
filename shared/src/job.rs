use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Job {
    pub id: String,
    pub device_id: String,
    pub action: String,
    pub payload: Option<String>,
    pub timeout_secs: u64,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobArtifact {
    pub name: String,
    pub size_bytes: u64,
    pub download_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobSummary {
    pub headline: String,
    pub request: Option<String>,
    pub outcome: Option<String>,
    pub highlights: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobEvidence {
    pub job: Job,
    pub summary: JobSummary,
    pub artifacts: Vec<JobArtifact>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}
